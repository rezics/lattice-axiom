//! Per-instance Bevy application ownership and host profiles.

use std::{fmt, time::Duration};

#[cfg(feature = "client")]
use std::sync::atomic::{AtomicBool, Ordering};

use bevy::{
    app::{App, FixedLast, PluginsState, ScheduleRunnerPlugin},
    prelude::{MinimalPlugins, PluginGroup, ResMut, Resource},
    tasks::tick_global_task_pools_on_main_thread,
    time::{Fixed, Real, Time, TimeUpdateStrategy, Virtual},
};
#[cfg(feature = "client")]
use bevy::{
    ecs::change_detection::DetectChanges,
    input_focus::tab_navigation::TabNavigationPlugin,
    prelude::{DefaultPlugins, Update, Window, WindowPlugin},
};
use thiserror::Error;

use latticeaxiom_core::CanonicalHash;
#[cfg(feature = "client")]
use latticeaxiom_launcher::{FreshClientAppLeaseProof, FreshClientAppLeaseToken};

use crate::prepared::{LockVerifiedComposeImages, StructurallyValidatedComposeImages};

/// Maximum fixed iterations accepted by one manual advancement call.
///
/// The bound keeps one host call from monopolizing the Bevy scheduler. Callers
/// that need to catch up further must issue multiple bounded advances.
pub const MAX_TICKS_PER_ADVANCE: u32 = 1_024;

#[cfg(feature = "client")]
static CLIENT_EVENT_LOOP_RESERVED: AtomicBool = AtomicBool::new(false);

/// Standard Bevy plugin profile selected for an engine instance.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Resource)]
pub enum EngineProfile {
    /// Interactive client built from Bevy's default plugin group.
    #[cfg(feature = "client")]
    Client,
    /// GPU-free host built from Bevy [`MinimalPlugins`].
    Headless,
}

/// Exact product-lock hash of the reopened `latticeaxiom.lock` used to boot.
///
/// Client and headless hosts that start from [`LockVerifiedComposeImages`]
/// carry the same hash. The resource is absent from scaffold instances that
/// bypass the production lock gate.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Resource)]
pub struct VerifiedProductLockHash(CanonicalHash);

impl VerifiedProductLockHash {
    /// Creates a hash resource from a reopened final product lock.
    #[must_use]
    pub const fn new(digest: CanonicalHash) -> Self {
        Self(digest)
    }

    /// Returns the exact product-lock hash.
    #[must_use]
    pub const fn get(self) -> CanonicalHash {
        self.0
    }
}

/// Number of Bevy fixed-schedule iterations completed by an instance.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Resource)]
pub struct FixedTickCount(u64);

impl FixedTickCount {
    /// Returns the completed fixed tick count.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// One isolated Lattice Axiom runtime hosted by its own Bevy [`App`].
///
/// The instance does not implement a runner, ECS, scheduler, or task runtime;
/// all of those remain owned by Bevy. Mutable [`App`] access is deliberately
/// limited to construction-time host setup so callers cannot replace the
/// deterministic headless clocks after activation.
pub struct EngineInstance {
    profile: EngineProfile,
    pub(crate) app: App,
    fixed_timestep: Option<Duration>,
}

impl EngineInstance {
    /// Builds the process's sole interactive client from Bevy [`DefaultPlugins`].
    ///
    /// Client creation reserves the process-global window event-loop slot
    /// permanently. This fails closed because the underlying winit event loop
    /// is not generally safe to recreate after a client is dropped.
    ///
    /// Ordinary launch must use [`Self::new_client_from_lock`] so a reopened
    /// final product lock is verified before runtime-image construction.
    ///
    /// # Errors
    ///
    /// Returns [`EngineInstanceError::ClientInstanceAlreadyExists`] after any
    /// earlier client-construction attempt reserved the process event loop.
    #[cfg(feature = "client")]
    pub fn new_client(
        images: StructurallyValidatedComposeImages,
    ) -> Result<Self, EngineInstanceError> {
        Self::new_client_with_setup(images, |_| {})
    }

    /// Builds the process's sole interactive client from a reopened final lock.
    ///
    /// The lock-verified images are shared with headless hosts and are not
    /// re-resolved. Native modules are not loaded. The launcher lease is
    /// consumed after Bevy [`DefaultPlugins`] are assembled.
    ///
    /// # Errors
    ///
    /// Returns [`EngineInstanceError::ClientInstanceAlreadyExists`] after any
    /// earlier client-construction attempt reserved the process event loop.
    #[cfg(feature = "client")]
    pub fn new_client_from_lock(
        images: LockVerifiedComposeImages,
        lease: FreshClientAppLeaseToken,
    ) -> Result<(Self, FreshClientAppLeaseProof), EngineInstanceError> {
        let product_lock_hash = VerifiedProductLockHash::new(images.product_lock_hash());
        let instance = Self::new_client_with_setup(images.into_images(), move |app| {
            app.insert_resource(product_lock_hash);
        })?;
        Ok((instance, lease.into_app_created_proof()))
    }

