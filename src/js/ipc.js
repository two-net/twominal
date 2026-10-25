// Thin wrappers over the Tauri globals (`withGlobalTauri`; there is no bundler).

const tauri = window.__TAURI__;

export const invoke = (command, args) => tauri.core.invoke(command, args);

export const Channel = tauri.core.Channel;

export const currentWindow = () => tauri.window.getCurrentWindow();

/** Listens for events sent to this window only. A global listener would also hear events
 * meant for other windows. */
export const listenHere = (event, handler) => tauri.webviewWindow.getCurrentWebviewWindow().listen(event, handler);

/** Listens for app-wide broadcasts such as settings changes. */
export const listenAll = (event, handler) => tauri.event.listen(event, handler);
