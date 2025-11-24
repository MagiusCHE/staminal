function debugDump() {
    console.log("Data path:", process.app.data_path);
    console.log("Config path:", process.app.config_path);
}
export { debugDump }