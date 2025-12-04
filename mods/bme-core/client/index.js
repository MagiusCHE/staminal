export function onAttach() {
    console.log("Attached.");
    system.registerEvent("AppStart", onAppStart, 100);
}

const onAppStart = async (req, res) => {
    //console.log("App started. req:", req, "res", res);
    // Get all opened windows and close them, keeping only one with no widgets
    const windows = await graphic.getWindows();
    const engine = await graphic.getEngineInfo();
    //let first = true;
    console.log("Found", windows.length, "windows...");
    console.log("Main window:", engine.mainWindow.id);
    for (const win of windows) {
        console.log(" - Win:", win.id);
        if (win.id == engine.mainWindow.id) {
            await win.clearWidgets();
        } else {
            await win.close();
        }
    }
    res.handled = true;
}
