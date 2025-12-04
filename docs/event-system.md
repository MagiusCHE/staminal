# Event System

This document describes the Staminal event system architecture, focusing on custom events dispatched via `system.sendEvent()`.

## Overview

The event system allows mods to communicate with each other through custom events. A mod can register handlers for specific event names, and other mods can dispatch those events to trigger the handlers.

## API

### Registering an Event Handler

```javascript
// In onAttach() or onBootstrap()
system.registerEvent("EventName", handlerFunction, priority);
```

- **eventName**: String identifier for the event
- **handlerFunction**: `(req, res) => void` - Function called when event is dispatched
- **priority**: Number (lower = called first)

### Dispatching an Event

```javascript
const result = await system.sendEvent("EventName", arg1, arg2, ...);
if (result.handled) {
    console.log("Event was handled");
}
```

Returns a Promise that resolves to an object containing:
- `handled: boolean` - Whether any handler marked the event as handled
- Any custom properties set by handlers

## Architecture

### The Challenge: Cross-Mod Event Dispatch

Each mod runs in its own JavaScript context (rquickjs `AsyncContext`). When mod A dispatches an event that mod B has registered a handler for, we need to:

1. Execute code in mod B's context (where the handler lives)
2. Return the result to mod A's context (where `sendEvent` was called)

This creates a synchronization challenge because:
- The JavaScript runtime is single-threaded
- We cannot block the JS thread while waiting for cross-context operations
- Calling `runtime.idle().await` while JS code is awaiting a response causes **deadlock**

### Solution: Channel-Based Dispatch with Synchronous Response Values

```
┌─────────────────────────────────────────────────────────────────────┐
│                         MOD A (caller)                               │
│  const result = await system.sendEvent("AppStart");                 │
│                           │                                          │
│                           ▼                                          │
│  ┌─────────────────────────────────────────────────────────────┐    │
│  │ JS Binding: send_event()                                     │    │
│  │  1. Serialize args to JSON                                   │    │
│  │  2. Send request via mpsc channel                            │    │
│  │  3. Await response via oneshot channel  ◄─────────────────┐  │    │
│  └─────────────────────────────────────────────────────────────┘ │  │
└──────────────────────────────────────────────────────────────────│──┘
                                                                   │
┌──────────────────────────────────────────────────────────────────│──┐
│                      MAIN EVENT LOOP                             │  │
│  ┌─────────────────────────────────────────────────────────────┐ │  │
│  │ tokio::select! {                                             │ │  │
│  │   request = send_event_rx.recv() => {                        │ │  │
│  │     // Dispatch to all runtime adapters                      │ │  │
│  │     let response = runtime_manager.dispatch_custom_event();  │ │  │
│  │     request.response_tx.send(response); ─────────────────────┘ │  │
│  │   }                                                           │  │
│  │ }                                                             │  │
│  └─────────────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────────────┘
                                │
                                ▼
┌─────────────────────────────────────────────────────────────────────┐
│                    JS RUNTIME ADAPTER                                │
│  dispatch_custom_event():                                           │
│    for handler in handlers:                                         │
│      1. Get handler's mod context                                   │
│      2. Create req/res objects                                      │
│      3. Call handler function                                       │
│      4. Read response values IMMEDIATELY (no idle().await!)         │
│      5. Aggregate results                                           │
└─────────────────────────────────────────────────────────────────────┘
                                │
                                ▼
┌─────────────────────────────────────────────────────────────────────┐
│                         MOD B (handler)                              │
│  const onAppStart = (req, res) => {                                 │
│      res.handled = true;  // ✓ Set BEFORE any await                 │
│      await doAsyncWork(); // Async work executes later              │
│  };                                                                 │
└─────────────────────────────────────────────────────────────────────┘
```

### Why We Cannot Call `runtime.idle().await`

The deadlock scenario:

1. **Mod A** calls `await system.sendEvent("AppStart")`
2. The JS binding sends the request and awaits the response (`response_rx.await`)
3. **Mod A's JS context is now suspended** waiting for the response
4. The main loop receives the request and calls `dispatch_custom_event()`
5. If we called `runtime.idle().await` here, it would wait for all pending JS jobs
6. But **Mod A's pending job** is waiting for the response we haven't sent yet
7. **DEADLOCK**: `idle()` waits for Mod A, Mod A waits for us

### The Synchronous Response Rule

**Handler response values must be set SYNCHRONOUSLY, before any `await` points.**

