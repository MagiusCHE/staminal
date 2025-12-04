export function onAttach() {
    console.log("Attached.");
    system.registerEvent("AppStart", onAppStart, 100);
}

const onAppStart = async (req, res) => {
    //console.log("App started. req:", req, "res", res);
    // Get all opened windows and close them, keeping only one with no widgets
    const windows = await graphic.getWindows();
    let first = true;
    for (const win of windows) {
        if (first) {
            await win.removeAllWidgets();
            first = false;
        } else {
            await win.close();
        }
    }
}
