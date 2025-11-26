export class Manager {
    constructor() {
        console.log("Initialized.");
    }
    async run() {
        console.log("prints_mods() initial");
        this.print_mods();
        setInterval(() => {
            console.log("prints_mods() intervalled");
            this.print_mods();
        }, 3000);
    }
    print_mods() {
        const mods = system.get_mods();
        console.log(`Found ${mods.length} mods:`);
        for (const mod of mods) {
            console.log(` - ${mod.id} [${mod.mod_type}] loaded=${mod.loaded}`);
        }
    }
}