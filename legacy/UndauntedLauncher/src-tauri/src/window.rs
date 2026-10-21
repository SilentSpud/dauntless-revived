//! The launcher's one window: frameless, sized to fit the screen, and locked to the app's own page
//! (rows L5 and L10).

use tauri::webview::{NewWindowResponse, PageLoadEvent};
use tauri::window::Color;
use tauri::{AppHandle, Emitter, Monitor, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent};
use url::Url;

pub const MAIN: &str = "main";

/// The page's ink (styles.css), so nothing flashes before it paints.
const INK: Color = Color(0x01, 0x09, 0x0d, 0xff);

/// Pushed to the page whenever the window is maximised or restored.
pub const WINDOW_STATE_EVENT: &str = "window-state";

/// Logical sizes: the window and its minimum, never larger than the primary screen's usable area
/// (the part the taskbar does not cover). 1366x768 laptops, or 1920x1080 at 150 % scaling, have less
/// than 760 px of height, and the big button at the bottom must stay on screen.
fn fit(work: Option<(f64, f64)>) -> ((f64, f64), (f64, f64)) {
    let (width, height) = work.unwrap_or((f64::INFINITY, f64::INFINITY));
    (
        (width.min(1280.0), height.min(760.0)),
        (width.min(1024.0), height.min(640.0)),
    )
}

fn work_area(monitor: &Monitor) -> (f64, f64) {
    let scale = monitor.scale_factor();
    let size = monitor.work_area().size;
    (f64::from(size.width) / scale, f64::from(size.height) / scale)
}

/// The app's own page, as each webview serves it, or the Vite dev server in development.
pub fn is_app_url(url: &Url, dev: Option<&Url>) -> bool {
    if let Some(dev) = dev {
        return url.origin() == dev.origin();
    }
    match url.scheme() {
        // WebKitGTK
        "tauri" => url.host_str() == Some("localhost"),
        // WebView2
        "http" | "https" => url.host_str() == Some("tauri.localhost"),
        _ => false,
    }
}

pub fn create(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    let ((width, height), (min_width, min_height)) = fit(app.primary_monitor()?.as_ref().map(work_area));
    let dev = if cfg!(dev) {
        app.config().build.dev_url.clone()
    } else {
        None
    };
    let window = WebviewWindowBuilder::new(app, MAIN, WebviewUrl::App("index.html".into()))
        .title("Dauntless Revived Launcher")
        .inner_size(width, height)
        .min_inner_size(min_width, min_height)
        .center()
        .decorations(false)
        .visible(false)
        .background_color(INK)
        .on_navigation(move |url| is_app_url(url, dev.as_ref()))
        .on_new_window(|_, _| NewWindowResponse::Deny)
        .on_page_load(|window, payload| {
            if payload.event() == PageLoadEvent::Finished {
                let _ = window.show();
            }
        })
        .build()?;

    let handle = window.clone();
    window.on_window_event(move |event| {
        if let WindowEvent::Resized(_) = event {
            let maximized = handle.is_maximized().unwrap_or(false);
            let _ = handle.emit_to(MAIN, WINDOW_STATE_EVENT, maximized);
        }
    });
    Ok(window)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(text: &str) -> Url {
        Url::parse(text).unwrap()
    }

    #[test]
    fn only_the_app_page_may_load() {
        assert!(is_app_url(&url("tauri://localhost/index.html"), None));
        assert!(is_app_url(&url("http://tauri.localhost/index.html#x"), None));
        assert!(is_app_url(&url("https://tauri.localhost/"), None));
        for other in [
            "https://example.com/",
            "http://localhost:5173/",
            "file:///C:/x.html",
            "tauri://evil/",
            "http://tauri.localhost.example.com/",
        ] {
            assert!(!is_app_url(&url(other), None), "{other}");
        }
        let dev = url("http://localhost:5173/");
        assert!(is_app_url(&url("http://localhost:5173/src/index.html"), Some(&dev)));
        assert!(!is_app_url(&url("http://localhost:5174/"), Some(&dev)));
        assert!(!is_app_url(&url("tauri://localhost/"), Some(&dev)));
    }

    #[test]
    fn the_window_fits_small_screens() {
        assert_eq!(fit(Some((1920.0, 1040.0))), ((1280.0, 760.0), (1024.0, 640.0)));
        assert_eq!(fit(Some((1366.0, 728.0))), ((1280.0, 728.0), (1024.0, 640.0)));
        assert_eq!(fit(Some((1280.0, 680.0))), ((1280.0, 680.0), (1024.0, 640.0)));
        assert_eq!(fit(None), ((1280.0, 760.0), (1024.0, 640.0)));
    }
}
