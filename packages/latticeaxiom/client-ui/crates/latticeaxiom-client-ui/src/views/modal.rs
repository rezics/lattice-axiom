//! Pause, Save & Quit, binding capture, and recovery modals.

use std::collections::BTreeSet;

use crate::key_capture::{KeyCapturePhase, KeyCaptureSession};
use crate::router::SaveQuitProjection;
use crate::semantic::{SemanticKey, SemanticNode, SemanticRole, SemanticState};
use crate::views::keys::surface_key;
use crate::widgets::{ButtonWidget, ModalWidget, ToastWidget};

/// Pause modal. Settings is entered only from here in the game process.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PauseViewV1 {
    /// Whether a world writer is open. Save & Quit stays enabled only with a writer path.
    pub writer_open: bool,
}

impl PauseViewV1 {
    /// Projects Resume, Settings, and Save & Quit. Widgets only emit commands.
    #[must_use]
    pub fn semantic_node(&self, focused: Option<&SemanticKey>) -> SemanticNode {
        let resume = ButtonWidget::new(
            surface_key("modal/pause/resume"),
            "Resume",
            Some("Return to play without writing".to_owned()),
            true,
        )
        .semantic_node(focused == Some(&surface_key("modal/pause/resume")));
        let settings = ButtonWidget::new(
            surface_key("modal/pause/settings"),
            "Settings",
            Some("Open the typed settings surface".to_owned()),
            true,
        )
        .semantic_node(focused == Some(&surface_key("modal/pause/settings")));
        let save_quit = ButtonWidget::new(
            surface_key("modal/pause/save-quit"),
            "Save and quit",
            Some("Confirm durable Save and Quit. Written is not Durable.".to_owned()),
            true,
        )
        .semantic_node(focused == Some(&surface_key("modal/pause/save-quit")));
        SemanticNode {
            key: surface_key("modal/pause"),
            role: SemanticRole::Dialog,
            name: "Paused".to_owned(),
            value: None,
            description: Some(if self.writer_open {
                "World writer is open. Save and Quit uses the durability barrier.".to_owned()
            } else {
                "Pause overlay. Resume, Settings, or Save and Quit.".to_owned()
            }),
            state: SemanticState {
                focusable: false,
                focused: false,
                disabled: false,
                expanded: Some(true),
                busy: false,
            },
            actions: [crate::SemanticAction::Cancel, crate::SemanticAction::Back]
                .into_iter()
                .collect(),
            children: vec![resume, settings, save_quit],
        }
    }
}

/// Confirm Save & Quit before entering [`crate::GameTransitionV1::SavingAndExiting`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConfirmSaveQuitViewV1;

impl ConfirmSaveQuitViewV1 {
    /// Projects confirm/cancel. Confirm closes every interactive surface.
    #[must_use]
    pub fn semantic_node(focused: Option<&SemanticKey>) -> SemanticNode {
        ModalWidget {
            key: surface_key("modal/confirm-save-quit"),
            name: "Save and quit".to_owned(),
            description: Some(
                "Confirm durable exit. The child may return to the shell only after Durable."
                    .to_owned(),
            ),
            confirm: ButtonWidget::new(
                surface_key("modal/confirm-save-quit/confirm"),
                "Save and quit",
                Some("Enter the durability barrier".to_owned()),
                true,
            ),
            cancel: ButtonWidget::new(
                surface_key("modal/confirm-save-quit/cancel"),
                "Cancel",
                Some("Return to pause".to_owned()),
                true,
            ),
        }
        .semantic_node(focused)
    }
}

/// Binding-capture child of Settings. Physical input stays in the input adapter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BindingCaptureViewV1<'a> {
    /// Live capture session bound to the current epoch.
    pub session: &'a KeyCaptureSession,
}

