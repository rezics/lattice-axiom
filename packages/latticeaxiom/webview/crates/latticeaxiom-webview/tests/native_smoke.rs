//! Opt-in real `WebView2` protocol/IPC acceptance without a visible game window.
#![cfg(target_os = "windows")]

use latticeaxiom_webview::{AssetBundle, WebViewHost};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::time::{Duration, Instant};
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, EventLoop},
    platform::{pump_events::EventLoopExtPumpEvents, windows::EventLoopBuilderExtWindows},
    window::{Window, WindowId},
};

struct SmokeApp {
    // Declaration order ensures the native child is destroyed before its parent.
    view: Option<WebViewHost>,
    window: Option<Window>,
    assets: AssetBundle,
    failure: Option<String>,
    original_clip_children: bool,
}

impl ApplicationHandler for SmokeApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() || self.failure.is_some() {
            return;
        }
        let result = (|| -> Result<(), Box<dyn std::error::Error>> {
            let window = event_loop.create_window(
                Window::default_attributes()
                    .with_visible(false)
                    .with_active(false)
                    .with_title("Lattice WebView protocol smoke"),
            )?;
            self.original_clip_children = check_clipping(&window, false)?;
            let view = WebViewHost::attach(&window, self.assets.clone(), 480, 320)?;
            assert!(
                !check_clipping(&window, false)?,
                "transparent host must retain the GPU parent surface"
            );
            assert!(
                !check_clipping(&window, true)?,
                "framework style changes must not reintroduce child clipping"
            );
            self.window = Some(window);
            self.view = Some(view);
            Ok(())
        })();
        if let Err(error) = result {
            self.failure = Some(error.to_string());
            event_loop.exit();
        }
    }

    fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: WindowEvent) {}
}

#[test]
#[ignore = "Requires installed WebView2 and a Windows desktop session"]
fn local_assets_and_bidirectional_ipc_reach_the_native_runtime()
-> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    std::fs::write(
        root.path().join("index.html"),
        "<!doctype html><html><head><script src='/app.js' defer></script></head><body>Smoke</body></html>",
    )?;
    std::fs::write(
        root.path().join("app.js"),
        "window.__latticeReceive = envelope => window.ipc.postMessage(JSON.stringify({type:'ack',token:envelope.token})); window.ipc.postMessage(JSON.stringify({type:'ready'}));",
    )?;
    let mut app = SmokeApp {
        view: None,
        window: None,
        assets: AssetBundle::from_directory(root.path())?,
        failure: None,
        original_clip_children: false,
    };
    let mut events = EventLoop::builder().with_any_thread(true).build()?;
    let started = Instant::now();
    let mut acknowledged = false;
    while started.elapsed() < Duration::from_secs(15) && !acknowledged && app.failure.is_none() {
        events.pump_app_events(Some(Duration::from_millis(10)), &mut app);
        if let Some(view) = &mut app.view {
            for message in view.drain_commands::<serde_json::Value>(16) {
                let message = message?;
                if message["type"] == "ready" {
                    assert!(
                        !view.is_application_focused(),
                        "hidden smoke must not take foreground focus"
                    );
                    view.resize(640, 360)?;
                    view.set_interactive(false)?;
                    view.send_state(&serde_json::json!({"token": "native-protocol-roundtrip"}))?;
                } else if message["type"] == "ack" {
                    assert_eq!(message["token"], "native-protocol-roundtrip");
                    assert_eq!(view.dropped_commands(), 0);
                    acknowledged = true;
                }
            }
        }
    }
    assert!(
        app.failure.is_none(),
        "native initialization: {:?}",
        app.failure
    );
    assert!(
        acknowledged,
        "WebView2 did not complete the local protocol/IPC handshake within 15 seconds"
    );
    drop(app.view.take());
    let window = app.window.as_ref().ok_or("missing smoke window")?;
    assert_eq!(
        check_clipping(window, false)?,
        app.original_clip_children,
        "teardown must restore the original parent clipping bit"
    );
    Ok(())
}

#[allow(unsafe_code)]
fn check_clipping(
    window: &Window,
    request_clipping: bool,
) -> Result<bool, Box<dyn std::error::Error>> {
    use windows::Win32::{
        Foundation::HWND,
        UI::WindowsAndMessaging::{
            GWL_STYLE, GetWindowLongPtrW, SetWindowLongPtrW, WS_CLIPCHILDREN,
        },
    };
    let RawWindowHandle::Win32(handle) = window.window_handle()?.as_raw() else {
        return Err("not a Windows smoke window".into());
    };
    let hwnd = HWND(handle.hwnd.get() as *mut std::ffi::c_void);
    let clipping = isize::try_from(WS_CLIPCHILDREN.0)?;
    // SAFETY: this test owns a live hidden window on its creating thread. Only
    // its own clipping bit is modified, simulating winit fullscreen style updates.
    unsafe {
        if request_clipping {
            let style = GetWindowLongPtrW(hwnd, GWL_STYLE);
            SetWindowLongPtrW(hwnd, GWL_STYLE, style | clipping);
        }
        Ok(GetWindowLongPtrW(hwnd, GWL_STYLE) & clipping != 0)
    }
}
