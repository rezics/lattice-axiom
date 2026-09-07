//! Main-thread `WebView` embedding with offline assets and bounded JSON transport.
//!
//! The product owns message schemas and domain authorization. This crate never
//! evaluates commands or exposes filesystem operations to JavaScript. Keep the
//! host as a Bevy non-send resource and drop it before its parent window.

mod assets;
mod endpoints;
#[cfg(target_os = "windows")]
mod native;

pub use assets::AssetBundle;
pub use endpoints::{
    EndpointHandler, EndpointMetadata, EndpointRegistrationError, EndpointRegistry,
};
pub use raw_window_handle::HasWindowHandle;

use serde::{Serialize, de::DeserializeOwned};
use std::{cell::RefCell, collections::VecDeque, rc::Rc};

/// Maximum queued incoming requests; producers never block the window thread.
pub const MAX_PENDING_COMMANDS: usize = 256;
/// Maximum UTF-8 bytes in one incoming request.
pub const MAX_COMMAND_BYTES: usize = 64 * 1024;
/// Maximum serialized state size for one JavaScript delivery.
pub const MAX_STATE_BYTES: usize = 2 * 1024 * 1024;

/// Native transport, asset validation, or JSON failure.
#[derive(Debug, thiserror::Error)]
pub enum BridgeError {
    /// A filesystem operation failed while preparing a fixed asset bundle.
    #[error("Web UI assets: {0}")]
    Io(#[from] std::io::Error),
    /// The supplied asset bundle failed validation.
    #[error("Web UI assets: {0}")]
    Assets(String),
    /// The installed native runtime failed.
    #[error("WebView2 runtime: {0}")]
    Native(String),
    /// A message could not be decoded into the product-owned request type.
    #[error("Web UI JSON: {0}")]
    Json(#[from] serde_json::Error),
    /// A message exceeds the configured transport budget.
    #[error("Web UI message exceeds {0} bytes")]
    MessageTooLarge(usize),
    /// This first implementation deliberately supports Windows only.
    #[error("Native Web UI currently requires Windows and WebView2 Runtime")]
    UnsupportedPlatform,
}

#[derive(Debug, Default)]
struct Inbox {
    messages: VecDeque<String>,
    dropped: u64,
}

impl Inbox {
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    fn push(&mut self, message: String) {
        if message.len() > MAX_COMMAND_BYTES || self.messages.len() >= MAX_PENDING_COMMANDS {
            self.dropped = self.dropped.saturating_add(1);
        } else {
            self.messages.push_back(message);
        }
    }
}

/// One transparent child `WebView` attached to an existing native game window.
///
/// This is deliberately `!Send`/`!Sync`. Construct, resize, send, focus and drop
/// it on the event-loop thread. The parent must outlive this value. Windows'
/// `WebView2` initialization establishes its STA on that thread through Wry.
pub struct WebViewHost {
    #[cfg(target_os = "windows")]
    native: native::NativeView,
    inbox: Rc<RefCell<Inbox>>,
}

impl std::fmt::Debug for WebViewHost {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("WebViewHost")
            .field("inbox", &self.inbox)
            .finish_non_exhaustive()
    }
}

impl WebViewHost {
    /// Attach a transparent child filling the physical pixel dimensions.
    ///
    /// The bundle is immutable after attachment. Only its local origin may
    /// navigate or send messages; external windows/downloads are denied.
    ///
    /// # Errors
    /// Returns runtime/profile failures, or an unsupported-platform error.
    pub fn attach(
        window: &impl HasWindowHandle,
        assets: AssetBundle,
        width: u32,
        height: u32,
    ) -> Result<Self, BridgeError> {
        #[cfg(target_os = "windows")]
        {
            let inbox = Rc::new(RefCell::new(Inbox::default()));
            let native =
                native::NativeView::attach(window, assets, width, height, Rc::clone(&inbox))?;
            Ok(Self { native, inbox })
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = (window, assets, width, height);
            Err(BridgeError::UnsupportedPlatform)
        }
    }

