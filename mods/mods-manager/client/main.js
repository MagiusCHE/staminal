import { debugDump } from "./helper.js";

// This function is called when the mod is attached to the game client.
export function onAttach() {
    console.log("Attached.");
}

// Called when client has loaded all bootstppinng mods and before activate or check other mods.
export function onBootstrap() {
    console.log("Boostrap...");
    debugDump();
}