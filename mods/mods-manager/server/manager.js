export class Manager {
    constructor() {
        console.debug("Initialized.");
    }

    async register() {
        // Register handler for mod download requests
        // Route prefix: /mods-manager/ - will match /mods-manager/{mod_id}/download
        system.registerEvent(SystemEvents.RequestUri, this.handle_mod_request.bind(this), 100, "stam://", "/mods-manager/");
    }


    async run() {
    }

    async handle_mod_request(req, res) {
        //  stam://127.0.0.1:9999/mods-manager/{mod_id}/download/client
        console.log("Handling mod request:", req);
        const parts = req.path.replace(/^[\./]+/, '').split("/");
        const action = parts[2];
        const mod_id = parts[1];
        const filter = parts[3] || "client";  // e.g., "client" or "server"
        // enum ModSides { Client, Server }
        const modSide = filter == "server" ? ModSides.Server : ModSides.Client;
        const mods = system.getModPackages(modSide);
        const mod_info = mods.find(m => m.id === mod_id);
        if (!mod_info) {
            console.error(`Mod not found: ${mod_id}`);
            res.status = 404;
            return;
        }
        switch (action) {
            case "download":
                await this.handle_mod_download(req, res, mod_info);
                break;
            default:
                console.error(`Unknown action: ${action}`);
                res.status = 404;
        }
    }
    async handle_mod_download(req, res, mod_info) {
        console.log("Handling mod download for:", mod_info);
        res.filepath = "mod-packages/" + mod_info.path;
        //res.buffer = mod_info.archive_bytes.toString();
        res.status = 200;
    }
}