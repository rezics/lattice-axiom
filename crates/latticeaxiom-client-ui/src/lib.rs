//! Shared headless-first client UI foundation.
//!
//! This crate owns theme tokens, typed shell/game surface routing, unique
//! focus ownership, button/list/modal semantics, semantic-tree projection, and
//! AccessKit helpers. It does not create a Bevy `App`, a second UI root, or a
//! settings persistence backend. Headless profiles may omit this crate without
//! changing authoritative registration.

mod a11y;
mod focus;
mod key_capture;
mod layout;
mod projection;
mod router;
mod semantic;
mod session;
mod surface;
mod theme;
mod views;
mod widgets;

pub use a11y::{A11yError, AccessKitNode, AccessKitRole, check_accesskit_tree};
pub use focus::{
    FocusController, FocusCursorProjection, FocusError, FocusOwner, LinearFocusDirection,
};
pub use key_capture::{
    BindingConflict, InputBindingCandidateV1, KeyCaptureError, KeyCapturePhase, KeyCaptureSession,
};
pub use layout::{
    LayoutContractError, LayoutEvidence, LogicalViewport, ReachableControl, verify_scroll_layout,
};
pub use projection::{
    ProjectionDiff, ProjectionError, SemanticSnapshot, accept_async_epoch, diff_snapshots,
};
pub use router::{
    GameApplyReceipt, GameModalV1, GameOverlayV1, GameSurfaceRoute, GameSurfaceRouter,
    GameTransitionV1, SaveQuitProjection, ShellApplyReceipt, ShellRouteV1, ShellSurfaceRouter,
    SurfaceCommandV1, SurfaceRouterError,
};
pub use semantic::{
    ClientSurfaceActionV1, ImeTextState, InputSource, MAX_SEMANTIC_KEY_BYTES, SemanticAction,
    SemanticCommand, SemanticCommandError, SemanticKey, SemanticKeyError, SemanticNode,
    SemanticRole, SemanticState, validate_semantic_command,
};
pub use session::{
    GamePresentationV1, GameSurfaceSession, ShellPresentationV1, ShellSurfaceSession,
    SurfaceInjectEffect, SurfaceSessionError, surface_command,
};
pub use surface::{
    ActiveInputContextStack, CapturePolicy, ContextPolicy, CursorPolicy, GameplayAdmission,
    InputContextKind, MAX_ROUTE_DEPTH, ROUTE_VOCABULARY_MAJOR, SurfaceEpoch,
};
pub use theme::{
    ChromeMetrics, CjkFallbackFontId, FocusRing, LinearRgba, SpacingScale, ThemePalette,
    ThemeTokenError, ThemeTokens, TypeScale, UiScale,
};
pub use views::{
    BindingCaptureViewV1, ConfirmSaveQuitViewV1, DiagnosticsAboutViewV1, FatalRecoveryViewV1,
    HUD_HOTBAR_SLOTS, HomeContinueV1, HomeViewV1, HotbarSlotV1, HotbarV1, HudModelV1, HudStatusV1,
    INVENTORY_SLOT_COUNT, InspectOverlayV1, InventoryClickV1, InventoryDraftV1, InventorySlotV1,
    LoadingViewV1, NewWorldViewV1, PackagesProfilesViewV1, PauseViewV1, QuitConfirmViewV1,
    RecipeListV1, RecipeRowV1, RecoveryViewV1, SavingViewV1, WorkbenchOverlayV1, WorldRowV1,
    WorldsViewV1, surface_key, try_surface_key,
};
pub use widgets::{
    ButtonWidget, CONTROL_VOCABULARY_MAJOR, ControlVocabularyRef, ListItemWidget, ListWidget,
    ModalWidget, SliderWidget, TextInputWidget, ToastWidget, ToggleWidget, UiRootError,
    WidgetCommand, WidgetError, WidgetKind, application_root, validate_control_vocabulary,
};
