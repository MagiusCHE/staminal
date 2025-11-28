export function onAttach() {
    console.log("Attached.");
    system.register_event("AppStart", onAppStart, 100);
}
const onAppStart = () => { 
    console.log("AppStart received in probe mod.");
}
