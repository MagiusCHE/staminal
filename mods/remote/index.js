export function onAttach() {
    console.log("mod attached.");
}

export function onBootstrap() {
    console.log("mod bootstrapped.");
    //throw new Error("Bootstrap error for testing purposes.");
}