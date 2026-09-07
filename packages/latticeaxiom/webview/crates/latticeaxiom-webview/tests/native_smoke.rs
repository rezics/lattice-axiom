//! Opt-in real `WebView2` protocol/IPC acceptance without a visible game window.
#![cfg(target_os = "windows")]

use latticeaxiom_webview::{AssetBundle, WebViewHost};
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
            let view = WebViewHost::attach(&window, self.assets.clone(), 480, 320)?;
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
    Ok(())
}
