function debugDump() {
    console.log("Data path:", Process.app.data_path);
    console.log("Config path:", Process.app.config_path);
}
export { debugDump }