import { wait, humanizeBytes } from "@js-helper";

const UPDATEUI_INTERVAL_MS = 500;

const INITIAL_FONT_SIZE = 20;

export class Manager {
    #gameInfo;
    #window;

    // UI elements for loading screen
    #loadingContainer;
    #statusLabel;
    #progressBarContainer;
    #progressBarFill;
    #progressText;
    #secondProgressBarFill;  // Secondary bar for download progress
    #actionButton;

    constructor() {
        this.#gameInfo = System.getGameInfo();
    }

    register() {
        System.registerEvent(SystemEvents.TerminalKeyPressed, this.onTerminalKeyPressed.bind(this), 100);
        System.registerEvent(SystemEvents.GraphicEngineReady, this.onGraphicEngineReady.bind(this), 100);
        System.registerEvent(SystemEvents.GraphicEngineWindowClosed, this.onGraphicEngineWindowClosed.bind(this), 100);
        System.registerEvent("EnsureAssets", this.onEnsureAssets.bind(this), 100);
    }

    async onTerminalKeyPressed(req, res) {
        //console.log("Console key pressed:", req);
        if (req.key == "c" && req.ctrl) {
            res.handled = true;
            System.exit(0);
            return;
        }
    }

    async run() {
        Graphic.enableEngine(GraphicEngines.Bevy, {
            window: {
                title: "Staminal",
                width: 500,
                height: 200,
                resizable: false,
                positionMode: WindowPositionModes.Centered
            }
        }) // can be awaited but we dont care about return here. Let it going asynchronously.

    }

    async onGraphicEngineWindowClosed(req, res) {
        //console.log("Graphic engine window closed", req);
        if (req.windowId === this.#window.id) {
            //console.log("Main window closed, exiting...");
            System.exit(0);
            res.handled = true;
        }
    }

    async onGraphicEngineReady() {

        const engine = await Graphic.getEngineInfo();

        // console.log("Graphic engine ready", engine);

        this.#window = engine.mainWindow;

        await this.prepareUi();
        this.ensureMods();
    }

