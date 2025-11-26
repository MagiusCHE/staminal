export function onAttach() { }

export async function wait(milliseconds) {
    await new Promise(resolve => setTimeout(resolve, milliseconds));
}