```javascript
// ✓ CORRECT - response values set before await
const onAppStart = async (req, res) => {
    res.handled = true;           // Captured!
    res.customProp = "value";     // Captured!

    await doAsyncWork();          // Executes later via event loop

    // Values set here are NOT captured in sendEvent result
};

// ✗ WRONG - response values set after await
const onAppStart = async (req, res) => {
    await doAsyncWork();
    res.handled = true;           // NOT captured - too late!
};

// ✓ ALSO CORRECT - synchronous handler
const onAppStart = (req, res) => {
    res.handled = true;           // Captured!
    // No async operations
};
```

### What Happens to Async Work?

Async operations in handlers still execute - they just don't block the `sendEvent` response:

1. Handler is called, returns a Promise
2. Response values are read immediately (sync values only)
3. Response is sent back to caller
4. The Promise continues executing via the main JS event loop (`run_js_event_loop`)
5. Any async operations (API calls, timers, etc.) complete normally

This means you can still do async work in handlers:

```javascript
const onAppStart = async (req, res) => {
    res.handled = true;  // Set immediately

    // These execute asynchronously after sendEvent returns
    const windows = await graphic.getWindows();
    for (const win of windows) {
        await win.clearWidgets();
    }
};
```

## Flow Summary

1. **Caller** invokes `await system.sendEvent("EventName", ...args)`
2. **JS Binding** serializes args and sends request through mpsc channel
3. **Main Loop** receives request, calls `RuntimeManager::dispatch_custom_event()`
4. **Runtime Adapter** iterates handlers, calls each in the correct mod context
5. **Handler** executes, sets response values synchronously
6. **Runtime Adapter** reads response values immediately (no waiting for Promises)
7. **Main Loop** sends aggregated response through oneshot channel
8. **JS Binding** receives response, returns to caller
9. **Async work** in handlers continues via the JS event loop

## Best Practices

1. **Always set `res.handled = true` first** if your handler handles the event
2. **Set all response properties before any `await`**
3. **Use async handlers freely** for work that doesn't need to be in the response
4. **Check `result.handled`** to know if any handler processed the event

## Error Handling

If no handlers are registered for an event:
- `result.handled` will be `false`
- No error is thrown

If a handler throws an error:
- The error is logged
- Other handlers continue to execute
- The event may still be marked as handled by other handlers

## System Events Behavior

The synchronous response rule applies differently depending on the event type:

### Events Requiring Synchronous `res.handled`

These events read `res.handled` immediately after calling the handler. If your handler is `async`, you **MUST** set `res.handled` before any `await`:

| Event | Description |
|-------|-------------|
| **Custom Events** (`sendEvent`) | User-defined events for mod-to-mod communication |
| **TerminalKeyPressed** | Terminal keyboard input handling |

```javascript
// ✓ CORRECT for TerminalKeyPressed
system.registerEvent(system.TerminalKeyPressed, async (req, res) => {
    if (req.combo === "Ctrl+Q") {
        res.handled = true;  // Must be set BEFORE await!
        await system.exit(0);
    }
}, 0);

// ✗ WRONG - handled set after await
system.registerEvent(system.TerminalKeyPressed, async (req, res) => {
    if (req.combo === "Ctrl+Q") {
        await system.exit(0);
        res.handled = true;  // NOT captured - too late!
    }
}, 0);
```

### Events With Different Behavior

These events do NOT read `res.handled` from async handlers. If the handler returns a Promise, `handled` is automatically set to `true`:

| Event | Description | Behavior |
|-------|-------------|----------|
| **GraphicEngineReady** | Fired when graphic engine is initialized | If async, returns `true` immediately |
| **GraphicEngineWindowClosed** | Fired when a window is closed | If async, returns `true` immediately |

For these events, the important thing is that the handler is **triggered**. The actual async work (like creating windows) happens via the JS event loop after the event dispatch returns.

```javascript
// ✓ CORRECT for GraphicEngineReady - async work is fine
system.registerEvent(system.GraphicEngineReady, async (req, res) => {
    // No need to set res.handled before await - it's automatic for async handlers
    const win = await graphic.createWindow({ title: "My Game" });
    await win.addWidget({ type: "label", text: "Hello!" });
}, 0);

// ✓ ALSO CORRECT - synchronous handler
system.registerEvent(system.GraphicEngineReady, (req, res) => {
    res.handled = true;  // This IS read for sync handlers
    // Start async work without awaiting
    initializeUI();
}, 0);
```

### Summary Table

| Event Type | Sync Handler | Async Handler |
|------------|--------------|---------------|
| **Custom Events** | `res.handled` read | `res.handled` must be set before `await` |
| **TerminalKeyPressed** | `res.handled` read | `res.handled` must be set before `await` |
| **GraphicEngineReady** | `res.handled` read | Returns `true` automatically |
| **GraphicEngineWindowClosed** | `res.handled` read | Returns `true` automatically |