    /// Builds the process's sole interactive client with a host setup hook.
    ///
    /// The hook exists only while the static host scaffold is being assembled.
    /// The input has already passed the compiled-receipt and runtime callback
    /// gate; the hook must not install callbacks outside that verified map.
    ///
    /// # Errors
    ///
    /// Returns [`EngineInstanceError::ClientInstanceAlreadyExists`] after any
    /// earlier client-construction attempt reserved the process event loop.
    #[cfg(feature = "client")]
    pub(crate) fn new_client_with_setup(
        images: StructurallyValidatedComposeImages,
        host_setup: impl FnOnce(&mut App),
    ) -> Result<Self, EngineInstanceError> {
        reserve_client_event_loop()?;

        let mut app = App::new();
        app.insert_resource(images)
            .insert_resource(EngineProfile::Client)
            .init_resource::<FixedTickCount>()
            .add_plugins(DefaultPlugins.set(WindowPlugin {
                primary_window: Some(Window {
                    title: "Lattice Axiom".into(),
                    ..Window::default()
                }),
                ..WindowPlugin::default()
            }));
        crate::observability::install_client_observability(&mut app);
        app.add_plugins(crate::video::VideoRuntimePlugin)
            .add_plugins(TabNavigationPlugin)
            .add_systems(Update, limit_client_fixed_catch_up)
            .add_systems(FixedLast, count_fixed_tick);
        host_setup(&mut app);
        finalize_plugins(&mut app);

        Ok(Self {
            profile: EngineProfile::Client,
            app,
            fixed_timestep: None,
        })
    }

    /// Builds a GPU-free instance with a manually controlled fixed timestep.
    ///
    /// Ordinary launch must use [`Self::new_headless_from_lock`] so a reopened
    /// final product lock is verified before runtime-image construction.
    ///
    /// # Errors
    ///
    /// Returns [`EngineInstanceError::ZeroFixedTimestep`] when
    /// `fixed_timestep` is zero.
    pub fn new_headless(
        images: StructurallyValidatedComposeImages,
        fixed_timestep: Duration,
    ) -> Result<Self, EngineInstanceError> {
        Self::new_headless_with_setup(images, fixed_timestep, |_| {})
    }

    /// Builds a GPU-free instance from a reopened final product lock.
    ///
    /// The same lock-verified images may start a
    /// [`bevy::prelude::DefaultPlugins`] client.
    /// This constructor uses Bevy [`MinimalPlugins`] and does not load native
    /// modules or open a world writer.
    ///
    /// # Errors
    ///
    /// Returns [`EngineInstanceError::ZeroFixedTimestep`] when
    /// `fixed_timestep` is zero.
    pub fn new_headless_from_lock(
        images: LockVerifiedComposeImages,
        fixed_timestep: Duration,
    ) -> Result<Self, EngineInstanceError> {
        let product_lock_hash = VerifiedProductLockHash::new(images.product_lock_hash());
        Self::new_headless_with_setup(images.into_images(), fixed_timestep, move |app| {
            app.insert_resource(product_lock_hash);
        })
    }

    /// Builds a GPU-free instance with a construction-time host setup hook.
    ///
    /// Headless apps use Bevy's standard [`MinimalPlugins`] composition and
    /// contain no window or render plugins. The hook is a temporary static-host
    /// scaffold, not a verified package adapter. All plugins added by the hook
    /// complete Bevy's `ready`, `finish`, and `cleanup` lifecycle before
    /// this function returns.
    ///
    /// # Errors
    ///
    /// Returns [`EngineInstanceError::ZeroFixedTimestep`] when
    /// `fixed_timestep` is zero. The host setup hook is not invoked in that
    /// case.
    pub(crate) fn new_headless_with_setup(
        images: StructurallyValidatedComposeImages,
        fixed_timestep: Duration,
        host_setup: impl FnOnce(&mut App),
    ) -> Result<Self, EngineInstanceError> {
        if fixed_timestep.is_zero() {
            return Err(EngineInstanceError::ZeroFixedTimestep);
        }

        let mut app = App::new();
        app.insert_resource(images)
            .insert_resource(EngineProfile::Headless)
            .init_resource::<FixedTickCount>()
            .add_plugins(MinimalPlugins.set(ScheduleRunnerPlugin::run_once()))
            .add_systems(FixedLast, count_fixed_tick);
        host_setup(&mut app);
        finalize_plugins(&mut app);
        configure_manual_time(&mut app, fixed_timestep);

        Ok(Self {
            profile: EngineProfile::Headless,
            app,
            fixed_timestep: Some(fixed_timestep),
        })
    }