    async prepareUi() {
        // console.log("Preparing UI for game %o", this.#gameInfo.id);

        //Exo2-Regular
        //const assetTestPath = System.getAssetsPath();

        // await Graphic.loadFont("terminus", System.getAssetsPath("fonts/terminus-ttf-4.49.3/TerminusTTF-Bold-4.49.3.ttf"));
        // await Graphic.loadFont("exo2", System.getAssetsPath("fonts/Exo_2/Exo2-VariableFont_wght.ttf"));
        // await Graphic.loadFont("jacquard24", System.getAssetsPath("fonts/Jacquard_24/Jacquard24-Regular.ttf"));
        await Graphic.loadFont("macondo", System.getAssetsPath("fonts/Macondo/Macondo-Regular.ttf"));

        await this.#window.setTitle("Staminal: " + this.#gameInfo.name);

        this.#window.setFont("macondo", INITIAL_FONT_SIZE);

        //await Graphic.createWindow({ title: "test" });

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
            content: Locale.get("mods-ensuring-title"),
            font: { size: INITIAL_FONT_SIZE * 1.3 },
            fontColor: "#ffffff"
        });

        // Progress bars wrapper - contains main bar and download bar
        const progressBarWrapper = await this.#loadingContainer.createChild(WidgetTypes.Container, {
            width: "90%",
            height: 47,  // 30 for main bar + 2 gap + 15 for download bar
            direction: FlexDirection.Column,
            gap: 2
        });

        // Main progress bar container (background track)
        this.#progressBarContainer = await progressBarWrapper.createChild(WidgetTypes.Container, {
            width: "100%",
            height: 30,
            backgroundColor: "#333344",
            borderRadius: 4,
            direction: FlexDirection.Row,
            justifyContent: JustifyContent.FlexStart,
            alignItems: AlignItems.Stretch
        });

        // Main progress bar fill (the colored part that grows)
        this.#progressBarFill = await this.#progressBarContainer.createChild(WidgetTypes.Container, {
            width: "0%",  // Will be updated dynamically
            height: "100%",
            backgroundColor: "#4a9eff",
            borderRadius: 4
        });

        // Progress text container - overlays using negative margin
        const progressTextContainer = await progressBarWrapper.createChild(WidgetTypes.Container, {
            width: "100%",
            height: 30,
            margin: { top: -32 },  // Go back up to overlay on the progress bar
            direction: FlexDirection.Row,
            justifyContent: JustifyContent.Center,
            alignItems: AlignItems.Center
        });

        // Progress text (mod name + operation) - centered in the overlay container
        this.#progressText = await progressTextContainer.createChild(WidgetTypes.Text, {
            content: "",
            font: { size: INITIAL_FONT_SIZE },
            fontColor: "#ffffff"
        });

        // Secondary download progress bar container (background track) - half height
        const secondBarContainer = await progressBarWrapper.createChild(WidgetTypes.Container, {
            width: "100%",
            height: 15,
            backgroundColor: "#333344",
            borderRadius: 2,
            direction: FlexDirection.Row,
            justifyContent: JustifyContent.FlexStart,
            alignItems: AlignItems.Stretch
        });

        // Secondary download progress bar fill
        this.#secondProgressBarFill = await secondBarContainer.createChild(WidgetTypes.Container, {
            width: "0%",  // Will be updated during download
            height: "100%",
            backgroundColor: "#7ac74f",  // Green color to differentiate
            borderRadius: 2
        });

        // await wait(3000)

        //Cancel button
        this.#actionButton = await this.#loadingContainer.createChild(WidgetTypes.Button, {
            // To create multiple text style in a single label, create childs with Text widgets and set default one to empty.
            label: Locale.get("cancel"),
            font: { size: INITIAL_FONT_SIZE },
            backgroundColor: "#cc3333",
            hoverColor: "#ff4444",
            pressedColor: "#991111",
            padding: { top: 8, right: 16, bottom: 8, left: 16 },
            borderRadius: 10
        });

        // Subscribe to button click event
        await this.#actionButton.on("click", this.onActionClicked.bind(this));
    }

    async onActionClicked() {
        await this.#actionButton.setProperty("disabled", true);

        if (this.#UIState.last_error_occurred) {
            // Exit on error
            await wait(500);
            System.exit(1);
            return;
        }
        // Else, abort is pressed

        this.#UIState.cancelled = true;
        this.updateUI();

        // Give a moment for UI to update, then exit
        await wait(500);
        System.exit(0);
    }

    #UIState = undefined

    resetUiState() {
        this.#UIState = {
            to_download: 0,
            to_attach: 0,
            mods: {
                // [key: mod_id]: {
                //   downloaded: false,
                //   downloading: false,
                //   attached: false,
                //   attaching: false,
                //   installed: false,
                //   installing: false,
                //   received_bytes: 0,
                //   expected_total_bytes: 0                
                // }
            },
            expected_total_bytes: 0,
            last_error_occurred: undefined,
            cancelled: false,
            completed: false,
            // For bps calculation
            previous_received: 0,
            previous_bps_str: "0 B/s",
            previous_timestamp: Date.now()
        }
    }

    async ensureMods() {
        //console.log("Ensuring mods...");
        const mods = System.getMods();
        console.trace(`mods:`, mods);
        // Filter mods that are not loaded yet
        const toAnalize = mods.filter(mod => !mod.loaded);

        this.resetUiState();

        // Separate mods that need download vs just attach
        const toDownload = toAnalize.filter(mod => !mod.exists);
        for (const mod of toDownload) {
            this.#UIState.expected_total_bytes += mod.archive_bytes || 0;
        }
        const toAttach = toAnalize.filter(mod => mod.exists);

        this.#UIState.to_download = toDownload.length;
        this.#UIState.to_attach = toAttach.length;

        for (const mod of toAnalize) {
            this.#UIState.mods[mod.id] = {
                downloaded: false,
                attached: false,
                installed: mod.exists,
                received_bytes: 0,
                expected_total_bytes: mod.archive_bytes || 0
            };
        }

        // We need 3 steps.
        // 1. Download missing mods (one by one)
        // 2. Install downloaded mods (one by one but soon after download)
        // 3. Attach existing mods (one by one)

        // Draw initial UI
        this.updateUI();

        // First, download all missing mods

        for (const mod of toDownload) {
            if (this.#UIState.cancelled) {
                console.log("Loading cancelled by user");
                return;
            }

            // Downloading                
            // After downloading mod will be installed asynchronously
            // to ensure its installation before attaching check this.#installedMods[mod.id]
            try {
                await this.downloadInstallMod(mod);
            } catch (e) {
                return;
            }
        }

        //await wait(5000); // DEBUG

        console.log("All downloads initiated, waiting for installations to complete...");
        // Wait all installations to complete
        while (!this.#UIState.cancelled && !this.#UIState.last_error_occurred) {
            const pendingInstalls = Object.entries(this.#UIState.mods).filter(([modId, modState]) => !modState.installed && !modState.exists);
            if (pendingInstalls.length === 0) {
                break;
            }
            await wait(250);
        }
        console.log("All installations completed.");

        if (this.#UIState.cancelled) {
            return;
        }

        if (this.#UIState.last_error_occurred) {
            return;
        }

        // Now attach all mods (downloaded + existing)
        for (const modId in toDownload) {
            const mod = toDownload[modId];
            toAttach.push(mod);
        }
        console.log("Attaching %o mods...", toAttach.length);
        for (const mod of toAttach) {
            // Loading (attaching)            
            try {
                await this.attachMod(mod);
            } catch (e) {
                console.error(`Failed to attach mod "${mod.id}":`, e);
                this.#UIState.last_error_occurred = Locale.getWithArgs("mod-attach-failed", { mod_id: mod.id });
                return;
            }

            if (this.#UIState.cancelled) {
                return;
            }
        }

        // All mods loaded successfully
        console.log("All mods loaded successfully!");

        this.#UIState.completed = true;

        // Brief pause to show completion
        await wait(1500);

        // Now start the game!
        const ret = await System.sendEvent("AppStart");
        //console.log("AppStart event result:", ret);
        if (!ret.handled) {
            console.error("AppStart event was not handled by any mod!");
            System.exit(0);
        }
    }

    #uiIntervalUpdate = undefined;

    async updateUI() {
        if (this.#uiIntervalUpdate) {
            clearTimeout(this.#uiIntervalUpdate);
        }
        if (this.#UIState.last_error_occurred) {
            // Red state
            await this.#statusLabel.setProperty("content", Locale.get("error-occurred"));
            await this.#statusLabel.setProperty("fontColor", "#ff4444");

            await this.#progressBarFill.setProperty("backgroundColor", "#cc3333");
            await this.#secondProgressBarFill.setProperty("backgroundColor", "#cc3333");

            // Change cancel button to "Exit" button
            await this.#actionButton.setProperty("label", Locale.get("exit"));
            await this.#actionButton.setProperty("disabled", false);
            return;
        } else if (this.#UIState.cancelled) {
            await this.#actionButton.setProperty("label", Locale.get("cancelling"));
            await this.#actionButton.setProperty("disabled", true);
            return;
        }
        const isDownloading = Object.values(this.#UIState.mods).some(modState => modState.downloading);
        const isAttaching = Object.values(this.#UIState.mods).some(modState => modState.attaching);
        const isInstalling = Object.values(this.#UIState.mods).some(modState => modState.installing);
        if (isDownloading) {
            const todoDone = {
                todo: this.#UIState.to_download,
                done: Object.entries(this.#UIState.mods).filter(([modId, modState]) => modState.downloaded).length + 1
            }
            await this.#statusLabel.setProperty("content", Locale.getWithArgs("mods-downloading-title", todoDone));
            const mainProgress = (todoDone.done > 0 ? (todoDone.todo > 0 ? Math.floor((todoDone.done / todoDone.todo) * 100) : 100) : 0) + "%";
            await this.#progressBarFill.setProperty("width", mainProgress);

            const received = Object.values(this.#UIState.mods).reduce((acc, modState) => acc + (modState.received_bytes || 0), 0)
            const percent = (received > 0 ? (this.#UIState.expected_total_bytes > 0 ? Math.floor((received / this.#UIState.expected_total_bytes) * 100) : 1) : 0) + "%";
            await this.#secondProgressBarFill.setProperty("width", percent);

            // Calculate bps storing previous received and timestamp
            const now = Date.now();
            const elapsed_ms = now - this.#UIState.previous_timestamp;
            const bytes_diff = received - this.#UIState.previous_received;

            // Calculate bytes per second
            // Only update if at least 100ms has passed to avoid division by near-zero
            let bps_str = this.#UIState.previous_bps_str || "0 B/s";
            if (elapsed_ms >= 1000) {
                const bps = (bytes_diff / elapsed_ms) * 1000; // Convert ms to seconds
                bps_str = humanizeBytes(Math.floor(bps)) + "/s";

                // Update previous values for next calculation
                this.#UIState.previous_bps_str = bps_str;
                this.#UIState.previous_received = received;
                this.#UIState.previous_timestamp = now;
            }

            await this.#progressText.setProperty("content", Locale.getWithArgs("mods-downloading-progress", {
                received: humanizeBytes(received),
                total: humanizeBytes(this.#UIState.expected_total_bytes),
                bps: bps_str,
            }));
        } else if (isInstalling) {
            const todoDone = {
                todo: this.#UIState.to_download,
                done: Object.entries(this.#UIState.mods).filter(([modId, modState]) => modState.installed && modState.downloaded).length + 1
            }
            await this.#statusLabel.setProperty("content", Locale.getWithArgs("mods-installing-title", todoDone));
            const mainProgress = (todoDone.done > 0 ? (todoDone.todo > 0 ? Math.floor((todoDone.done / todoDone.todo) * 100) : 100) : 0) + "%";
            await this.#progressBarFill.setProperty("width", mainProgress);

            // const received = Object.values(this.#UIState.mods).reduce((acc, modState) => acc + (modState.received_bytes || 0), 0)
            // const percent = (received > 0 ? (this.#UIState.expected_total_bytes > 0 ? Math.floor((received / this.#UIState.expected_total_bytes) * 100) : 1) : 0) + "%";
            await this.#secondProgressBarFill.setProperty("width", "50%"); // Static 50% during install

            await this.#progressText.setProperty("content", Locale.get("mods-installing-progress"));
        } else if (isAttaching) {
            const todoDone = {
                todo: this.#UIState.to_attach,
                done: Object.entries(this.#UIState.mods).filter(([modId, modState]) => modState.attached).length + 1
            }
            await this.#statusLabel.setProperty("content", Locale.getWithArgs("mods-attaching-title", todoDone));
            const mainProgress = (todoDone.done > 0 ? (todoDone.todo > 0 ? Math.floor((todoDone.done / todoDone.todo) * 100) : 100) : 0) + "%";
            await this.#progressBarFill.setProperty("width", mainProgress);

            // const received = Object.values(this.#UIState.mods).reduce((acc, modState) => acc + (modState.received_bytes || 0), 0)
            // const percent = (received > 0 ? (this.#UIState.expected_total_bytes > 0 ? Math.floor((received / this.#UIState.expected_total_bytes) * 100) : 1) : 0) + "%";
            await this.#secondProgressBarFill.setProperty("width", "50%"); // Static 50% during install

            await this.#progressText.setProperty("content", Locale.get("mods-attaching-progress"));
        } else {
            if (this.#UIState.completed) {
                // Completed, no further updates needed
                await this.#progressText.setProperty("content", Locale.getWithArgs("loading-complete", { mods: Object.entries(this.#UIState.mods).length }));
                await this.#progressBarFill.setProperty("width", "100%");
                await this.#secondProgressBarFill.setProperty("width", "100%");
                await this.#statusLabel.setProperty("content", Locale.getWithArgs("starting-game", { game_name: this.#gameInfo.name }));

                // Hide cancel button or change it to "Start" button
                await this.#actionButton.setProperty("label", Locale.get("starting"));
                console.warn("Game is starting...", Locale.get("starting"));
                await this.#actionButton.setProperty("hoverColor", "#444dccff");
                await this.#actionButton.setProperty("disabled", true);
                await this.#actionButton.setProperty("backgroundColor", "#3e46b6ff");

                return;
            }
        }




        this.#uiIntervalUpdate = setTimeout(this.updateUI.bind(this), UPDATEUI_INTERVAL_MS);
    }

    async downloadInstallMod(mod_info) {
        // Build the stam:// URI for this mod
        const uri = mod_info.download_url;

        console.log(`Downloading: ${uri}`);

        this.#UIState.mods[mod_info.id].downloading = true;
        let response;
        try {
            response = await Network.download(uri, (percentage, receivedBytes, totalBytes) => {
                this.#UIState.mods[mod_info.id].received_bytes = receivedBytes;
                this.#UIState.mods[mod_info.id].expected_total_bytes = totalBytes;
            });
        } finally {
            this.#UIState.mods[mod_info.id].downloading = false;
        }

        if (response.status !== 200) {
            throw new Error(`Status ${response.status}`);
        }

        if (!response.temp_file_path) {
            throw new Error("No data received");
        }

        this.#UIState.mods[mod_info.id].downloaded = true;

        this.#UIState.mods[mod_info.id].installing = true;
        System.installModFromPath(response.temp_file_path, mod_info.id).then(() => {
            this.#UIState.mods[mod_info.id].installed = true;
            this.#UIState.mods[mod_info.id].installing = false;
        }).catch((e) => {
            this.#UIState.mods[mod_info.id].installing = false;
            console.error(`Failed to install mod "${mod_info.id}":`, e);
            this.#UIState.last_error_occurred = Locale.getWithArgs("mod-install-failed", { mod_id: mod_info.id });
        });
        return mod_info;
    }

    async attachMod(mod_info) {
        this.#UIState.mods[mod_info.id].attaching = true;
        try {
            await System.attachMod(mod_info.id);
            this.#UIState.mods[mod_info.id].attached = true;
        } finally {
            this.#UIState.mods[mod_info.id].attaching = false;
        }
    }

    async onEnsureAssets(req, res) {

    }
}
