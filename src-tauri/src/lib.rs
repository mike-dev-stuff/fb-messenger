use std::sync::Mutex;

use tauri::menu::{AboutMetadata, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::webview::{NewWindowFeatures, NewWindowResponse, PageLoadEvent, WebviewWindowBuilder};
use tauri::Manager;
use tauri::{Url, WebviewUrl};
use tauri_plugin_opener::OpenerExt;

/// facebook.com/messages, not messenger.com.
///
/// messenger.com keeps a session separate from facebook.com, and its SSO
/// handoff is broken: "Continue" redirects to
/// facebook.com/two_step_verification/authentication?flow=pre_authentication
/// which renders "This content isn't available right now" every time. This
/// URL works with the ordinary Facebook session, and `custom.css` hides the
/// Facebook bar so it still looks like Messenger.
const MESSENGER_URL: &str = "https://www.facebook.com/messages";

/// Start page. Overridable with MESSENGER_URL for testing without a rebuild.
fn start_url() -> String {
    std::env::var("MESSENGER_URL").unwrap_or_else(|_| MESSENGER_URL.to_string())
}

/// Tauri uses WKWebView on macOS, whose default user-agent omits the
/// `Version/... Safari/...` suffix — Facebook reads that as an unsupported
/// browser. Claim a plain macOS Safari rather than Chrome: the engine really
/// *is* WebKit, so asking for Chrome code paths only invites breakage.
const USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) \
     AppleWebKit/605.1.15 (KHTML, like Gecko) Version/26.6 Safari/605.1.15";

/// Compiled in, not read from disk: inside a `.app` bundle there is no
/// project directory to read from. Edit the file and rebuild.
const CUSTOM_CSS: &str = include_str!("../../custom.css");

/// Injected before the page is parsed. Two jobs: apply `custom.css`, and
/// mirror the unread count in the page title onto the dock icon.
const BRIDGE_JS: &str = r#"
(function () {
  if (!/(^|\.)(messenger|facebook)\.com$/.test(location.hostname)) return;

  function invoke(cmd, args) {
    var core = window.__TAURI__ && window.__TAURI__.core;
    if (core && core.invoke) return core.invoke(cmd, args);
    var internals = window.__TAURI_INTERNALS__;
    if (internals && internals.invoke) return internals.invoke(cmd, args);
    return Promise.reject(new Error("Tauri IPC unavailable"));
  }

  function injectCss() {
    var css = globalThis.__MESSENGER_CSS__;
    if (!css || !css.trim()) return;
    var style = document.createElement("style");
    style.textContent = css;
    (document.head || document.documentElement).appendChild(style);
  }

  var lastCount = -1;
  function syncBadge() {
    var match = document.title.match(/\((\d+)\)/);
    var count = match ? parseInt(match[1], 10) : 0;
    if (count === lastCount) return;
    lastCount = count;
    invoke("update_badge", { count: count }).catch(function (err) {
      console.error("[Messenger] badge update failed:", err);
    });
  }

  function start() {
    injectCss();
    syncBadge();
    // Messenger is a SPA that replaces the <title> node wholesale, so poll
    // instead of binding an observer to an element that gets swapped out.
    // The lastCount guard keeps this to one IPC call per actual change.
    setInterval(syncBadge, 1000);
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", start);
  } else {
    start();
  }
})();
"#;

/// Reports the rendered page state back over IPC. Only injected when
/// MESSENGER_DEBUG is set; catches client-side routing that never fires a
/// main-frame navigation.
const DEBUG_REPORTER_JS: &str = r#"
(function () {
  function inv(cmd, args) {
    var c = window.__TAURI__ && window.__TAURI__.core;
    if (c && c.invoke) return c.invoke(cmd, args);
    var i = window.__TAURI_INTERNALS__;
    if (i && i.invoke) return i.invoke(cmd, args);
    return Promise.reject(new Error("no ipc"));
  }
  var last = "";
  function tick() {
    var body = document.body ? document.body.innerText.replace(/\s+/g, " ").slice(0, 160) : "(no body)";
    var msg = "url=" + location.href + " | vw=" + window.innerWidth + "x" + window.innerHeight +
              " | title=" + document.title + " | body=" + body;
    if (msg !== last) {
      last = msg;
      inv("debug_report", { what: msg }).catch(function () {});
    }
  }
  function probeLinks() {
    var main = document.querySelector("[role=main]") || document.body;
    var as = main ? main.querySelectorAll("a[href]") : [];
    var seen = {}, out = [];
    Array.prototype.forEach.call(as, function (a) {
      var h = a.getAttribute("href") || "";
      if (h.indexOf("http") !== 0) return;
      var host = "";
      try { host = new URL(a.href).hostname; } catch (e) { host = "?"; }
      var key = host + "|" + (a.getAttribute("target") || "-");
      if (seen[key]) return;
      seen[key] = 1;
      out.push(host + " target=" + (a.getAttribute("target") || "-") + " href=" + a.href.slice(0, 90));
    });
    inv("debug_report", { what: "links(" + as.length + ") " + (out.join(" ;; ") || "none") })
      .catch(function () {});
  }
  setInterval(tick, 4000);
  setTimeout(tick, 2500);
  setTimeout(probeLinks, 12000);

})();
"#;

