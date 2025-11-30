# Client Main Thread Refactoring Plan

## Obiettivo

Ristrutturare l'architettura del client per prepararlo ai GraphicEngines:
1. Il **main thread** rimane libero per ospitare il graphic engine (Bevy richiede il main thread su macOS/Windows)
2. Tutta la logica attuale (network, mods, JS runtime) viene spostata in un **worker thread**
3. Gestione graceful dello shutdown del worker

## Architettura Attuale

```
Main Thread (tokio runtime)
├── Logging setup
├── Args parsing
├── Locale initialization
├── Network connection
├── Mod loading
├── JS runtime event loop
└── Shutdown
```

## Architettura Proposta

```
Main Thread (std::thread, NO tokio)
├── Logging setup
├── Args parsing
├── Spawn Worker Thread
├── Wait for:
│   ├── Worker thread completion
│   └── (Future) GraphicEngine events
└── Exit

Worker Thread (tokio runtime)
├── Locale initialization
├── Network connection
├── Mod loading
├── JS runtime event loop
├── Handle shutdown:
│   ├── system.exit() from JS
│   ├── CTRL+C not handled by mods
│   └── Connection closed
└── Signal main thread and exit
```

## Comunicazione tra Thread

```rust
/// Canale per comunicare dal worker al main thread
enum WorkerMessage {
    /// Worker thread terminato normalmente
    Terminated { exit_code: i32 },
    /// Worker thread crashato
    Error { message: String },
}

/// Canale per comunicare dal main thread al worker
enum MainMessage {
    /// Richiesta di shutdown graceful
    Shutdown,
}
```

## Dettagli Implementativi

### 1. Main Thread

Il main thread NON usa tokio. Usa solo `std::thread` e `std::sync::mpsc` per:
- Creare il worker thread
- Attendere messaggi dal worker
- (Futuro) Eseguire il graphic engine loop

```rust
fn main() {
    // Setup logging (come prima)
    setup_logging(&args);

    // Crea i canali di comunicazione
    let (worker_tx, main_rx) = std::sync::mpsc::channel::<WorkerMessage>();
    let (main_tx, worker_rx) = std::sync::mpsc::channel::<MainMessage>();

    // Spawn del worker thread
    let worker_handle = std::thread::spawn(move || {
        worker_main(args, worker_tx, worker_rx)
    });

    // Main loop (per ora solo attende il worker)
    let exit_code = loop {
        match main_rx.recv() {
            Ok(WorkerMessage::Terminated { exit_code }) => {
                break exit_code;
            }
            Ok(WorkerMessage::Error { message }) => {
                eprintln!("Worker error: {}", message);
                break 1;
            }
            Err(_) => {
                // Channel closed = worker crashed
                break 1;
            }
        }
    };

    // Attendi che il worker thread termini
    let _ = worker_handle.join();

    // (Futuro) Qui ci sarà anche il loop del graphic engine
    // Per ora, se il worker è terminato, usciamo

    std::process::exit(exit_code);
}
```

### 2. Worker Thread

Il worker thread usa tokio come runtime async:

```rust
fn worker_main(
    args: Args,
    tx: std::sync::mpsc::Sender<WorkerMessage>,
    rx: std::sync::mpsc::Receiver<MainMessage>,
) {
    // Crea il tokio runtime per questo thread
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("Failed to create tokio runtime");

    // Esegui la logica async
    let exit_code = runtime.block_on(async {
        worker_async_main(args, rx).await
    });

    // Notifica il main thread
    let _ = tx.send(WorkerMessage::Terminated { exit_code });
}

async fn worker_async_main(args: Args, main_rx: std::sync::mpsc::Receiver<MainMessage>) -> i32 {
    // Tutta la logica attuale di connect_to_game_server e main
    // ...

    // Il main loop deve anche controllare main_rx per shutdown requests
}
```

### 3. Gestione CTRL+C

CTRL+C deve essere catturato nel worker thread (dove c'è tokio):
- Se un mod gestisce `TerminalKeyPressed` con CTRL+C e marca `handled = true`, nulla cambia
- Se nessun mod gestisce CTRL+C, il worker termina

Il main thread non deve catturare CTRL+C direttamente - lo lascia gestire al worker.

### 4. Shutdown da system.exit()

Quando un mod chiama `system.exit(code)`:
1. Il worker riceve lo shutdown request
2. Il worker fa cleanup (chiude connessioni, salva stato)
3. Il worker invia `WorkerMessage::Terminated { exit_code }` al main
4. Il worker thread termina
5. Il main thread riceve il messaggio e se non ci sono altre attività, termina

## File da Modificare

### `apps/stam_client/src/main.rs`

1. **Rimuovere `#[tokio::main]`** - il main thread non usa tokio
2. **Creare `struct Args`** parsing con clap (rimane uguale)
3. **Creare `fn main()`** che:
   - Setup logging
   - Parse args
   - Spawn worker thread
   - Attende il worker
4. **Creare `fn worker_main()`** che:
   - Crea tokio runtime
   - Esegue la logica async
5. **Spostare tutta la logica attuale** in `async fn worker_async_main()`

## Fasi di Implementazione

### Fase 1: Estrazione della logica async
- Creare `worker_async_main()` con tutta la logica attuale
- Mantenere `#[tokio::main]` per ora

### Fase 2: Creazione del worker thread
- Rimuovere `#[tokio::main]`
- Creare `worker_main()` con tokio runtime locale
- Creare i canali di comunicazione
- Il main thread spawna il worker e attende

### Fase 3: Gestione shutdown
- Verificare che system.exit() funzioni correttamente
- Verificare che CTRL+C funzioni correttamente
- Testare vari scenari di shutdown

## Note Importanti

1. **Logging**: Il setup del logging deve avvenire nel main thread PRIMA di spawnare il worker, così il worker può loggare subito.

2. **Panic handling**: Se il worker fa panic, il main thread deve catturarlo e uscire gracefully.

3. **Tokio runtime**: Usiamo `new_current_thread()` invece di `new_multi_thread()` perché il worker è single-threaded e non abbiamo bisogno di parallelismo interno.

4. **Future GraphicEngine**: Quando implementeremo il graphic engine:
   - Il main thread eseguirà il loop Bevy/winit
   - Il worker comunicherà col graphic engine via channels
   - Se l'utente chiude la finestra, il main invia `MainMessage::Shutdown` al worker