    /// Returns the selected standard Bevy host profile.
    #[must_use]
    pub const fn profile(&self) -> EngineProfile {
        self.profile
    }

    /// Returns read-only access to this instance's Bevy application.
    pub const fn app(&self) -> &App {
        &self.app
    }

    /// Returns the configured fixed timestep for a headless instance.
    #[must_use]
    pub const fn fixed_timestep(&self) -> Option<Duration> {
        self.fixed_timestep
    }

    /// Hands the instance to Bevy's client event-loop runner.
    ///
    /// Construction already finished and cleaned up plugins. The winit runner
    /// observes [`bevy::app::PluginsState::Cleaned`] and does not finish again.
    #[cfg(feature = "client")]
    pub fn run(mut self) -> bevy::app::AppExit {
        self.app.run()
    }

    /// Returns the number of completed fixed-schedule iterations.
    #[must_use]
    pub fn completed_fixed_ticks(&self) -> u64 {
        self.app
            .world()
            .get_resource::<FixedTickCount>()
            .copied()
            .map_or(0, FixedTickCount::get)
    }

    /// Requests a validated fixed frequency at the next fixed boundary.
    ///
    /// Render updates remain independently paced by [`crate::VideoRuntimeSettings`].
    ///
    /// # Errors
    ///
    /// Returns [`EngineInstanceError::SimulationTickRate`] when `hertz` is
    /// outside `1..=10_000`, or
    /// [`EngineInstanceError::SimulationClockUnavailable`] when the active
    /// host did not install [`latticeaxiom_player::PlayerPlugin`].
    pub fn request_simulation_tick_rate(
        &mut self,
        request_id: u64,
        hertz: u16,
    ) -> Result<(), EngineInstanceError> {
        let rate = latticeaxiom_player::SimulationTickRate::new(hertz)?;
        self.app
            .world_mut()
            .write_message(latticeaxiom_player::SimulationTickRateRequest { request_id, rate })
            .ok_or(EngineInstanceError::SimulationClockUnavailable)?;
        Ok(())
    }

    /// Advances a headless app through exactly `ticks` Bevy fixed iterations.
    ///
    /// This delegates to Bevy's [`TimeUpdateStrategy::FixedTimesteps`] and
    /// [`App::update`]; it does not create a project-owned runner.
    ///
    /// # Errors
    ///
    /// Returns [`EngineInstanceError::ManualTicksRequireHeadless`] for a
    /// client instance, [`EngineInstanceError::TickAdvanceLimitExceeded`] when
    /// `ticks` exceeds [`MAX_TICKS_PER_ADVANCE`], or
    /// [`EngineInstanceError::TickDurationOverflow`] when the configured
    /// timestep cannot be multiplied by `ticks`. Advancing by zero ticks
    /// succeeds without running an app frame.
    pub fn advance_fixed_ticks(&mut self, ticks: u32) -> Result<(), EngineInstanceError> {
        let Some(fixed_timestep) = self.fixed_timestep else {
            return Err(EngineInstanceError::ManualTicksRequireHeadless);
        };
        if ticks == 0 {
            return Ok(());
        }
        if ticks > MAX_TICKS_PER_ADVANCE {
            return Err(EngineInstanceError::TickAdvanceLimitExceeded {
                requested: ticks,
                maximum: MAX_TICKS_PER_ADVANCE,
            });
        }
        if fixed_timestep.checked_mul(ticks).is_none() {
            return Err(EngineInstanceError::TickDurationOverflow {
                fixed_timestep,
                ticks,
            });
        }

        self.app
            .world_mut()
            .insert_resource(TimeUpdateStrategy::FixedTimesteps(ticks));
        self.app.update();
        self.app
            .world_mut()
            .insert_resource(TimeUpdateStrategy::FixedTimesteps(0));
        Ok(())
    }
}

impl fmt::Debug for EngineInstance {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EngineInstance")
            .field("profile", &self.profile)
            .field("fixed_timestep", &self.fixed_timestep)
            .field("completed_fixed_ticks", &self.completed_fixed_ticks())
            .finish_non_exhaustive()
    }
}

#[cfg(feature = "client")]
pub(crate) fn reserve_client_event_loop() -> Result<(), EngineInstanceError> {
    CLIENT_EVENT_LOOP_RESERVED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .map(|_| ())
        .map_err(|_| EngineInstanceError::ClientInstanceAlreadyExists)
}

