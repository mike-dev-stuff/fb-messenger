# Messenger

A lightweight macOS desktop app for Facebook Messenger, built with [Tauri](https://v2.tauri.app/).

## Features

- Official Messenger app is being deprecated — this wraps Messenger in a native window
- Unread count mirrored onto the dock icon
- Facebook's top bar hidden, so you get Messenger without the Facebook chrome
- Zoom with Cmd+- / Cmd+= / Cmd+0, remembered between launches
- Off-site links open in your real browser instead of hijacking the window
- ~4 MB app bundle, using the system WebKit rather than shipping a browser

## Prerequisites

- [Node.js](https://nodejs.org/) (v18+) — for the Tauri CLI
- [Rust](https://rustup.rs/) (1.77.2+) — `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
- Xcode Command Line Tools — `xcode-select --install`

## Setup

```sh
npm install
```

## Development

```sh
npm start
```

Rust changes trigger a rebuild and relaunch automatically.

## Build

Builds a macOS `.app` bundle and `.dmg`:

```sh
npm run build
```

Output goes to `src-tauri/target/release/bundle/`:
- `macos/Messenger.app` — the app bundle
- `dmg/Messenger_0.0.2_aarch64.dmg` — distributable disk image

> The app is ad-hoc signed. On first launch you may need to right-click and choose **Open** to bypass Gatekeeper.

## Customization

| File | Purpose |
|------|---------|
| `custom.css` | Compiled into the binary and injected before the page renders — add your own styles |
| `src/index.html` | The loading spinner shown while Messenger boots |
| `messenger.png` | Icon source — regenerate the icon set with `npx tauri icon messenger.png` |

Because `custom.css` is compiled in with `include_str!` (there is no project
directory to read from inside a `.app`), changes need a rebuild rather than
just a reload.

## Project Structure

```
src/index.html                          — loading screen, shown until Messenger loads
custom.css                              — custom styles injected into Messenger
messenger.png                           — icon source image
src-tauri/src/lib.rs                    — window setup, navigation rules, dock badge
src-tauri/src/main.rs                   — entry point
src-tauri/build.rs                      — declares the update_badge command to the ACL
src-tauri/tauri.conf.json               — app, bundle and window config
src-tauri/capabilities/                 — IPC permissions (see note below)
src-tauri/icons/                        — generated app icons
```

### Why facebook.com/messages and not messenger.com

messenger.com keeps a session entirely separate from facebook.com, and its
SSO handoff is broken: the "Continue" button redirects to

```
facebook.com/two_step_verification/authentication/?…&flow=pre_authentication
```

which renders "This content isn't available right now" every time. Being
logged into Facebook does not log you into messenger.com, so `messenger.com/`
just serves the logged-out marketing page.

`facebook.com/messages` works with the ordinary Facebook session. The cost is
Facebook's top bar, which `custom.css` hides — it is the only navigation
landmark labelled `Facebook`, while Messenger's chat list is labelled
`Thread list`, so the selector can target one without touching the other.

### Debugging navigation

If a page loads somewhere unexpected, trace the main-frame navigations:

```sh
MESSENGER_DEBUG=1 /Applications/Messenger.app/Contents/MacOS/messenger
```

`MESSENGER_URL` and `MESSENGER_UA` override the start page and user agent, so
you can test a different page or browser identity without rebuilding.

Each `[nav]` line is one main-frame load. This is the fastest way to find a
domain that needs adding to `INTERNAL_DOMAINS`.

### A note on link handling

There is deliberately **no `on_navigation` guard**. wry invokes that hook for
every navigation including subframes — it never checks `targetFrame` — so
cancelling off-site URLs there also kills any third-party iframe. Facebook's
two-step verification embeds its captcha from `fbsbx.com`, and blocking it
made logging in impossible: the window sat on a half-rendered Meta shell
while the captcha opened in your default browser instead.

Links are handled in two hooks that only ever see the main frame:

- `on_new_window` — where `target="_blank"` links from messages land
- `on_page_load` — a backstop that bounces the window back if a foreign page
  takes over the main frame

If login breaks again after Meta adds a domain, add it to `INTERNAL_DOMAINS`
rather than reaching for a navigation guard. `cargo test` covers the
domain-matching rules, lookalikes (`fbsbx.com.evil.co`) included.

### A note on `capabilities/`

Messenger is a **remote** origin, so it cannot reach the app's IPC unless a
capability explicitly says so. `capabilities/messenger-remote.json` grants
`allow-update-badge` to `*.messenger.com` / `*.facebook.com`, and `build.rs`
is what generates that permission from the command name. Remove either one
and the dock badge stops updating — silently, with only a console error on
the page.

## License

ISC

## Attributions
<a href="https://www.flaticon.com/free-icons/logo" title="logo icons">Logo icons created by Freepik - Flaticon</a>
