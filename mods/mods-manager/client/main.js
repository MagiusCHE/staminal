// This function is called when the mod is attached to the game client.
function onAttach() {
    console.log("Attached.");
}

// Called when client has loaded all bootstppinng mods and before activate or check other mods.
function onBootstrap() {
    console.log("Boostrap...");  
    console.log("Data path:", process.app.data_path);
    console.log("Config path:", process.app.config_path);
}