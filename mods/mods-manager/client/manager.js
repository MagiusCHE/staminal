import { wait } from "@js-helper";
export class Manager {
    #gameInfo;
    #window;
    constructor() {
        console.log("Initialized.");
        this.#gameInfo = system.getGameInfo();
    }

    register() {
        system.registerEvent(SystemEvents.TerminalKeyPressed, this.onTerminalKeyPressed.bind(this), 100);
        system.registerEvent(SystemEvents.GraphicEngineReady, this.onGraphicEngineReady.bind(this), 100);
        system.registerEvent(SystemEvents.GraphicEngineWindowClosed, this.onGraphicEngineWindowClosed.bind(this), 100);
    }

    async onTerminalKeyPressed(req, res) {
        //console.log("Console key pressed:", req);
        if (req.key == "c" && req.ctrl) {
            res.handled = true;
            system.exit(0);
            return;
        }
    }

    async run() {
        graphic.enableEngine(GraphicEngines.Bevy, {
            window: {
                title: "Staminal",
                width: 400,
                height: 100,
                resizable: false,
                positionMode: WindowPositionModes.Centered
            }
        }) // can be awaited but we dont care about return here. Let it going asynchronously.

    }

    async onGraphicEngineWindowClosed(req, res) {
        console.log("Graphic engine window closed", req);
        if (req.windowId === this.#window.id) {
            console.log("Main window closed, exiting...");
            system.exit(0);
            res.handled = true;
        }
    }

    async onGraphicEngineReady() {

        const engine = await graphic.getEngineInfo();

        console.log("Graphic engine ready", engine);

        this.#window = engine.mainWindow;

        await this.prepareUi();
        await this.ensureMods();
    }

    async prepareUi() {
        console.log("Preparing UI for game %o", this.#gameInfo.id);

        await this.#window.setTitle("Staminal: " + this.#gameInfo.name);

        // await graphic.createWindow({
        //     title: "Staminal (subwin): " + this.#gameInfo.name,
        //     width: 1280,
        //     height: 800,
        //     resizable: true,
        //     positionMode: WindowPositionModes.Centered
        // })

        // Create a container with flexbox layout
        const container = await this.#window.createWidget(WidgetTypes.Container, {
            width: "100%",
            height: "100%",
            direction: FlexDirection.Column,
            justifyContent: JustifyContent.Center,
            alignItems: AlignItems.Center,
            backgroundColor: "#1a1a2e"
        });

        // Create a text widget
        const text = await container.createChild(WidgetTypes.Text, {
            content: "Hello, World!",
            fontSize: 32,
            fontColor: "#ffffff"
        });

        //Create a button
        const button = await container.createChild(WidgetTypes.Button, {
            label: "Click Me",
            backgroundColor: "#0077cc",
            hoverColor: "#0099ff",
            pressedColor: "#005599"
        });

    }

    async ensureMods() {
        console.log("Ensuring mods...");
        const mods = system.getMods();
        // Filter mods that are not loaded yet
        const toload = mods.filter(mod => !mod.loaded);

        if (toload.length === 0) {
            console.log("All mods already loaded.");
            return;
        }

        // Separate mods that need download vs just attach
        const toDownload = toload.filter(mod => !mod.exists);
        const toAttach = toload.filter(mod => mod.exists);

        if (toDownload.length > 0) {
            console.log(`${toDownload.length} mod(s) need to be downloaded:`, toDownload.map(m => m.id).join(", "));
        }
        if (toAttach.length > 0) {
            console.log(`${toAttach.length} mod(s) need to be attached:`, toAttach.map(m => m.id).join(", "));
        }

        let error_occurred = undefined;
        for (const mod of toload) {
            if (!mod.exists) {
                console.log(`Downloading mod: ${mod.id}...`);
                this.downloadInstallMod(mod).then(() => toload.splice(toload.indexOf(mod), 1)).catch((e) => {
                    console.error(`Failed to download mod "${mod.id}":`, e);
                    error_occurred = locale.getWithArgs("mod-download-failed", { mod_id: mod.id });
                });
            } else {
                console.log(`Attaching mod: ${mod.id}...`);
                system.attachMod(mod.id).then(() => toload.splice(toload.indexOf(mod), 1)).catch((e) => {
                    console.error(`Failed to attach mod "${mod.id}":`, e);
                    error_occurred = locale.getWithArgs("mod-attach-failed", { mod_id: mod.id });
                });
            }
            if (error_occurred) {
                break;
            }
        }
        while (!error_occurred && toload.length > 0) {
            await wait(100);
        }

        if (error_occurred) {
            // TODO: Show error in UI
            await this.exitWithError(error_occurred);
        }

        // Now start the game!
        system.sendEvent("AppStart");
    }

    async exitWithError(message) {
        console.error(`Exiting due to error: ${message}`);
        console.warn("TODO: Implement User Interface to show error message before exiting...");
        system.exit(1);
    }

    async downloadInstallMod(mod_info) {
        // Build the stam:// URI for this mod
        const uri = mod_info.download_url;
        console.log(mod_info)
        console.log(`Downloading: ${uri}`);

        const response = await network.download(uri);

        if (response.status !== 200) {
            throw new Error(`Status ${response.status}`);
        }

        if (!response.temp_file_path) {
            throw new Error("No data received");
        }

        console.log(`Downloaded ${mod_info.id} into %o`, response.temp_file_path);
        await system.installModFromPath(response.temp_file_path, mod_info.id); //test
        await system.attachMod(mod_info.id);
        return;
    }


    print_mods() {
        const mods = system.getMods();
        console.log(`Found ${mods.length} mods:`);
        for (const mod of mods) {
            console.log(` - ${mod.id} [${mod.mod_type}] loaded=${mod.loaded}`);
        }
    }
}