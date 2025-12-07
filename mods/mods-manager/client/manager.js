import { wait, humanizeBytes } from "@js-helper";

const UPDATEUI_INTERVAL_MS = 500;

const INITIAL_FONT_SIZE = 20;

export class Manager {
    #gameInfo;
    #window;

    // UI elements for loading screen (ECS entities)
    #loadingContainer;      // Entity: main container
    #progressBarsContainer; // Entity: container for progress bars
    #statusLabel;           // Entity: status text
    #mainProgressBarContainer;  // Entity: progress bar background
    #mainProgressBarFill;       // Entity: progress bar fill
    #mainProgressText;          // Entity: progress text
    #secondBarContainer;    // Entity: secondary progress bar background
    #secondProgressBarFill; // Entity: secondary progress bar fill
    #actionButton;          // Entity: button container (ECS with Interaction+Button)
    #actionButtonText;      // Entity: button text

    constructor() {
        this.#gameInfo = System.getGameInfo();
    }

    register() {
        System.registerEvent(SystemEvents.TerminalKeyPressed, this.onTerminalKeyPressed.bind(this), 100);
        System.registerEvent(SystemEvents.GraphicEngineReady, this.onGraphicEngineReady.bind(this), 100);
        System.registerEvent(SystemEvents.GraphicEngineWindowClosed, this.onGraphicEngineWindowClosed.bind(this), 100);
        
        // Should download assets or use mod pack to encapsulate them?
        //System.registerEvent("EnsureAssets", this.onEnsureAssets.bind(this), 100);
    }