fn finalize_plugins(app: &mut App) {
    while app.plugins_state() == PluginsState::Adding {
        tick_global_task_pools_on_main_thread();
    }
    app.finish();
    app.cleanup();
}

fn configure_manual_time(app: &mut App, fixed_timestep: Duration) {
    // Bevy's first real-time update establishes a baseline with zero delta.
    // Establish it here so the first requested update advances exactly.
    let mut real_time = Time::<Real>::default();
    real_time.update_with_duration(Duration::ZERO);

    app.insert_resource(Time::<Fixed>::from_duration(fixed_timestep))
        .insert_resource(TimeUpdateStrategy::FixedTimesteps(0))
        .insert_resource(real_time)
        .insert_resource(Time::<Virtual>::from_max_delta(Duration::MAX));
}

fn count_fixed_tick(mut ticks: ResMut<'_, FixedTickCount>) {
    ticks.0 = ticks.0.saturating_add(1);
}

#[cfg(feature = "client")]
#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
fn limit_client_fixed_catch_up(
    clock: Option<bevy::prelude::Res<'_, latticeaxiom_player::SimulationClock>>,
    fixed: Option<bevy::prelude::Res<'_, Time<Fixed>>>,
    mut virtual_time: ResMut<'_, Time<Virtual>>,
) {
    let (Some(clock), Some(fixed)) = (clock, fixed) else {
        return;
    };
    if !clock.is_changed() {
        return;
    }
    let maximum = fixed.timestep().saturating_mul(2);
    virtual_time.set_max_delta(maximum);
}

/// Failure to construct or manually advance an engine instance.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum EngineInstanceError {
    /// Bevy requires a nonzero fixed timestep.
    #[error("the fixed timestep must be nonzero")]
    ZeroFixedTimestep,
    /// A requested interactive simulation frequency was invalid.
    #[error(transparent)]
    SimulationTickRate(#[from] latticeaxiom_player::SimulationTickRateError),
    /// The instance has no player-owned simulation clock message channel.
    #[error("the active host did not install the simulation clock")]
    SimulationClockUnavailable,
    /// The process-global client event-loop slot was already reserved.
    #[error("an interactive client event loop has already been reserved in this process")]
    ClientInstanceAlreadyExists,
    /// Manual deterministic advancement is only exposed by the headless host.
    #[error("manual fixed-tick advancement requires a headless engine instance")]
    ManualTicksRequireHeadless,
    /// One call requested more work than the scheduler monopolization bound.
    #[error("requested {requested} fixed ticks, but one advance is limited to {maximum}")]
    TickAdvanceLimitExceeded {
        /// Requested fixed iterations.
        requested: u32,
        /// Maximum fixed iterations accepted by one call.
        maximum: u32,
    },
    /// Multiplying the fixed timestep by the requested tick count overflowed.
    #[error("fixed timestep {fixed_timestep:?} overflows when multiplied by {ticks} ticks")]
    TickDurationOverflow {
        /// Configured fixed timestep.
        fixed_timestep: Duration,
        /// Requested fixed iterations.
        ticks: u32,
    },
}

#[cfg(test)]
mod tests {
    use bevy::prelude::{App, MinimalPlugins, Plugin, Resource};

    use super::finalize_plugins;

    #[cfg(feature = "client")]
    use super::{EngineInstanceError, reserve_client_event_loop};

    #[test]
    #[cfg(feature = "client")]
    fn client_event_loop_reservation_is_process_global_and_permanent() {
        assert_eq!(reserve_client_event_loop(), Ok(()));
        assert_eq!(
            reserve_client_event_loop(),
            Err(EngineInstanceError::ClientInstanceAlreadyExists)
        );
    }

    #[derive(Resource, Default)]
    struct LifecycleProbe {
        finished: bool,
        cleaned: bool,
    }

    struct LifecycleProbePlugin;

    impl Plugin for LifecycleProbePlugin {
        fn build(&self, app: &mut App) {
            app.init_resource::<LifecycleProbe>();
        }

        fn finish(&self, app: &mut App) {
            app.world_mut().resource_mut::<LifecycleProbe>().finished = true;
        }

        fn cleanup(&self, app: &mut App) {
            app.world_mut().resource_mut::<LifecycleProbe>().cleaned = true;
        }
    }

    #[test]
    fn plugins_added_by_host_setup_finish_and_cleanup_before_updates() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(LifecycleProbePlugin);

        finalize_plugins(&mut app);

        let probe = app.world().resource::<LifecycleProbe>();
        assert!(probe.finished);
        assert!(probe.cleaned);
    }
}