/// Hosts that stay inside the app. Deliberately broad: login, checkpoint and
/// 2FA hop between several Meta domains, and sending any of them to the
/// browser strands you mid-sign-in.
const INTERNAL_DOMAINS: [&str; 6] = [
    "messenger.com",
    "facebook.com",
    "fbcdn.net",
    "fb.com",
    // Two-step verification serves its captcha frame from fbsbx.com.
    "fbsbx.com",
    "facebook.net",
];

/// The app's own splash page. Its origin differs between a bundled build
/// (`tauri://localhost`) and `tauri dev`, which serves the frontend over a
/// local HTTP server — both must count as ours, or the splash gets treated
/// as an off-site link and kicked out to the browser.
fn is_local_app(url: &Url) -> bool {
    match url.scheme() {
        "tauri" | "asset" => true,
        "http" | "https" => matches!(
            url.host_str(),
            Some("tauri.localhost" | "localhost" | "127.0.0.1")
        ),
        _ => false,
    }
}

/// Login sometimes throws a reCAPTCHA, which loads as a google.com iframe.
/// That counts as a navigation, so without this it gets treated as an
/// off-site link and opened in the default browser — leaving the in-app
/// login stranded forever on a half-rendered shell.
///
/// Scoped to the captcha paths on purpose: a plain google.com link someone
/// sends you in a chat should still open in your real browser.
fn is_login_challenge(url: &Url) -> bool {
    let Some(host) = url.host_str() else {
        return false;
    };

    let challenge_host = matches!(host, "google.com" | "recaptcha.net" | "gstatic.com")
        || host.ends_with(".google.com")
        || host.ends_with(".recaptcha.net")
        || host.ends_with(".gstatic.com");

    challenge_host && url.path().contains("/recaptcha")
}

/// Facebook rewrites links in messages through a redirector:
/// `https://l.facebook.com/l.php?u=<real url>&h=...`
///
/// The shim lives on a facebook.com subdomain, so it reads as internal and a
/// clicked link would open inside the app. Unwrap it to the real destination,
/// which also skips the tracking hop.
fn unwrap_link_shim(url: &Url) -> Option<Url> {
    let host = url.host_str()?;
    let is_shim = matches!(
        host,
        "l.facebook.com" | "lm.facebook.com" | "l.messenger.com" | "lm.messenger.com"
    ) && url.path() == "/l.php";

    if !is_shim {
        return None;
    }

    let target = url.query_pairs().find(|(k, _)| k == "u")?.1.into_owned();
    let parsed = target.parse::<Url>().ok()?;
    matches!(parsed.scheme(), "http" | "https").then_some(parsed)
}

fn is_internal(url: &Url) -> bool {
    if is_local_app(url) || is_login_challenge(url) {
        return true;
    }

    match url.scheme() {
        // Anything the webview loads inline.
        "data" | "blob" | "about" => return true,
        "http" | "https" => {}
        _ => return false,
    }

    let Some(host) = url.host_str() else {
        return false;
    };

    INTERNAL_DOMAINS
        .iter()
        .any(|domain| host == *domain || host.ends_with(&format!(".{domain}")))
}

const ZOOM_MIN: f64 = 0.5;
const ZOOM_MAX: f64 = 2.0;
const ZOOM_STEP: f64 = 0.1;

/// Current page zoom, persisted so the window reopens at the size you left it.
struct Zoom(Mutex<f64>);

fn zoom_file(app: &tauri::AppHandle) -> Option<std::path::PathBuf> {
    let dir = app.path().app_config_dir().ok()?;
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir.join("zoom"))
}

fn load_zoom(app: &tauri::AppHandle) -> f64 {
    zoom_file(app)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| s.trim().parse::<f64>().ok())
        .filter(|z| (ZOOM_MIN..=ZOOM_MAX).contains(z))
        .unwrap_or(1.0)
}