impl BindingCaptureViewV1<'_> {
    /// Projects capture, conflict confirmation, or accepted/cancelled status.
    #[must_use]
    pub fn semantic_node(&self, focused: Option<&SemanticKey>) -> SemanticNode {
        match self.session.phase() {
            KeyCapturePhase::Capturing => SemanticNode {
                key: surface_key("modal/settings/capture"),
                role: SemanticRole::Dialog,
                name: "Capture binding".to_owned(),
                value: Some(self.session.row().as_str().to_owned()),
                description: Some(
                    "Press a binding. Esc cancels. Capture is a Settings child, not a new root."
                        .to_owned(),
                ),
                state: SemanticState {
                    focusable: false,
                    focused: false,
                    disabled: false,
                    expanded: Some(true),
                    busy: true,
                },
                actions: [crate::SemanticAction::Cancel, crate::SemanticAction::Back]
                    .into_iter()
                    .collect(),
                children: vec![
                    ButtonWidget::new(
                        surface_key("modal/settings/capture/clear"),
                        "Clear binding",
                        Some("Explicit unbind".to_owned()),
                        true,
                    )
                    .semantic_node(focused == Some(&surface_key("modal/settings/capture/clear"))),
                    ButtonWidget::new(
                        surface_key("modal/settings/capture/cancel"),
                        "Cancel",
                        Some("Return to the Controls row".to_owned()),
                        true,
                    )
                    .semantic_node(focused == Some(&surface_key("modal/settings/capture/cancel"))),
                ],
            },
            KeyCapturePhase::ConfirmingConflict => ModalWidget {
                key: surface_key("modal/settings/capture"),
                name: "Replace conflicting binding".to_owned(),
                description: Some(
                    "Same-context conflict. Confirm replaces the occupying action or cancel."
                        .to_owned(),
                ),
                confirm: ButtonWidget::new(
                    surface_key("modal/settings/capture/confirm"),
                    "Replace",
                    Some("Accept the candidate and replace the occupying action".to_owned()),
                    true,
                ),
                cancel: ButtonWidget::new(
                    surface_key("modal/settings/capture/cancel"),
                    "Cancel",
                    Some("Keep the occupying action".to_owned()),
                    true,
                ),
            }
            .semantic_node(focused),
            KeyCapturePhase::Accepted
            | KeyCapturePhase::Cancelled
            | KeyCapturePhase::Cleared
            | KeyCapturePhase::Idle => ToastWidget {
                key: surface_key("modal/settings/capture"),
                name: format!("Capture {:?}", self.session.phase()),
                description: Some(
                    "Capture ended; apply still belongs to the settings transaction".to_owned(),
                ),
            }
            .semantic_node(),
        }
    }
}

/// Save & Quit transition. Interactive surfaces cannot reopen.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SavingViewV1 {
    /// Durability projection. Only Durable may return to the shell.
    pub projection: SaveQuitProjection,
}

impl SavingViewV1 {
    /// Projects a non-interactive status. Written is not Durable.
    #[must_use]
    pub fn semantic_node(self) -> SemanticNode {
        let (name, value, description) = match self.projection {
            SaveQuitProjection::Saving => (
                "Saving",
                "saving",
                "Durability barrier is running. Interactive surfaces are closed.",
            ),
            SaveQuitProjection::WrittenNotDurable => (
                "Written",
                "written-not-durable",
                "Writer reported Written. This is not a completed save.",
            ),
            SaveQuitProjection::Durable => (
                "Saved",
                "durable",
                "Durability barrier reported Durable. The child may exit to the shell.",
            ),
            SaveQuitProjection::Timeout => (
                "Save timed out",
                "timeout",
                "Shutdown timed out. Latest recoverable state is retained.",
            ),
            SaveQuitProjection::StorageFailure => (
                "Save failed",
                "storage-failure",
                "Storage failed. Latest recoverable state is retained.",
            ),
        };
        SemanticNode {
            key: surface_key("transition/saving"),
            role: SemanticRole::Status,
            name: name.to_owned(),
            value: Some(value.to_owned()),
            description: Some(description.to_owned()),
            state: SemanticState {
                focusable: false,
                focused: false,
                disabled: false,
                expanded: None,
                busy: matches!(
                    self.projection,
                    SaveQuitProjection::Saving | SaveQuitProjection::WrittenNotDurable
                ),
            },
            actions: BTreeSet::new(),
            children: Vec::new(),
        }
    }
}

/// Bounded fatal recovery. Supervisor starts the recovery shell after exit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FatalRecoveryViewV1 {
    /// Bounded diagnostic text.
    pub diagnostic: String,
}

impl FatalRecoveryViewV1 {
    /// Projects a non-interactive recovery status.
    #[must_use]
    pub fn semantic_node(&self) -> SemanticNode {
        SemanticNode {
            key: surface_key("transition/recovery"),
            role: SemanticRole::Alert,
            name: "Fatal recovery".to_owned(),
            value: Some(self.diagnostic.clone()),
            description: Some(
                "Bounded crash marker. Supervisor starts the recovery shell after exit.".to_owned(),
            ),
            state: SemanticState::default(),
            actions: BTreeSet::new(),
            children: Vec::new(),
        }
    }
}
