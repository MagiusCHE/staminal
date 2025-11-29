import { wait } from "@js-helper";
export class Manager {
    #game_info;
    constructor() {
        console.log("Initialized.");
        this.#game_info = system.get_game_info();
    }

    register() {
        system.register_event(SystemEvents.TerminalKeyPressed, this.TerminalKeyPressed.bind(this), 100);
    }
    async TerminalKeyPressed(req, res) {
        //console.log("Console key pressed:", req);
        if (req.key == "c" && req.ctrl) {
            res.handled = true;
            system.exit(0);
            return;
        }
    }

    async run() {
        await this.prepare_ui();
        await this.ensure_mods();
    }

    async prepare_ui() {
        console.log("Preparing UI for game %o", this.#game_info.id);
        
        // await system.enable_graphic_engine(GraphicEngines.Bevy)
        // this.#window = window.get_main_window();
        // //window.create("Staminal2: " + this.#game_info.id, 800, 600, true);
        // this.#window.set_position_mode(WindowPositionModes.Centered);
        // this.#window.set_size(1280, 720);
        // this.#window.set_title("Staminal: " + this.#game_info.name);
        // this.#window.set_resizable(true);

        console.warn("TODO: implement User Interface to show mods loading/startup progress...");
    }

    async ensure_mods() {
        console.log("Ensuring mods...");
        const mods = system.get_mods();
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
                this.download_install_mod(mod).then(() => toload.splice(toload.indexOf(mod), 1)).catch((e) => {
                    console.error(`Failed to download mod "${mod.id}":`, e);
                    error_occurred = locale.get_with_args("mod-download-failed", { mod_id: mod.id });
                });
            } else {
                console.log(`Attaching mod: ${mod.id}...`);
                system.attach_mod(mod.id).then(() => toload.splice(toload.indexOf(mod), 1)).catch((e) => {
                    console.error(`Failed to attach mod "${mod.id}":`, e);
                    error_occurred = locale.get_with_args("mod-attach-failed", { mod_id: mod.id });
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
            await this.exit_with_error(error_occurred);
        }

        // Now start the game!
        system.send_event("AppStart");
    }

    async exit_with_error(message) {
        console.error(`Exiting due to error: ${message}`);
        console.warn("TODO: Implement User Interface to show error message before exiting...");
        system.exit(1);
    }

    async download_install_mod(mod_info) {
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
        await system.install_mod_from_path(response.temp_file_path, mod_info.id); //test
        await system.attach_mod(mod_info.id);
        return;
    }


    print_mods() {
        const mods = system.get_mods();
        console.log(`Found ${mods.length} mods:`);
        for (const mod of mods) {
            console.log(` - ${mod.id} [${mod.mod_type}] loaded=${mod.loaded}`);
        }
    }
}