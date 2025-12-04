import { Game } from "./game.js";

export function onAttach() {
    console.log("Attached.");
    System.registerEvent("AppStart", onAppStart, 100);
}

const onAppStart = async (req, res) => {
    //console.log("App started. req:", req, "res", res);
    // Get all opened windows and close them, keeping only one with no widgets
    res.handled = true;

    const game = new Game();

    game.run();
}
