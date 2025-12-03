export function onAttach() { }

export async function wait(milliseconds) {
    await new Promise(resolve => setTimeout(resolve, milliseconds));
}


/**
 * Humanize bytes to a human-readable format
 * @param {number} bytes - Number of bytes
 * @returns {string} Human-readable string (e.g., "1.5 MB")
 */
export function humanizeBytes(bytes) {
    if (bytes === 0) return "0 B";
    const units = ["B", "KB", "MB", "GB", "TB"];
    const k = 1024;
    const i = Math.floor(Math.log(bytes) / Math.log(k));
    const value = bytes / Math.pow(k, i);
    return `${value.toFixed(i > 0 ? 1 : 0)} ${units[i]}`;
}