/// `delta` of `None` resets to 100%.
fn adjust_zoom(app: &tauri::AppHandle, delta: Option<f64>) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let state = app.state::<Zoom>();
    let Ok(mut zoom) = state.0.lock() else {
        return;
    };

    *zoom = match delta {
        // Round to a step boundary so repeated presses don't drift on
        // floating-point error.
        Some(d) => (((*zoom + d) / ZOOM_STEP).round() * ZOOM_STEP).clamp(ZOOM_MIN, ZOOM_MAX),
        None => 1.0,
    };

    let _ = window.set_zoom(*zoom);
    if let Some(path) = zoom_file(app) {
        let _ = std::fs::write(path, zoom.to_string());
    }
}

/// Mirror the unread count onto the macOS dock icon. `0` clears the badge.
///
/// The page calling this is a remote origin, so it only reaches the IPC
/// because `capabilities/messenger-remote.json` grants `allow-update-badge`
/// to facebook.com, and `build.rs` declares the command so that permission
/// exists at all. Drop either and the badge stops updating silently.
#[tauri::command]
fn update_badge(window: tauri::WebviewWindow, count: i64) -> Result<(), String> {
    let badge = if count > 0 { Some(count) } else { None };
    window.set_badge_count(badge).map_err(|e| e.to_string())
}

/// Reports what the page actually rendered. Only wired up when
/// MESSENGER_DEBUG is set; catches client-side routing that never fires a
/// main-frame navigation.
#[tauri::command]
fn debug_report(what: String) {
    eprintln!("[page] {what}");
}

