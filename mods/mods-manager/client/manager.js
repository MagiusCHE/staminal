import { wait } from "@js-helper";

export class Manager {
    #gameInfo;
    #window;

    // UI elements for loading screen
    #loadingContainer;
    #statusLabel;
    #progressBarContainer;
    #progressBarFill;
    #progressText;
    #cancelButton;

    // Loading state
    #cancelled = false;

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
                width: 500,
                height: 150,
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

        const assetTestPath = system.getAssetsPath("fonts/PerfectDOSVGA437.ttf");

        await graphic.loadFont("default", assetTestPath);

        await this.#window.setTitle("Staminal: " + this.#gameInfo.name);

        this.#window.setFont("default", 16);

        // Main container with dark background
        this.#loadingContainer = await this.#window.createWidget(WidgetTypes.Container, {
            width: "100%",
            height: "100%",
            direction: FlexDirection.Column,
            justifyContent: JustifyContent.Center,
            alignItems: AlignItems.Center,
            backgroundColor: "#1a1a2e",
            padding: { top: 20, right: 20, bottom: 20, left: 20 },
            gap: 15
        });

        // Status label: "Loading mods:"
        this.#statusLabel = await this.#loadingContainer.createChild(WidgetTypes.Text, {
            content: locale.get("loading-mods"),
            font: { size: 18 },
            fontColor: "#ffffff"
        });

        // Progress bar wrapper - contains two stacked layers via Column direction
        const progressBarWrapper = await this.#loadingContainer.createChild(WidgetTypes.Container, {
            width: "90%",
            height: 30
        });

        // Progress bar container (background track)
        this.#progressBarContainer = await progressBarWrapper.createChild(WidgetTypes.Container, {
            width: "100%",
            height: "100%",
            backgroundColor: "#333344",
            borderRadius: 4,
            direction: FlexDirection.Row,
            justifyContent: JustifyContent.FlexStart,
            alignItems: AlignItems.Stretch
        });

        // Progress bar fill (the colored part that grows)
        this.#progressBarFill = await this.#progressBarContainer.createChild(WidgetTypes.Container, {
            width: "50%",  // Will be updated dynamically
            height: "100%",
            backgroundColor: "#4a9eff",
            borderRadius: 4
        });

        // Progress text container - overlays using negative margin on wrapper
        const progressTextContainer = await progressBarWrapper.createChild(WidgetTypes.Container, {
            width: "100%",
            height: 30,
            margin: { top: -30 },  // Go back up to overlay on the progress bar
            direction: FlexDirection.Row,
            justifyContent: JustifyContent.Center,
            alignItems: AlignItems.Center
        });

        //await wait(3000)

        // Progress text (mod name + operation) - centered in the overlay container
        this.#progressText = await progressTextContainer.createChild(WidgetTypes.Text, {
            content: "",
            font: { size: 14 },
            fontColor: "#ffffff"
        });

       // await wait(3000)

        //Cancel button
        this.#cancelButton = await this.#loadingContainer.createChild(WidgetTypes.Button, {
            label: locale.get("cancel"),
            font: { size: 14 },
            backgroundColor: "#cc3333",
            hoverColor: "#ff4444",
            pressedColor: "#991111",
            padding: { top: 8, right: 16, bottom: 8, left: 16 }
        });

        // Subscribe to button click event
        await this.#cancelButton.on("click", this.onCancelClicked.bind(this));
    }

    async onCancelClicked() {
        console.log("Cancel button clicked");
        this.#cancelled = true;
        await this.#statusLabel.setProperty("content", locale.get("cancelling"));
        await this.#cancelButton.setProperty("disabled", true);
        await this.#cancelButton.setProperty("label", locale.get("cancelling"));

        // Give a moment for UI to update, then exit
        await wait(500);
        system.exit(0);
    }

    /**
     * Update the progress bar UI
     * @param {number} current - Current mod index (0-based)
     * @param {number} total - Total number of mods to process
     * @param {string} modId - ID of the mod being processed
     * @param {string} operation - Current operation ("downloading" or "loading")
     */
    async updateProgress(current, total, modId, operation) {
        if (this.#cancelled) return;

        const percent = total > 0 ? Math.round(((current ) / total) * 100) : 0;
        const percentStr = `${percent}%`;

        // Update the fill bar width
        await this.#progressBarFill.setProperty("width", percentStr);

        console.log(`AAAAAAAAAAAAAAAAAAAAAAAAAAa Progress: ${percentStr} - Mod: ${modId} (${operation})`);

        // Update the progress text
        const operationText = locale.get(operation); // "downloading" or "loading"
        const progressLabel = `${modId} (${operationText})`;
        await this.#progressText.setProperty("content", progressLabel);

        // Update status label with counter
        const statusText = locale.getWithArgs("loading-mods-progress", {
            current: current + 1,
            total: total
        });
        await this.#statusLabel.setProperty("content", statusText);
    }

    async ensureMods() {
        console.log("Ensuring mods...");
        const mods = system.getMods();
        // Filter mods that are not loaded yet
        const toload = mods.filter(mod => !mod.loaded);

        if (toload.length === 0) {
            console.log("All mods already loaded.");
            await this.onModsReady();
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

        const totalMods = toload.length;
        let currentIndex = 0;
        let error_occurred = undefined;

        for (const mod of toload) {
            if (this.#cancelled) {
                console.log("Loading cancelled by user");
                return;
            }

            try {
                if (!mod.exists) {
                    // Downloading
                    await this.updateProgress(currentIndex, totalMods, mod.id, "downloading");
                    console.log(`Downloading mod: ${mod.id}...`);
                    await this.downloadInstallMod(mod);
                } else {
                    // Loading (attaching)
                    await this.updateProgress(currentIndex, totalMods, mod.id, "loading");
                    console.log(`Attaching mod: ${mod.id}...`);
                    await system.attachMod(mod.id);
                }
                currentIndex++;
            } catch (e) {
                console.error(`Failed to process mod "${mod.id}":`, e);
                if (!mod.exists) {
                    error_occurred = locale.getWithArgs("mod-download-failed", { mod_id: mod.id });
                } else {
                    error_occurred = locale.getWithArgs("mod-attach-failed", { mod_id: mod.id });
                }
                break;
            }
        }

        if (this.#cancelled) {
            return;
        }

        if (error_occurred) {
            await this.exitWithError(error_occurred);
            return;
        }

        await this.#progressBarFill.setProperty("width", "100%");

        // All mods loaded successfully
        await this.onModsReady();
    }

    async onModsReady() {
        console.log("All mods loaded successfully!");

        // Update UI to show completion
        await this.#statusLabel.setProperty("content", locale.get("loading-complete"));
        await this.#progressBarFill.setProperty("width", "100%");
        await this.#progressText.setProperty("content", locale.get("starting-game"));

        // Hide cancel button or change it to "Start" button
        await this.#cancelButton.setProperty("label", locale.get("starting"));
        await this.#cancelButton.setProperty("disabled", true);
        await this.#cancelButton.setProperty("backgroundColor", "#33cc33");

        // Brief pause to show completion
        await wait(500);

        // Now start the game!
        system.sendEvent("AppStart");
    }

    async exitWithError(message) {
        console.error(`Exiting due to error: ${message}`);

        // Update UI to show error
        await this.#statusLabel.setProperty("content", locale.get("error-occurred"));
        await this.#statusLabel.setProperty("fontColor", "#ff4444");

        await this.#progressBarFill.setProperty("backgroundColor", "#cc3333");
        await this.#progressText.setProperty("content", message);

        // Change cancel button to "Exit" button
        await this.#cancelButton.setProperty("label", locale.get("exit"));
        await this.#cancelButton.setProperty("disabled", false);

        // Wait for user to click exit, or auto-exit after delay
        await wait(5000);
        system.exit(1);
    }

    async downloadInstallMod(mod_info) {
        // Build the stam:// URI for this mod
        const uri = mod_info.download_url;
        console.log(mod_info)
        console.log(`Downloading: ${uri}`);

        const response = await network.download(uri, (percentage, receivedBytes, totalBytes) => {
            // Update progress UI during download
            // Note: This callback may be called very frequently, so keep it efficient
            // We don't have currentIndex/totalMods here, so we skip updating those
            const percentStr = totalBytes > 0 ? `${Math.round(percentage)}%` : "";
            this.#progressText.setProperty("content", `${mod_info.id} (downloading) ${percentStr}`);
            console.log(`Download progress for ${mod_info.id}: ${percentStr} (${receivedBytes}/${totalBytes} bytes)`);
        });

        if (response.status !== 200) {
            throw new Error(`Status ${response.status}`);
        }

        if (!response.temp_file_path) {
            throw new Error("No data received");
        }

        console.log(`Downloaded ${mod_info.id} into %o`, response.temp_file_path);
        await system.installModFromPath(response.temp_file_path, mod_info.id);
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
