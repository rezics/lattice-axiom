//! Development observability built from Bevy's diagnostics and logging runtime.

use bevy::prelude::App;

#[cfg(feature = "development")]
use std::time::Duration;

#[cfg(feature = "development")]
use bevy::{
    diagnostic::{
        EntityCountDiagnosticsPlugin, FrameTimeDiagnosticsPlugin, LogDiagnosticsPlugin,
        SystemInformationDiagnosticsPlugin,
    },
    log::info,
    prelude::{Plugin, Res, Startup},
};

#[cfg(feature = "development")]
use crate::{EngineProfile, VerifiedProductLockHash};

#[cfg(feature = "development")]
const DEVELOPMENT_DIAGNOSTIC_INTERVAL: Duration = Duration::from_secs(1);

/// Installs bounded client diagnostics for the development feature set.
pub(crate) fn install_client_observability(app: &mut App) {
    #[cfg(feature = "development")]
    app.add_plugins(DevelopmentObservabilityPlugin);

    #[cfg(not(feature = "development"))]
    let _ = app;
}

#[cfg(feature = "development")]
struct DevelopmentObservabilityPlugin;

#[cfg(feature = "development")]
impl Plugin for DevelopmentObservabilityPlugin {
    fn build(&self, app: &mut App) {
        let logged_diagnostics = [
            FrameTimeDiagnosticsPlugin::FPS,
            FrameTimeDiagnosticsPlugin::FRAME_TIME,
            EntityCountDiagnosticsPlugin::ENTITY_COUNT,
            SystemInformationDiagnosticsPlugin::PROCESS_CPU_USAGE,
            SystemInformationDiagnosticsPlugin::PROCESS_MEM_USAGE,
        ]
        .into_iter()
        .collect();

        app.add_plugins((
            FrameTimeDiagnosticsPlugin::default(),
            EntityCountDiagnosticsPlugin::default(),
            SystemInformationDiagnosticsPlugin,
            LogDiagnosticsPlugin {
                debug: false,
                wait_duration: DEVELOPMENT_DIAGNOSTIC_INTERVAL,
                filter: Some(logged_diagnostics),
            },
        ))
        .add_systems(Startup, log_client_app_started);
    }
}

#[cfg(feature = "development")]
#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
fn log_client_app_started(
    profile: Res<'_, EngineProfile>,
    product_lock: Option<Res<'_, VerifiedProductLockHash>>,
) {
    if let Some(product_lock) = product_lock {
        info!(
            target: "latticeaxiom::lifecycle",
            event = "client_app_started",
            component = "engine",
            profile = ?*profile,
            product_lock = %product_lock.get(),
            "client application entered the Bevy startup schedule"
        );
    } else {
        info!(
            target: "latticeaxiom::lifecycle",
            event = "client_app_started",
            component = "engine",
            profile = ?*profile,
            "client application entered the Bevy startup schedule"
        );
    }
}

#[cfg(all(test, feature = "development"))]
mod tests {
    use bevy::{
        diagnostic::{
            DiagnosticsPlugin, EntityCountDiagnosticsPlugin, FrameTimeDiagnosticsPlugin,
            LogDiagnosticsPlugin, SystemInformationDiagnosticsPlugin,
        },
        prelude::{App, MinimalPlugins},
    };

    use super::install_client_observability;

    #[test]
    fn development_observability_uses_bevy_diagnostic_plugins() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(DiagnosticsPlugin);

        install_client_observability(&mut app);

        assert!(app.is_plugin_added::<FrameTimeDiagnosticsPlugin>());
        assert!(app.is_plugin_added::<EntityCountDiagnosticsPlugin>());
        assert!(app.is_plugin_added::<SystemInformationDiagnosticsPlugin>());
        assert!(app.is_plugin_added::<LogDiagnosticsPlugin>());
    }
}