    /// Drain at most `limit` queued messages using the product's typed schema.
    /// Parsing occurs in the host's scheduled phase, never inside a COM callback.
    pub fn drain_commands<T: DeserializeOwned>(
        &mut self,
        limit: usize,
    ) -> Vec<Result<T, BridgeError>> {
        let mut inbox = self.inbox.borrow_mut();
        let count = limit.min(inbox.messages.len());
        inbox
            .messages
            .drain(..count)
            .map(|message| serde_json::from_str(&message).map_err(Into::into))
            .collect()
    }

    /// Total oversized or overflowing requests discarded since attachment.
    #[must_use]
    pub fn dropped_commands(&self) -> u64 {
        self.inbox.borrow().dropped
    }

    /// Whether this parent application owns the foreground native window.
    /// This remains true when keyboard focus moves into its web child.
    #[must_use]
    pub fn is_application_focused(&self) -> bool {
        #[cfg(target_os = "windows")]
        return self.native.is_application_focused();
        #[cfg(not(target_os = "windows"))]
        false
    }

    /// Deliver a product-owned envelope to `window.__latticeReceive`.
    ///
    /// The page must first install its receiver then post its `ready` request.
    /// The product should send its first snapshot in response to that handshake.
    ///
    /// # Errors
    /// Returns serialization, message-budget, or native runtime failures.
    pub fn send_state(&self, envelope: &impl Serialize) -> Result<(), BridgeError> {
        let json = serde_json::to_string(envelope)?;
        if json.len() > MAX_STATE_BYTES {
            return Err(BridgeError::MessageTooLarge(MAX_STATE_BYTES));
        }
        #[cfg(target_os = "windows")]
        return self
            .native
            .evaluate(&format!("window.__latticeReceive?.({json});"));
        #[cfg(not(target_os = "windows"))]
        Err(BridgeError::UnsupportedPlatform)
    }

    /// Resize in physical pixels after the parent resize/scale-factor event.
    ///
    /// # Errors
    /// Returns a native runtime failure if bounds cannot be applied.
    pub fn resize(&self, width: u32, height: u32) -> Result<(), BridgeError> {
        #[cfg(target_os = "windows")]
        return self.native.resize(width, height);
        #[cfg(not(target_os = "windows"))]
        {
            let _ = (width, height);
            Err(BridgeError::UnsupportedPlatform)
        }
    }

    /// Enable web interaction for menus, or yield native input to the game HUD.
    ///
    /// Windows disables the native child window in HUD mode: this is OS input
    /// routing, independent of HTML `pointer-events`. Interactive mode focuses
    /// the child; HUD mode focuses the parent. Call only when the mode changes,
    /// so an unfocused application is not repeatedly asked to acquire focus.
    ///
    /// # Errors
    /// Returns a native runtime failure if focus cannot be transferred.
    pub fn set_interactive(&self, interactive: bool) -> Result<(), BridgeError> {
        #[cfg(target_os = "windows")]
        return self.native.set_interactive(interactive);
        #[cfg(not(target_os = "windows"))]
        {
            let _ = interactive;
            Err(BridgeError::UnsupportedPlatform)
        }
    }

    /// Show or hide the complete web layer without destroying its state.
    ///
    /// # Errors
    /// Returns a native runtime failure if visibility cannot be changed.
    pub fn set_visible(&self, visible: bool) -> Result<(), BridgeError> {
        #[cfg(target_os = "windows")]
        return self.native.set_visible(visible);
        #[cfg(not(target_os = "windows"))]
        {
            let _ = visible;
            Err(BridgeError::UnsupportedPlatform)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ingress_is_bounded_and_preserves_order() {
        let mut inbox = Inbox::default();
        for value in 0..MAX_PENDING_COMMANDS + 5 {
            inbox.push(value.to_string());
        }
        assert_eq!(inbox.messages.len(), MAX_PENDING_COMMANDS);
        assert_eq!(inbox.dropped, 5);
        assert_eq!(inbox.messages.front().map(String::as_str), Some("0"));
        inbox.push("x".repeat(MAX_COMMAND_BYTES + 1));
        assert_eq!(inbox.dropped, 6);
    }
}
