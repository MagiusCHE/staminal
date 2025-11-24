import { debugDump } from "./helper.js";

// This function is called when the mod is attached to the game client.
export function onAttach() {
    console.log("Attached.");
}

// Called when client has loaded all bootstppinng mods and before activate or check other mods.
export function onBootstrap() {
    console.log("Bootstrap...");
    debugDump();

    // Test setTimeout with various delays
    console.log("Testing setTimeout...");
    setTimeout(() => {
        console.log("setTimeout: 500ms fired!");
    }, 500);

    setTimeout(() => {
        console.log("setTimeout: 1000ms fired!");
    }, 1000);

    setTimeout(() => {
        console.log("setTimeout: 2000ms fired!");
    }, 2000);

    // Test setInterval
    let counter = 0;
    const intervalId = setInterval(() => {
        counter++;
        console.log(`setInterval: tick ${counter}`);
        if (counter >= 3) {
            console.log("setInterval: clearing after 3 ticks");
            clearInterval(intervalId);
        }
    }, 800);
}