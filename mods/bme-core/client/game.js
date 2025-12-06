import { wait } from "@js-helper";
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
                    //resolution: await Graphic.getScreenResolution(screen),
                    resolution: { width: 1280, height: 720 },
                    mode: WindowModes.Windowed,
                    resizable: true,
                }
            }
        }
        console.log("Configuration loaded:", this.#config);
    }
    async loadPreliminaryResources() {
        console.warn("TODO: Loading preliminary resources...");
        console.warn("TODO: Loading preliminary assets from bme-assets-* mod...");
        Resource.load("@bme-assets-01/assets/background/title.jpg", "title-screen-background");

        await this.waitForResources();
    }
    async waitForResources() {
        console.log("Waiting for resources to load...", Resource.getLoadingProgress());

        // We can use await Resource.whenLoadedAll(); but i want to show progress bar
        while (!Resource.isLoadingCompleted()) {
            await wait(100);
        }

        console.log("All resources loaded: ", Resource.getLoadingProgress());
    }
    async load() {
        console.warn("TODO: Loading remaining required resources to start the game...");
        console.warn("TODO: Loading remaining required assets from bme-assets-* mod...");
    }
    async initializeWindow() {
        const windows = await Graphic.getWindows();
        //let first = true;
        //console.log("Found", windows.length, "windows...");
        //console.log("Main window:", engine.mainWindow.id);
        

        // All windows are destroyed, create our main window
        //console.log(" - Win:", win.id);

        const mainWin = await Graphic.createWindow({
            title: (await Graphic.getEngineInfo()).mainWindow.getTitle() || "Staminal",
            width: this.#config.graphic.resolution.width,
            height: this.#config.graphic.resolution.height,
            resizable: this.#config.graphic.resizable,
            mode: this.#config.graphic.mode,
            positionMode: WindowPositionModes.Centered,
        });        
        
        Graphic.setMainWindow(mainWin);

        for (const win of windows) {
            await win.close();
        }

        const cont = await mainWin.createWidget(WidgetTypes.Container, {
            width: "100%",
            height: "100%",
            direction: FlexDirection.Column,
            justifyContent: JustifyContent.Center,
            alignItems: AlignItems.Center,
            backgroundColor: "#1a1a2e",
        });

        // console.log("Waiting 5 seconds to ensure resources are loaded...");
        // await wait(5000);
        // console.log("After wait, checking resource...");

        if (!Resource.isLoaded("title-screen-background")) {
            throw new Error("Title screen background resource not loaded: title-screen-background");
        }

        const bkg = await cont.createChild(WidgetTypes.Image, {
            resourceId: "title-screen-background",
            width: "100%",
            height: "100%",
            scaleMode: ImageScaleModes.Contain,
            //backgroundColor: "#00000000", // no need
        });

    }
    async startGame() {
        console.warn("TODO: Starting the game...");
    }
}