import { wait } from "@js-helper";
export class Manager {
    constructor() {
        console.log("Initialized.");
    }

    async run() {
        await this.prepare_ui();
        await this.ensure_mods();
    }

    async prepare_ui() {
        console.log("Preparing UI...");
        console.warn("TODO: implement User Interface to show mods loading/startup progress...");
    }

    async ensure_mods() {
        console.log("Ensuring mods...");
        const mods = system.get_mods();
        const toload = mods.filter(mod => !mod.loaded);

        if (toload.length === 0) {
            console.log("All mods already loaded.");
            return;
        }

        console.log(`${toload.length} mod(s) need to be downloaded:`, toload.map(m => m.id).join(", "));

        let error_occurred = undefined;
        for (const mod of toload) {
            console.log(`Downloading mod: ${mod.id}...`);
            this.download_mod(mod).then(() => toload.splice(toload.indexOf(mod), 1)).catch((e) => {
                console.error(`Failed to download mod ${mod.id}: ${e}`);
                error_occurred = locale.translate("mod-download-failed", { mod_id: mod.id });
            });
            if (error_occurred) {
                break;
            }
        }


        if (error_occurred) {
            // TOOD: Show error in UI
            await this.exit_with_error(error_occurred);
        }
    }

    async exit_with_error(message) {
        console.error(`Exiting due to error: ${message}`);
        console.warn("TODO: Implement User Interface to show error message before exiting...");
        system.exit(1);
    }

    async download_mod(mod_info) {
        console.warn(`TODO: Simulating download BEGIN of mod ${mod_info.id}...`);
        await wait(1000);
        console.warn(`TODO: Simulating download DONE of mod ${mod_info.id}`);
        throw new Error("Download failed: Not implemented yet");
    }


    print_mods() {
        const mods = system.get_mods();
        console.log(`Found ${mods.length} mods:`);
        for (const mod of mods) {
            console.log(` - ${mod.id} [${mod.mod_type}] loaded=${mod.loaded}`);
        }
    }
}