export class Manager {
    constructor() {
        console.log("Initialized.");
    }

    async register() {
        // Initialization logic here
        system.register_event(SystemEvents.RequestUri, this.handle_mod_download_request.bind(this), 100, "stam://", "/mods-manager/download");
    }

    async run() {
    }

    async handle_mod_download_request(request, response) {
        console.log("Handling mod download request:", request);
    }
}