    async onTerminalKeyPressed(req, res) {
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
        });
    }

    async onGraphicEngineWindowClosed(req, res) {
        const engine = await Graphic.getEngineInfo();
        if (req.windowId === engine.mainWindow.id) {
            System.exit(0);
            res.handled = true;
        }
    }

    async onGraphicEngineReady() {
        const engine = await Graphic.getEngineInfo();
        this.#window = engine.mainWindow;

        await this.prepareUi();

        //this.test()
        //this.updateUI();
        this.ensureMods();

    }

    // Handle ECS entity interaction changes (click, hover, etc.)
    // async onEntityInteractionChanged(req, res) {
    //     console.log("Entity interaction changed:", req);
    //     if (!this.#actionButton) return;

    //     // Check if this event is for our button
    //     if (req.entityId === this.#actionButton.id && req.interaction === "pressed") {
    //         await this.onActionClicked();
    //         res.handled = true;
    //     }
    // }

    async prepareUi() {
        this.resetUiState();
        // Load custom font
        await Graphic.loadFont("macondo", System.getAssetsPath("fonts/Macondo/Macondo-Regular.ttf"));

        await this.#window.setTitle("Staminal: " + this.#gameInfo.name);
        this.#window.setFont("macondo", INITIAL_FONT_SIZE);

        // Note: The root node of the window already has flex_direction: column
        // We spawn a main container parented to window root, then all UI elements as children

        // Background container that fills 100% - also sets the dark background
        // This is parented to the window root (no parent specified = window root)
        this.#loadingContainer = await World.spawn({
            Node: {
                width: "100%",
                height: "100%",
                flex_direction: FlexDirection.Column,
                justify_content: JustifyContent.Center,
                align_items: AlignItems.Center,
                padding: 20,
                row_gap: 16
            },
            BackgroundColor: "#1a1a2e"
        });

        // Status label: "Loading mods:" - child of loading container
        this.#statusLabel = await World.spawn({
            Node: {
                width: "auto",
                height: "auto"
            },
            Text: {
                value: Locale.get("mods-ensuring-title"),
                font_size: INITIAL_FONT_SIZE * 1.3,
                color: "#ffffff"
            }
        }, this.#loadingContainer);

        this.#progressBarsContainer = await World.spawn({
            Node: {
                width: "100%",
                height: "100%",
                flex_direction: FlexDirection.Column,
                justify_content: JustifyContent.Center,
                align_items: AlignItems.Center,
                padding: 0,
                row_gap: 4
            },
            //BackgroundColor: "#FF0000",
        }, this.#loadingContainer);
        
        // Main progress bar container (background track) - child of loading container
        // Contains both the fill bar and the text overlay
        this.#mainProgressBarContainer = await World.spawn({
            Node: {
                width: "90%",
                height: 30,
                flex_direction: FlexDirection.Row,
                justify_content: JustifyContent.Center,
                align_items: AlignItems.Center
            },
            BackgroundColor: "#333344",
            BorderRadius: 4
        }, this.#progressBarsContainer);
       
        // Main progress bar fill (the colored part that grows) - positioned absolute
        this.#mainProgressBarFill = await World.spawn({
            Node: {
                width: "0%",
                height: "100%",
                position_type: PositionType.Absolute,
                left: 0,
                top: 0,
                bottom: 0
            },
            BackgroundColor: "#4a9eff",
            BorderRadius: 4
        }, this.#mainProgressBarContainer);
        

        // Progress text - child of progress bar container, centered on top of the fill
        this.#mainProgressText = await World.spawn({
            Node: {
                width: "auto",
                height: "auto"
            },
            Text: {
                value: "",
                font_size: INITIAL_FONT_SIZE,
                color: "#ffffff"
            }
        }, this.#mainProgressBarContainer);

        // Secondary download progress bar container - child of loading container
        this.#secondBarContainer = await World.spawn({
            Node: {
                width: "90%",
                height: 15,
                flex_direction: FlexDirection.Row,
                justify_content: JustifyContent.FlexStart,
                align_items: AlignItems.Stretch
            },
            BackgroundColor: "#333344",
            BorderRadius: 2
        }, this.#progressBarsContainer);

        // Secondary download progress bar fill - child of secondary bar container
        this.#secondProgressBarFill = await World.spawn({
            Node: {
                width: "0%",
                height: "100%"
            },
            BackgroundColor: "#7ac74f",
            BorderRadius: 2
        }, this.#secondBarContainer);

        // Action button - child of loading container, using pure ECS with Interaction + Button components
        this.#actionButton = await World.spawn({
            Node: {
                width: "auto",
                height: "auto",
                padding: { top: 8, right: 16, bottom: 8, left: 16 },
                justify_content: JustifyContent.Center,
                align_items: AlignItems.Center
            },
            BackgroundColor: "#cc3333",
            DisabledBackgroundColor: "#884444",
            HoverBackgroundColor: "#dd5555",
            PressedBackgroundColor: "#aa2222",
            BorderRadius: 10,
            Button: {
                on_click: this.onActionClicked.bind(this)
            },
            Interaction: {}
        }, this.#loadingContainer);

        // Button text - child of action button
        this.#actionButtonText = await World.spawn({
            Node: {
                width: "auto",
                height: "auto"
            },
            Text: {
                value: Locale.get("cancel"),
                font_size: INITIAL_FONT_SIZE,
                color: "#ffffff"
            }
        }, this.#actionButton);

    }

    #buttonDisabled = false;

    async onActionClicked() {
        if (this.#buttonDisabled) return;
        this.#buttonDisabled = true;

        // Dim the button to show it's disabled
        //await this.#actionButton.update("BackgroundColor", "#666666");

        if (this.#UIState.last_error_occurred) {
            await wait(500);
            System.exit(1);
            return;
        }

        this.#UIState.cancelled = true;
        this.updateUI();

        await wait(500);
        System.exit(0);
    }

    #UIState = undefined

    resetUiState() {
        this.#UIState = {
            to_download: 0,
            to_attach: 0,
            mods: {},
            expected_total_bytes: 0,
            last_error_occurred: undefined,
            cancelled: false,
            completed: false,
            previous_received: 0,
            previous_bps_str: "0 B/s",
            previous_timestamp: Date.now()
        }
    }

    async ensureMods() {
        const mods = System.getMods();
        //console.trace(`mods:`, mods);
        const toAnalize = mods.filter(mod => !mod.loaded);        

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

        this.updateUI();

        for (const mod of toDownload) {
            if (this.#UIState.cancelled) {
                console.log("Loading cancelled by user");
                return;
            }

            try {
                await this.downloadInstallMod(mod);
            } catch (e) {
                return;
            }
        }

        console.log("All downloads initiated, waiting for installations to complete...");
        while (!this.#UIState.cancelled && !this.#UIState.last_error_occurred) {
            const pendingInstalls = Object.entries(this.#UIState.mods).filter(([modId, modState]) => !modState.installed && !modState.exists);
            if (pendingInstalls.length === 0) {
                break;
            }
            await wait(250);
        }
        console.log("All installations completed.");

        if (this.#UIState.cancelled) return;
        if (this.#UIState.last_error_occurred) return;

        for (const modId in toDownload) {
            const mod = toDownload[modId];
            toAttach.push(mod);
        }
        console.log("Attaching %o mods...", toAttach.length);
        for (const mod of toAttach) {
            try {
                await this.attachMod(mod);
            } catch (e) {
                console.error(`Failed to attach mod "${mod.id}":`, e);
                this.#UIState.last_error_occurred = Locale.getWithArgs("mod-attach-failed", { mod_id: mod.id });
                return;
            }

            if (this.#UIState.cancelled) return;
        }

        console.log("All mods loaded successfully!");

        this.#UIState.completed = true;

        await wait(1500);

        const ret = await System.sendEvent("AppStart");
        if (!ret.handled) {
            console.error("AppStart event was not handled by any mod!");
            System.exit(0);
        }
    }

    #uiIntervalUpdate = undefined;

    async updateUI() {
        if (this.#uiIntervalUpdate) {
            clearTimeout(this.#uiIntervalUpdate);
            this.#uiIntervalUpdate = undefined;
        }
        if (this.#UIState.last_error_occurred) {
            // Red state - error occurred
            await this.#statusLabel.update("Text", { value: Locale.get("error-occurred") });
            await this.#statusLabel.update("TextColor", "#ff4444");

            await this.#mainProgressBarFill.update("BackgroundColor", "#cc3333");
            await this.#secondProgressBarFill.update("BackgroundColor", "#cc3333");

            // Update button to "Exit"
            await this.#actionButtonText.update("Text", { value: Locale.get("exit") });
            this.#buttonDisabled = false;            
            return;
        } else if (this.#UIState.cancelled) {
            await this.#actionButtonText.update("Text", { value: Locale.get("cancelling") });
            await this.#actionButton.update("Disabled", true);            
            this.#buttonDisabled = true;
            return;
        }

        const isDownloading = Object.values(this.#UIState.mods).some(modState => modState.downloading);
        const isAttaching = Object.values(this.#UIState.mods).some(modState => modState.attaching);
        const isInstalling = Object.values(this.#UIState.mods).some(modState => modState.installing);

        //console.log("Is downloading:", isDownloading, "is installing:", isInstalling, "is attaching:", isAttaching);
        if (isDownloading) {
            const todoDone = {
                todo: this.#UIState.to_download,
                done: Object.entries(this.#UIState.mods).filter(([modId, modState]) => modState.downloaded).length + 1
            }
            await this.#statusLabel.update("Text", { value: Locale.getWithArgs("mods-downloading-title", todoDone) });

            const mainProgress = (todoDone.done > 0 ? (todoDone.todo > 0 ? Math.floor((todoDone.done / todoDone.todo) * 100) : 100) : 0) + "%";
            await this.#mainProgressBarFill.update("Node", { width: mainProgress });

            const received = Object.values(this.#UIState.mods).reduce((acc, modState) => acc + (modState.received_bytes || 0), 0)
            const percent = (received > 0 ? (this.#UIState.expected_total_bytes > 0 ? Math.floor((received / this.#UIState.expected_total_bytes) * 100) : 1) : 0) + "%";
            await this.#secondProgressBarFill.update("Node", { width: percent });

            const now = Date.now();
            const elapsed_ms = now - this.#UIState.previous_timestamp;
            const bytes_diff = received - this.#UIState.previous_received;

            let bps_str = this.#UIState.previous_bps_str || "0 B/s";
            if (elapsed_ms >= 1000) {
                const bps = (bytes_diff / elapsed_ms) * 1000;
                bps_str = humanizeBytes(Math.floor(bps)) + "/s";

                this.#UIState.previous_bps_str = bps_str;
                this.#UIState.previous_received = received;
                this.#UIState.previous_timestamp = now;
            }

            await this.#mainProgressText.update("Text", {
                value: Locale.getWithArgs("mods-downloading-progress", {
                    received: humanizeBytes(received),
                    total: humanizeBytes(this.#UIState.expected_total_bytes),
                    bps: bps_str,
                })
            });
        } else if (isInstalling) {
            const todoDone = {
                todo: this.#UIState.to_download,
                done: Object.entries(this.#UIState.mods).filter(([modId, modState]) => modState.installed && modState.downloaded).length + 1
            }
            await this.#statusLabel.update("Text", { value: Locale.getWithArgs("mods-installing-title", todoDone) });

            const mainProgress = (todoDone.done > 0 ? (todoDone.todo > 0 ? Math.floor((todoDone.done / todoDone.todo) * 100) : 100) : 0) + "%";
            await this.#mainProgressBarFill.update("Node", { width: mainProgress });

            await this.#secondProgressBarFill.update("Node", { width: "50%" });

            await this.#mainProgressText.update("Text", { value: Locale.get("mods-installing-progress") });
        } else if (isAttaching) {
            const todoDone = {
                todo: this.#UIState.to_attach,
                done: Object.entries(this.#UIState.mods).filter(([modId, modState]) => modState.attached).length + 1
            }
            await this.#statusLabel.update("Text", { value: Locale.getWithArgs("mods-attaching-title", todoDone) });

            const mainProgress = (todoDone.done > 0 ? (todoDone.todo > 0 ? Math.floor((todoDone.done / todoDone.todo) * 100) : 100) : 0) + "%";
            await this.#mainProgressBarFill.update("Node", { width: mainProgress });

            await this.#secondProgressBarFill.update("Node", { width: "50%" });

            await this.#mainProgressText.update("Text", { value: Locale.get("mods-attaching-progress") });
        } else {
            if (this.#UIState.completed) {
                await this.#mainProgressText.update("Text", {
                    value: Locale.getWithArgs("loading-complete", { mods: Object.entries(this.#UIState.mods).length })
                });

                await this.#mainProgressBarFill.update("Node", { width: "100%" });

                await this.#secondProgressBarFill.update("Node", { width: "100%" });

                await this.#statusLabel.update("Text", {
                    value: Locale.getWithArgs("starting-game", { game_name: this.#gameInfo.name })
                });

                // Update button to "Starting"
                await this.#actionButtonText.update("Text", { value: Locale.get("starting") });
                console.warn("Game is starting...", Locale.get("starting"));
                await this.#actionButton.update({
                    BackgroundColor: "#3e46b6ff",
                    DisabledBackgroundColor: "#3e46b6ff",
                    Disabled: true,
                });
                this.#buttonDisabled = true;

                return;
            }
        }

        this.#uiIntervalUpdate = setTimeout(this.updateUI.bind(this), UPDATEUI_INTERVAL_MS);
    }

    async downloadInstallMod(mod_info) {
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
