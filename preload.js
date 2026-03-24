// Preload script — runs in an isolated context before the page loads.
// You can expose safe APIs to the page via contextBridge here if needed.
const { contextBridge, ipcRenderer } = require("electron");

contextBridge.exposeInMainWorld("messengerApp", {
  platform: process.platform,
  openExternal: (url) => ipcRenderer.send("open-external", url),
  setBadgeCount: (count) => ipcRenderer.send("set-badge-count", count),
});
