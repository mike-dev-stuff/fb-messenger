const { app, BrowserWindow, session, ipcMain, shell } = require("electron");
const path = require("path");
const fs = require("fs");

const MESSENGER_URL = "https://www.facebook.com/messages";

const LOADING_HTML = `
<html>
<head>
<style>
  body {
    margin: 0;
    background: #1a1a1a;
    display: flex;
    align-items: center;
    justify-content: center;
    height: 100vh;
    font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Helvetica, Arial, sans-serif;
  }
  .spinner {
    width: 48px;
    height: 48px;
    border: 4px solid rgba(255,255,255,0.1);
    border-top-color: #00C6FF;
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }
  @keyframes spin {
    to { transform: rotate(360deg); }
  }
</style>
</head>
<body><div class="spinner"></div></body>
</html>
`;

function createWindow() {
  const win = new BrowserWindow({
    width: 1200,
    height: 800,
    title: "Messenger",
    icon: path.join(__dirname, "icon.icns"),
    show: false,
    webPreferences: {
      preload: path.join(__dirname, "preload.js"),
      contextIsolation: true,
      nodeIntegration: false,
    },
  });

  // Remove the default menu bar
  win.setMenuBarVisibility(false);

  // Show the loading screen first
  win.loadURL("data:text/html;charset=utf-8," + encodeURIComponent(LOADING_HTML));
  win.once("ready-to-show", () => {
    win.show();
  });

  // Spoof the user-agent so Facebook doesn't block the embedded browser
  const chromeVersion = "124.0.0.0";
  session.defaultSession.webRequest.onBeforeSendHeaders((details, callback) => {
    details.requestHeaders["User-Agent"] = `Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/${chromeVersion} Safari/537.36`;
    callback({ requestHeaders: details.requestHeaders });
  });

  // Now load Messenger in the background
  win.loadURL(MESSENGER_URL);

  // Inject custom CSS and JS after the page loads, then reveal
  win.webContents.on("did-finish-load", async () => {
    const currentURL = win.webContents.getURL();
    if (currentURL.startsWith("data:")) return;

    // Inject custom CSS
    const cssPath = path.join(__dirname, "custom.css");
    if (fs.existsSync(cssPath)) {
      const css = fs.readFileSync(cssPath, "utf-8");
      await win.webContents.insertCSS(css);
    }

    // Inject custom JS
    const jsPath = path.join(__dirname, "custom.js");
    if (fs.existsSync(jsPath)) {
      const js = fs.readFileSync(jsPath, "utf-8");
      await win.webContents.executeJavaScript(js).catch((err) => {
        console.error("Custom JS injection error:", err);
      });
    }
  });

  // Open external links in the default browser
  win.webContents.setWindowOpenHandler(({ url }) => {
    if (!url.startsWith("https://www.facebook.com/messages")) {
      require("electron").shell.openExternal(url);
      return { action: "deny" };
    }
    return { action: "allow" };
  });

  // Intercept navbar and other in-page navigations away from Messenger
  win.webContents.on("will-navigate", (event, url) => {
    if (!url.startsWith("https://www.facebook.com/messages")) {
      event.preventDefault();
      require("electron").shell.openExternal(url);
    }
  });
}

// Handle open-external requests from the renderer
ipcMain.on("open-external", (event, url) => {
  if (typeof url === "string" && url.startsWith("https://")) {
    shell.openExternal(url);
  }
});

// Update dock badge when unread count changes
ipcMain.on("set-badge-count", (event, count) => {
  if (process.platform === "darwin") {
    app.dock.setBadge(count > 0 ? String(count) : "");
  }
});

app.whenReady().then(createWindow);

app.on("window-all-closed", () => {
  app.quit();
});

app.on("activate", () => {
  if (BrowserWindow.getAllWindows().length === 0) {
    createWindow();
  }
});
