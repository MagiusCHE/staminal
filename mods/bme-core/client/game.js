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
            // onMousePressed: async (win, button, x, y) => {
            //     console.log(`Main window mouse pressed: button=${button}, x=${x}, y=${y}`);
            // }
        });

        // mainWin.onMousePressed = async (win, button, x, y) => { 
        //     console.log(`Main window mouse pressed: button=${button}, x=${x}, y=${y}`);
        // }

        Graphic.setMainWindow(mainWin);

        for (const win of windows) {
            await win.close();
        }

        // Create container using ECS API
        const cont = await World.spawn({
            Node: {
                width: "100%",
                height: "100%",
                flex_direction: "column",
                justify_content: "center",
                align_items: "center",
            },
            BackgroundColor: "#1a1a2e",
        });

        // console.log("Waiting 5 seconds to ensure resources are loaded...");
        // await wait(5000);
        // console.log("After wait, checking resource...");

        if (!Resource.isLoaded("title-screen-background")) {
            throw new Error("Title screen background resource not loaded: title-screen-background");
        }

        // Create background image using ECS API
        const bkg = await World.spawn({
            Node: {
                width: "100%",
                height: "100%",
            },
            ImageNode: {
                resource_id: "title-screen-background",
                image_mode: NodeImageMode.Contain,
                // cover_position: {
                //     x: "50%",
                //     y: "50%",
                // },
            },
            BackgroundColor: "#FF0000",
        }, cont);

        const text = await World.spawn({
            Node: {
                width: "auto",
                height: "auto",
            },
            Text: {
                value: "Welcome to Staminal!",
                font_size: 48,
                color: "#ffffff",
                shadow: {
                    color: "#000000",
                    offset: { x: 3, y: 3 }
                }
            },
        }, bkg);

        // Set parent relationship


    }
    async startGame() {
        console.warn("TODO: Starting the game...");
    }
}
