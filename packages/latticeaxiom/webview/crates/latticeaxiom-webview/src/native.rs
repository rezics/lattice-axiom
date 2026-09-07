use crate::{AssetBundle, BridgeError, Inbox};
use raw_window_handle::HasWindowHandle;
use std::{borrow::Cow, cell::RefCell, rc::Rc};
use wry::{
    NewWindowResponse, PermissionResponse, Rect, WebContext, WebView, WebViewBuilder,
    WebViewBuilderExtWindows, WebViewExtWindows,
    dpi::{PhysicalPosition, PhysicalSize},
    http::{HeaderValue, Response, StatusCode, Uri},
};

const LOCAL_URL: &str = "lattice://localhost/index.html";
const CSP: &str = "default-src 'none'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self'; connect-src 'self'; base-uri 'none'; frame-src 'none'; object-src 'none'; form-action 'none'; frame-ancestors 'none'";

pub(crate) struct NativeView(WebView);

impl NativeView {
    pub(crate) fn attach(
        window: &impl HasWindowHandle,
        assets: AssetBundle,
        width: u32,
        height: u32,
        inbox: Rc<RefCell<Inbox>>,
    ) -> Result<Self, BridgeError> {
        let profile = if let Some(path) = std::env::var_os("LATTICEAXIOM_WEBVIEW_DATA_DIR") {
            std::path::PathBuf::from(path)
        } else {
            let local = std::env::var_os("LOCALAPPDATA").ok_or_else(|| {
                BridgeError::Native("LOCALAPPDATA is unavailable for the WebView2 profile".into())
            })?;
            let executable = std::env::current_exe()?;
            let product = executable.file_stem().unwrap_or_default();
            std::path::PathBuf::from(local)
                .join("LatticeAxiom")
                .join("WebView2")
                .join(product)
        };
        std::fs::create_dir_all(&profile)?;
        let mut context = WebContext::new(Some(profile));
        let view = WebViewBuilder::new_with_web_context(&mut context)
            .with_transparent(true)
            .with_focused(false)
            .with_bounds(bounds(width, height))
            .with_https_scheme(true)
            .with_devtools(cfg!(debug_assertions))
            .with_hotkeys_zoom(false)
            .with_general_autofill_enabled(false)
            .with_navigation_handler(|url| url.parse::<Uri>().is_ok_and(|uri| is_local(&uri)))
            .with_new_window_req_handler(|_, _| NewWindowResponse::Deny)
            .with_download_started_handler(|_, _| false)
            .with_permission_handler(|_| PermissionResponse::Deny)
            .with_custom_protocol("lattice".into(), move |_, request| {
                let mut response = Response::new(Cow::Owned(Vec::<u8>::new()));
                *response.status_mut() = StatusCode::NOT_FOUND;
                if is_local(request.uri())
                    && request.method() == "GET"
                    && let Some((bytes, content_type)) = assets.get(request.uri().path())
                {
                    *response.body_mut() = Cow::Owned(bytes.to_vec());
                    *response.status_mut() = StatusCode::OK;
                    response
                        .headers_mut()
                        .insert("content-type", HeaderValue::from_static(content_type));
                }
                response
                    .headers_mut()
                    .insert("content-security-policy", HeaderValue::from_static(CSP));
                response.headers_mut().insert(
                    "x-content-type-options",
                    HeaderValue::from_static("nosniff"),
                );
                response
                    .headers_mut()
                    .insert("cache-control", HeaderValue::from_static("no-cache"));
                response
            })
            .with_ipc_handler(move |request| {
                if is_local(request.uri()) {
                    inbox.borrow_mut().push(request.into_body());
                }
            })
            .with_url(LOCAL_URL)
            .build_as_child(window)
            .map_err(native_error)?;
        Ok(Self(view))
    }

    pub(crate) fn evaluate(&self, javascript: &str) -> Result<(), BridgeError> {
        self.0.evaluate_script(javascript).map_err(native_error)
    }

    pub(crate) fn resize(&self, width: u32, height: u32) -> Result<(), BridgeError> {
        self.0
            .set_bounds(bounds(width, height))
            .map_err(native_error)
    }

    // This narrowly scoped native call controls the Wry-owned child HWND. The
    // rest of this package and all product code retain the unsafe-code deny lint.
    #[allow(unsafe_code)]
    pub(crate) fn set_interactive(&self, interactive: bool) -> Result<(), BridgeError> {
        // SAFETY: hwnd() is the live child owned by self.0; this !Send value is
        // only used on its creating window thread and the HWND is not retained.
        // EnableWindow returns previous enabled state, not success/failure.
        unsafe {
            let _ = windows::Win32::UI::Input::KeyboardAndMouse::EnableWindow(
                self.0.hwnd(),
                interactive,
            );
        }
        if interactive {
            self.0.focus().map_err(native_error)
        } else {
            self.0.focus_parent().map_err(native_error)
        }
    }

    pub(crate) fn set_visible(&self, visible: bool) -> Result<(), BridgeError> {
        self.0.set_visible(visible).map_err(native_error)
    }

    #[allow(unsafe_code)]
    pub(crate) fn is_application_focused(&self) -> bool {
        use windows::Win32::UI::WindowsAndMessaging::{GA_ROOT, GetAncestor, GetForegroundWindow};
        // SAFETY: hwnd() belongs to this live main-thread WebView. Both calls
        // query OS-owned window identity without retaining or dereferencing it.
        unsafe {
            let root = GetAncestor(self.0.hwnd(), GA_ROOT);
            !root.is_invalid() && root == GetForegroundWindow()
        }
    }
}

fn bounds(width: u32, height: u32) -> Rect {
    Rect {
        position: PhysicalPosition::new(0, 0).into(),
        size: PhysicalSize::new(width.max(1), height.max(1)).into(),
    }
}

// `Result::map_err` consumes the upstream error at this conversion boundary.
#[allow(clippy::needless_pass_by_value)]
fn native_error(error: wry::Error) -> BridgeError {
    BridgeError::Native(error.to_string())
}

fn is_local(uri: &Uri) -> bool {
    matches!(
        (
            uri.scheme_str(),
            uri.authority().map(wry::http::uri::Authority::as_str)
        ),
        (Some("lattice"), Some("localhost")) | (Some("https"), Some("lattice.localhost"))
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origin_filter_is_exact_and_rejects_confusable_hosts()
    -> Result<(), Box<dyn std::error::Error>> {
        for uri in [
            "lattice://localhost/index.html",
            "https://lattice.localhost/assets/app.js",
        ] {
            assert!(is_local(&uri.parse()?));
        }
        for uri in [
            "http://lattice.localhost/",
            "https://lattice.localhost.evil.test/",
            "https://lattice.localhost:443/",
            "https://user@lattice.localhost/",
            "file:///secret",
            "https://example.org/",
        ] {
            assert!(!uri.parse::<Uri>().is_ok_and(|uri| is_local(&uri)));
        }
        Ok(())
    }
}
