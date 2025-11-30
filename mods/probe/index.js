export function onAttach() {
    console.log("Attached.");
    system.registerEvent("AppStart", onAppStart, 100);
}
const onAppStart = () => { 
    console.log("AppStart received in probe mod.");
}
