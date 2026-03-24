# Messenger

A lightweight macOS desktop app for Facebook Messenger, built with Electron.

## Features

- **Official messenger app is being deprecated - wrapped fb.com/messenger w/ electron**

## Prerequisites

- [Node.js](https://nodejs.org/) (v18+)
- npm

## Setup

```sh
npm install
```

## Development

```sh
npm start
```

## Build

Builds a macOS `.dmg` and `.app` bundle:

```sh
npm run build
```

Output goes to the `dist/` directory:
- `dist/mac-arm64/Messenger.app` — the app bundle
- `dist/Messenger-1.0.0-arm64.dmg` — distributable disk image

> The app is ad-hoc signed. On first launch you may need to right-click and choose **Open** to bypass Gatekeeper.

## Customization

| File | Purpose |
|------|---------|
| `custom.css` | Injected into the page after load — add your own styles |
| `custom.js` | Injected into the page after load — add your own scripts |
| `icon.iconset/` | macOS icon source PNGs — regenerate `icon.icns` with `iconutil -c icns icon.iconset` |

## Project Structure

```
main.js          — Electron main process (window, session, IPC)
preload.js       — Preload script (exposes safe APIs to the renderer)
custom.css       — Custom styles injected into Messenger
custom.js        — Custom scripts injected into Messenger
icon.icns        — macOS app icon
icon.iconset/    — Source PNGs for the icon
messenger.png    — Original logo source image
package.json     — Project config and build settings
```

## License

ISC
