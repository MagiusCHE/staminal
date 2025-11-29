import { Manager } from "./manager";

const manager = new Manager();

// This function is called when the mod is attached to the game client.
export function onAttach() {
    manager.register();   
}

// Called when client has loaded all bootstppinng mods and before activate or check other mods.
export async function onBootstrap() {
    const manager = new Manager();
    manager.run(); // it works in async mode but singlethreaded
}