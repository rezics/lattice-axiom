//! Typed semantic views for shell and game surfaces. Widgets never mutate routes.

mod hud;
mod keys;
mod modal;
mod overlay;
mod shell;

pub use hud::{
    HUD_HOTBAR_SLOTS, HotbarSlotV1, HotbarV1, HudModelV1, HudStatusV1, InspectOverlayV1,
};
pub use keys::{surface_key, try_surface_key};
pub use modal::{
    BindingCaptureViewV1, ConfirmSaveQuitViewV1, FatalRecoveryViewV1, PauseViewV1, SavingViewV1,
};
pub use overlay::{
    INVENTORY_SLOT_COUNT, InventoryClickV1, InventoryDraftV1, InventorySlotV1, RecipeListV1,
    RecipeRowV1, WorkbenchOverlayV1,
};
pub use shell::{
    DiagnosticsAboutViewV1, HomeContinueV1, HomeViewV1, LoadingViewV1, NewWorldViewV1,
    PackagesProfilesViewV1, QuitConfirmViewV1, RecoveryViewV1, WorldRowV1, WorldsViewV1,
};
