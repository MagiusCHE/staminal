export class Game {
    #config
    constructor() {

    }
    async run() {
        await this.preload();
        await this.initializeWindow();
        await this.load();
        await this.startGame();
    }
    async preload() {
        await this.loadConfiguration();
        await this.loadPreliminaryResources();
    }
    async loadConfiguration() {
        const path = System.getGameConfigPath("main.json");
        this.#config = await File.readJson(path, "utf-8", null);
        if (!this.#config) {
            console.log("No configuration found, using defaults");
            const screen = await Graphic.getPrimaryScreen();
            this.#config = {
                graphic: {
                    screen: screen,
                    resolution: await Graphic.getScreenResolution(screen),
                    // WindowMode.BorderlessFullscreen | WindowMode.Fullscreen | WindowMode.Windowed
                    mode: "borderless_fullscreen"  // TODO: use WindowMode enum when available
                }
            }
        }
        console.log("Configuration loaded:", this.#config);
    }
    async loadPreliminaryResources() {
        console.warn("TODO: Loading preliminary resources...");
        console.warn("TODO: Loading preliminary assets from bme-assets-* mod...");
        await Resource.preload("@bme-assets-01/assets/background/title.jpg", "title-screen-background");
    }
    async load() {
        console.warn("TODO: Loading remaining required resources to start the game...");
        console.warn("TODO: Loading remaining required assets from bme-assets-* mod...");
    }
    async initializeWindow() {
        const windows = await Graphic.getWindows();
        const engine = await Graphic.getEngineInfo();
        //let first = true;
        //console.log("Found", windows.length, "windows...");
        //console.log("Main window:", engine.mainWindow.id);
        for (const win of windows) {
            //console.log(" - Win:", win.id);
            if (win.id == engine.mainWindow.id) {
                await win.clearWidgets();
                if (this.#config.graphic.mode == "borderless_fullscreen") {
                    await win.setMode(WindowModes.BorderlessFullscreen);
                } else if (this.#config.graphic.mode == "fullscreen") {
                    await win.setMode(WindowModes.Fullscreen);
                } else {
                    await win.setMode(WindowModes.Windowed);
                    await win.setSize(this.#config.graphic.resolution.width, this.#config.graphic.resolution.height);
                    // FIXME: Should center the window
                    console.warn("FIXME: Center the window on screen");
                }

                const cont = await win.createWidget(WidgetTypes.Container, {
                    width: "100%",
                    height: "100%",
                    direction: FlexDirection.Column,
                    justifyContent: JustifyContent.Center,
                    alignItems: AlignItems.Center,
                    backgroundColor: "#1a1a2e",
                });

                const bkg = await cont.createChild(WidgetTypes.Image, {
                    resourceId: "title-screen-background",
                    width: "100%",
                    height: "100%",
                    stretchMode: "cover"
                });
            } else {
                await win.close();
            }
        }
    }
    async startGame() {
        console.warn("TODO: Starting the game...");
    }
}