/// `custom.css` has no injection point of its own, so hand it to the bridge
/// as a global. JSON-encoding it keeps quotes and newlines from breaking out
/// of the string literal.
fn initialization_script() -> String {
    let mut script = String::from("globalThis.__MESSENGER_CSS__ = ");
    script.push_str(&serde_json::to_string(CUSTOM_CSS).unwrap_or_else(|_| "\"\"".into()));
    script.push_str(";\n");
    script.push_str(BRIDGE_JS);
    if std::env::var_os("MESSENGER_DEBUG").is_some() {
        script.push_str(DEBUG_REPORTER_JS);
    }
    script
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![update_badge, debug_report])
        .on_menu_event(|app, event| match event.id().0.as_str() {
            "zoom_in" => adjust_zoom(app, Some(ZOOM_STEP)),
            "zoom_out" => adjust_zoom(app, Some(-ZOOM_STEP)),
            "zoom_reset" => adjust_zoom(app, None),
            _ => {}
        })
        .setup(|app| {
            let handle = app.handle().clone();
            let page_handle = app.handle().clone();
            let zoom_handle = app.handle().clone();

            app.manage(Zoom(Mutex::new(load_zoom(&handle))));

            // A custom menu replaces Tauri's default wholesale, so Edit has
            // to be re-declared here — without it Cmd+C/Cmd+V stop working
            // in the webview.
            let pkg = app.package_info();
            let about = AboutMetadata {
                name: Some(pkg.name.clone()),
                version: Some(pkg.version.to_string()),
                ..Default::default()
            };
            let menu = Menu::with_items(
                app,
                &[
                    &Submenu::with_items(
                        app,
                        pkg.name.clone(),
                        true,
                        &[
                            &PredefinedMenuItem::about(app, None, Some(about))?,
                            &PredefinedMenuItem::separator(app)?,
                            &PredefinedMenuItem::services(app, None)?,
                            &PredefinedMenuItem::separator(app)?,
                            &PredefinedMenuItem::hide(app, None)?,
                            &PredefinedMenuItem::hide_others(app, None)?,
                            &PredefinedMenuItem::separator(app)?,
                            &PredefinedMenuItem::quit(app, None)?,
                        ],
                    )?,
                    &Submenu::with_items(
                        app,
                        "Edit",
                        true,
                        &[
                            &PredefinedMenuItem::undo(app, None)?,
                            &PredefinedMenuItem::redo(app, None)?,
                            &PredefinedMenuItem::separator(app)?,
                            &PredefinedMenuItem::cut(app, None)?,
                            &PredefinedMenuItem::copy(app, None)?,
                            &PredefinedMenuItem::paste(app, None)?,
                            &PredefinedMenuItem::select_all(app, None)?,
                        ],
                    )?,
                    &Submenu::with_items(
                        app,
                        "View",
                        true,
                        &[
                            &MenuItem::with_id(
                                app,
                                "zoom_in",
                                "Zoom In",
                                true,
                                Some("CmdOrCtrl+="),
                            )?,
                            &MenuItem::with_id(
                                app,
                                "zoom_out",
                                "Zoom Out",
                                true,
                                Some("CmdOrCtrl+-"),
                            )?,
                            &MenuItem::with_id(
                                app,
                                "zoom_reset",
                                "Actual Size",
                                true,
                                Some("CmdOrCtrl+0"),
                            )?,
                            &PredefinedMenuItem::separator(app)?,
                            &PredefinedMenuItem::fullscreen(app, None)?,
                        ],
                    )?,
                    &Submenu::with_items(
                        app,
                        "Window",
                        true,
                        &[
                            &PredefinedMenuItem::minimize(app, None)?,
                            &PredefinedMenuItem::separator(app)?,
                            &PredefinedMenuItem::close_window(app, None)?,
                        ],
                    )?,
                ],
            )?;
            app.set_menu(menu)?;

            WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                .title("Messenger")
                .inner_size(1200.0, 800.0)
                .min_inner_size(400.0, 500.0)
                .user_agent(
                    &std::env::var("MESSENGER_UA").unwrap_or_else(|_| USER_AGENT.to_string()),
                )
                // Runs before the page is parsed, so styles land without the
                // flash of unstyled Messenger a post-load inject would give.
                .initialization_script(initialization_script())
                // No on_navigation guard on purpose. wry calls that hook for
                // *every* navigation, including subframes — it does not check
                // targetFrame — so cancelling off-site URLs there also kills
                // third-party iframes. Facebook's login embeds exactly that
                // (a captcha frame), and blocking it made logging in
                // impossible. Link handling lives in the two hooks below,
                // both of which only ever see the main frame.
                .on_new_window(move |url, _features: NewWindowFeatures| {
                    if std::env::var_os("MESSENGER_DEBUG").is_some() {
                        eprintln!("[new-window] {url} internal={}", is_internal(&url));
                    }
                    // Messenger opens links from messages with target="_blank",
                    // so this is where a clicked link actually lands.
                    let target = unwrap_link_shim(&url).unwrap_or(url);
                    if is_internal(&target) {
                        // Left in-app deliberately: Messenger also uses
                        // window.open for its own popups, calls included.
                        return NewWindowResponse::Allow;
                    }
                    let _ = handle.opener().open_url(target.as_str(), None::<&str>);
                    NewWindowResponse::Deny
                })
                .on_page_load(move |webview, payload| {
                    if payload.event() != PageLoadEvent::Finished {
                        return;
                    }
                    let url = payload.url();

                    // Run with MESSENGER_DEBUG=1 to trace main-frame loads.
                    // didFinishNavigation is main-frame only, so this is a
                    // clean view of where the window actually went.
                    if std::env::var_os("MESSENGER_DEBUG").is_some() {
                        eprintln!("[nav] {url}");
                    }

                    // The window opens on the local spinner so there is
                    // something on screen immediately; as soon as that has
                    // painted, swap it for Messenger.
                    if is_local_app(url) {
                        if let Ok(target) = start_url().parse::<Url>() {
                            if let Err(e) = webview.clone().navigate(target) {
                                eprintln!("failed to open Messenger: {e}");
                            }
                        }
                        return;
                    }

                    // WKWebView drops the zoom factor on navigation, so put it
                    // back every time a page settles.
                    if let Some(state) = zoom_handle.try_state::<Zoom>() {
                        if let Ok(zoom) = state.0.lock() {
                            if (*zoom - 1.0).abs() > f64::EPSILON {
                                let _ = webview.set_zoom(*zoom);
                            }
                        }
                    }

                    // didFinishNavigation fires for the main frame only, so
                    // reaching here means a foreign page actually took over the
                    // window (a link without target="_blank", or a JS redirect).
                    // Hand it to the browser and step back, so the window can
                    // never strand you off-site with no way back.
                    let target = unwrap_link_shim(url).unwrap_or_else(|| url.clone());
                    if !is_internal(&target) {
                        let _ = page_handle.opener().open_url(target.as_str(), None::<&str>);
                        let _ = webview.eval(
                            "if (history.length > 1) { history.back(); } \
                             else { location.replace('https://www.messenger.com'); }",
                        );
                    }
                })
                .build()?;

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Messenger");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(s: &str) -> Url {
        s.parse().expect("valid url")
    }

    #[test]
    fn messenger_and_login_flow_stay_in_app() {
        assert!(is_internal(&url("https://www.messenger.com/t/12345")));
        assert!(is_internal(&url("https://messenger.com/")));
        // Login, 2FA and checkpoint bounce through facebook.com.
        assert!(is_internal(&url("https://www.facebook.com/login.php")));
        assert!(is_internal(&url("https://www.facebook.com/checkpoint/")));
    }

    #[test]
    fn recaptcha_stays_in_app() {
        // The bug this guards: these were being opened in the default
        // browser, stranding the in-app login.
        assert!(is_internal(&url(
            "https://www.google.com/recaptcha/api2/anchor?k=abc"
        )));
        assert!(is_internal(&url(
            "https://www.google.com/recaptcha/api2/bframe?hl=en"
        )));
        assert!(is_internal(&url(
            "https://www.gstatic.com/recaptcha/releases/x/recaptcha__en.js"
        )));
        assert!(is_internal(&url(
            "https://www.recaptcha.net/recaptcha/api.js"
        )));
    }

    #[test]
    fn ordinary_google_links_still_open_externally() {
        assert!(!is_internal(&url("https://www.google.com/search?q=cats")));
        assert!(!is_internal(&url("https://maps.google.com/")));
        assert!(!is_internal(&url("https://mail.google.com/mail/u/0")));
    }

    #[test]
    fn two_step_verification_captcha_stays_in_app() {
        // The exact URL that was bouncing out to Chrome and blocking login.
        assert!(is_internal(&url(
            "https://www.fbsbx.com/captcha/recaptcha/iframe/?referer=https%3A%2F%2Fwww.facebook.com"
        )));
        assert!(is_internal(&url("https://fbsbx.com/captcha/")));
        assert!(is_internal(&url(
            "https://connect.facebook.net/en_US/sdk.js"
        )));
    }

    #[test]
    fn chat_links_unwrap_to_the_real_destination() {
        // Exactly the shape Messenger renders for a link in a message.
        let shim = url(
            "https://l.facebook.com/l.php?u=https%3A%2F%2Fcarsandbids.com%2Fauctions%2F3qbLmvj1&h=AT1",
        );
        let real = unwrap_link_shim(&shim).expect("unwraps");
        assert_eq!(real.as_str(), "https://carsandbids.com/auctions/3qbLmvj1");
        // The shim itself looks internal; the destination must not.
        assert!(is_internal(&shim));
        assert!(!is_internal(&real));
    }

    #[test]
    fn link_shim_variants_and_non_shims() {
        assert!(unwrap_link_shim(&url(
            "https://lm.facebook.com/l.php?u=https%3A%2F%2Fexample.com%2F"
        ))
        .is_some());
        // Not the shim path.
        assert!(unwrap_link_shim(&url(
            "https://www.facebook.com/l.php?u=https%3A%2F%2Fexample.com"
        ))
        .is_none());
        // No destination parameter.
        assert!(unwrap_link_shim(&url("https://l.facebook.com/l.php?h=AT1")).is_none());
        // Refuse to hand a non-http scheme to the OS.
        assert!(
            unwrap_link_shim(&url("https://l.facebook.com/l.php?u=javascript%3Aalert(1)"))
                .is_none()
        );
        // A genuine Messenger page is untouched.
        assert!(unwrap_link_shim(&url("https://www.facebook.com/messages")).is_none());
    }

    #[test]
    fn off_site_links_open_externally() {
        assert!(!is_internal(&url("https://example.com/")));
        assert!(!is_internal(&url("https://news.ycombinator.com/")));
    }

    #[test]
    fn lookalike_domains_are_not_internal() {
        assert!(!is_internal(&url("https://evil-facebook.com/")));
        assert!(!is_internal(&url("https://facebook.com.attacker.net/")));
        assert!(!is_internal(&url("https://notgoogle.com/recaptcha/api.js")));
        assert!(!is_internal(&url("https://messenger.com.evil.co/")));
        assert!(!is_internal(&url("https://fbsbx.com.evil.co/captcha/")));
        assert!(!is_internal(&url("https://notfbsbx.com/captcha/")));
    }

    #[test]
    fn local_app_origin_works_in_dev_and_bundled() {
        assert!(is_local_app(&url("tauri://localhost/index.html")));
        assert!(is_local_app(&url("http://127.0.0.1:1430/")));
        assert!(is_local_app(&url("http://tauri.localhost/")));
        assert!(!is_local_app(&url("https://www.messenger.com/")));
    }
}
