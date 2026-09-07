//! Preserve the parent GPU surface behind a transparent native child.
//!
//! Winit defaults to `WS_CLIPCHILDREN`, which removes the entire `WebView` rectangle
//! from the parent render surface instead of composing its transparent pixels.
//! The narrowly scoped native subclass keeps that bit cleared for this host's
//! lifetime, including fullscreen/style changes, and restores it after teardown.
#![allow(unsafe_code)]

use crate::BridgeError;
use windows::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, WPARAM},
    UI::{
        Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
        WindowsAndMessaging::{
            GA_ROOT, GWL_STYLE, GetAncestor, GetWindowLongPtrW, IsWindow, STYLESTRUCT,
            SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER,
            SetWindowLongPtrW, SetWindowPos, WM_NCDESTROY, WM_STYLECHANGING, WS_CLIPCHILDREN,
        },
    },
};

// Unique identity for this crate's parent-window subclass, not a heap pointer.
const SUBCLASS_ID: usize = 0x4c41_5756;

pub(crate) struct ParentCompositionGuard {
    parent: HWND,
    restore_clip_children: bool,
}

impl ParentCompositionGuard {
    pub(crate) fn attach(child: HWND) -> Result<Self, BridgeError> {
        // SAFETY: Wry supplies its live child HWND on the event-loop thread.
        // The parent outlives this guard under WebViewHost's documented contract.
        unsafe {
            let parent = GetAncestor(child, GA_ROOT);
            if parent.is_invalid() {
                return Err(BridgeError::Native(
                    "WebView parent window is unavailable".into(),
                ));
            }
            let style = GetWindowLongPtrW(parent, GWL_STYLE);
            if !SetWindowSubclass(parent, Some(parent_subclass), SUBCLASS_ID, 0).as_bool() {
                return Err(BridgeError::Native(
                    "cannot install WebView parent composition guard".into(),
                ));
            }
            let guard = Self {
                parent,
                restore_clip_children: style & clip_mask() != 0,
            };
            SetWindowLongPtrW(parent, GWL_STYLE, style & !clip_mask());
            SetWindowPos(
                parent,
                None,
                0,
                0,
                0,
                0,
                SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            )
            .map_err(|error| BridgeError::Native(error.to_string()))?;
            if GetWindowLongPtrW(parent, GWL_STYLE) & clip_mask() != 0 {
                return Err(BridgeError::Native(
                    "parent still clips the transparent WebView surface".into(),
                ));
            }
            Ok(guard)
        }
    }
}

impl Drop for ParentCompositionGuard {
    fn drop(&mut self) {
        // SAFETY: teardown stays on the creating thread. If the OS has already
        // destroyed the parent, there is no subclass or style left to restore.
        unsafe {
            if IsWindow(Some(self.parent)).as_bool() {
                let _ = RemoveWindowSubclass(self.parent, Some(parent_subclass), SUBCLASS_ID);
                if self.restore_clip_children {
                    let style = GetWindowLongPtrW(self.parent, GWL_STYLE);
                    SetWindowLongPtrW(self.parent, GWL_STYLE, style | clip_mask());
                    let _ = SetWindowPos(
                        self.parent,
                        None,
                        0,
                        0,
                        0,
                        0,
                        SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                    );
                }
            }
        }
    }
}

fn clip_mask() -> isize {
    // This Win32 style is positive and representable on all supported Windows targets.
    isize::try_from(WS_CLIPCHILDREN.0).unwrap_or(0x0200_0000)
}

unsafe extern "system" fn parent_subclass(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    subclass_id: usize,
    _: usize,
) -> LRESULT {
    // SAFETY: Windows invokes this callback on the owning thread. For this exact
    // message/index, lparam points to a writable STYLESTRUCT for the call's life.
    // No pointer or reference to that structure is retained after returning.
    unsafe {
        if message == WM_STYLECHANGING && wparam.0 == (-16_isize).cast_unsigned() {
            if let Some(style) = (lparam.0 as *mut STYLESTRUCT).as_mut() {
                style.styleNew &= !WS_CLIPCHILDREN.0;
            }
        } else if message == WM_NCDESTROY {
            let _ = RemoveWindowSubclass(window, Some(parent_subclass), subclass_id);
        }
        DefSubclassProc(window, message, wparam, lparam)
    }
}
