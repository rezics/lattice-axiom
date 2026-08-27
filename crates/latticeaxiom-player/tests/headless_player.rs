//! Headless acceptance tests for the fixed-cycle D2 player contract.

#![allow(clippy::expect_used)] // Test expectations state the fixture invariant being checked.
#![allow(clippy::float_cmp)] // Exact frozen constants and axis vectors are intentional.

use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use avian3d::prelude::{
    Collider, LinearVelocity, PhysicsPlugins, PhysicsSchedule, RigidBody, Sensor,
};
use bevy::{
    app::App,
    prelude::{MinimalPlugins, Quat, Time, Transform, Vec3},
    time::{Fixed, TimeUpdateStrategy, Virtual},
    transform::TransformPlugin,
};
use latticeaxiom_gameplay::{BlockPosition, ChunkRevision, PlayerId};
use latticeaxiom_player::{
    ActionAxis2V1, ActionFrameInbox, AuthoritativeBlockEditRequestV1, BlockEditAuthority,
    BlockEditAuthorityResource, BlockEditRejectV1, BlockEditSuccessV1, D2PlayerBundle,
    DetachedSpectator, PlayerActionButtonsV1, PlayerActionFrameV1, PlayerActionV1,
    PlayerControllerState, PlayerFixedTick, PlayerPlugin, SuccessfulEditCooldownV1,
};

const FIXED_HZ: f64 = 60.0;

fn headless_app() -> App {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        TransformPlugin,
        PhysicsPlugins::default(),
        PlayerPlugin,
    ));
    app.world_mut()
        .resource_mut::<Time<Virtual>>()
        .set_max_delta(Duration::from_secs(10));
    app.finish();
    app.cleanup();
    app
}

fn prime_spatial_queries(app: &mut App) {
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::ZERO));
    app.update();
    app.world_mut().run_schedule(PhysicsSchedule);
}

fn test_app() -> App {
    let mut app = headless_app();
    app.world_mut().spawn((
        RigidBody::Static,
        Collider::cuboid(200.0, 1.0, 200.0),
        Transform::from_xyz(0.0, -0.5, 0.0),
    ));
    prime_spatial_queries(&mut app);
    app
}

fn spawn_player(app: &mut App) -> bevy::prelude::Entity {
    app.world_mut()
        .spawn(D2PlayerBundle::new(
            PlayerId::new(1),
            Transform::from_xyz(0.0, 0.91, 0.0),
        ))
        .id()
}

fn push_frames(app: &mut App, frames: impl IntoIterator<Item = PlayerActionFrameV1>) {
    let mut inbox = app.world_mut().resource_mut::<ActionFrameInbox>();
    for frame in frames {
        assert!(inbox.push_headless(frame).is_ok());
    }
}

fn run_update_for_ticks(app: &mut App, ticks: u32) {
    let timestep = app.world().resource::<Time<Fixed>>().timestep();
    let elapsed = timestep.checked_mul(ticks).unwrap_or(Duration::MAX);
    app.insert_resource(TimeUpdateStrategy::ManualDuration(elapsed));
    app.update();
}

fn movement_frames(count: u64) -> impl Iterator<Item = PlayerActionFrameV1> {
    (1..=count).map(|generation| PlayerActionFrameV1 {
        generation,
        movement: ActionAxis2V1 { x: 0.0, y: 1.0 },
        ..PlayerActionFrameV1::default()
    })
}

fn run_batched_movement(batch: u32) -> (Transform, PlayerFixedTick) {
    let mut app = test_app();
    let player = spawn_player(&mut app);
    push_frames(&mut app, movement_frames(120));
    for _ in 0..(120 / batch) {
        run_update_for_ticks(&mut app, batch);
    }
    (
        *app.world()
            .entity(player)
            .get::<Transform>()
            .expect("player bundle has Transform"),
        *app.world().resource::<PlayerFixedTick>(),
    )
}

#[test]
fn fixed_action_stream_is_independent_of_update_batching() {
    let (single_transform, single_tick) = run_batched_movement(1);
    let (quad_transform, quad_tick) = run_batched_movement(4);

    assert_eq!(single_tick, quad_tick);
    assert_eq!(single_tick.get(), 120);
    assert_eq!(single_transform.translation, quad_transform.translation);
    assert!((single_transform.translation.z + 9.0).abs() < 0.15);
}

#[test]
fn sixty_hz_jump_reaches_the_frozen_apex() {
    let mut app = test_app();
    let player = spawn_player(&mut app);
    push_frames(
        &mut app,
        (1..=3)
            .map(|generation| PlayerActionFrameV1 {
                generation,
                ..PlayerActionFrameV1::default()
            })
            .chain(std::iter::once({
                let mut started = PlayerActionButtonsV1::empty();
                started.insert(PlayerActionV1::Jump);
                PlayerActionFrameV1 {
                    generation: 4,
                    started,
                    ..PlayerActionFrameV1::default()
                }
            }))
            .chain((5..=90).map(|generation| PlayerActionFrameV1 {
                generation,
                ..PlayerActionFrameV1::default()
            })),
    );

    let mut apex = 0.91_f32;
    for _ in 0..90 {
        run_update_for_ticks(&mut app, 1);
        apex = apex.max(
            app.world()
                .entity(player)
                .get::<Transform>()
                .expect("player Transform remains present")
                .translation
                .y,
        );
    }

    assert!((apex - 0.91 - 1.25).abs() < 0.08, "observed apex {apex}");
}

fn hold_forward_frames(count: u64, sprint: bool) -> impl Iterator<Item = PlayerActionFrameV1> {
    (1..=count).map(move |generation| {
        let mut held = PlayerActionButtonsV1::empty();
        if sprint {
            held.insert(PlayerActionV1::Sprint);
        }
        PlayerActionFrameV1 {
            generation,
            movement: ActionAxis2V1 { x: 0.0, y: 1.0 },
            held,
            ..PlayerActionFrameV1::default()
        }
    })
}

fn horizontal_distance_after(
    frames: impl IntoIterator<Item = PlayerActionFrameV1>,
    ticks: u32,
) -> f32 {
    let mut app = test_app();
    let player = spawn_player(&mut app);
    push_frames(&mut app, frames);
    run_update_for_ticks(&mut app, ticks);
    app.world()
        .entity(player)
        .get::<Transform>()
        .expect("player Transform remains present")
        .translation
        .z
}

#[test]
fn forward_sprint_is_thirty_percent_faster_than_walk() {
    let walked = horizontal_distance_after(hold_forward_frames(60, false), 60);
    let sprinted = horizontal_distance_after(hold_forward_frames(60, true), 60);
    assert!((walked + 4.50).abs() < 0.15, "walk distance was {walked}");
    assert!(
        (sprinted + 5.85).abs() < 0.20,
        "sprint distance was {sprinted}"
    );
    assert!(
        sprinted.abs() > walked.abs() + 1.0,
        "sprint {sprinted} should outpace walk {walked}"
    );
}

fn jump_edge_frame(generation: u64) -> PlayerActionFrameV1 {
    let mut started = PlayerActionButtonsV1::empty();
    let mut held = PlayerActionButtonsV1::empty();
    started.insert(PlayerActionV1::Jump);
    held.insert(PlayerActionV1::Jump);
    PlayerActionFrameV1 {
        generation,
        started,
        held,
        ..PlayerActionFrameV1::default()
    }
}

fn held_frame(generation: u64, action: PlayerActionV1) -> PlayerActionFrameV1 {
    let mut held = PlayerActionButtonsV1::empty();
    held.insert(action);
    PlayerActionFrameV1 {
        generation,
        held,
        ..PlayerActionFrameV1::default()
    }
}

#[test]
fn double_jump_enters_flight_and_holds_altitude() {
    let mut app = test_app();
    let player = spawn_player(&mut app);
    push_frames(
        &mut app,
        (1..=8)
            .map(|generation| PlayerActionFrameV1 {
                generation,
                ..PlayerActionFrameV1::default()
            })
            .chain(std::iter::once(jump_edge_frame(9)))
            .chain(std::iter::once(jump_edge_frame(10)))
            .chain((11..=40).map(|generation| PlayerActionFrameV1 {
                generation,
                ..PlayerActionFrameV1::default()
            })),
    );
    run_update_for_ticks(&mut app, 10);
    let after_toggle = *app
        .world()
        .entity(player)
        .get::<PlayerControllerState>()
        .expect("player bundle has controller state");
    assert!(after_toggle.flying(), "second jump must enable flight");
    let start_y = app
        .world()
        .entity(player)
        .get::<Transform>()
        .expect("player Transform remains present")
        .translation
        .y;
    run_update_for_ticks(&mut app, 30);
    let end_y = app
        .world()
        .entity(player)
        .get::<Transform>()
        .expect("player Transform remains present")
        .translation
        .y;
    let flying = app
        .world()
        .entity(player)
        .get::<PlayerControllerState>()
        .expect("player bundle has controller state")
        .flying();
    assert!(flying, "idle flight must not fall out of the air");
    assert!(
        (end_y - start_y).abs() < 0.08,
        "flight must cancel gravity: {start_y} -> {end_y}"
    );
}

#[test]
fn flight_ascends_with_jump_and_descends_with_sneak() {
    let mut app = test_app();
    let player = spawn_player(&mut app);
    push_frames(
        &mut app,
        (1..=8)
            .map(|generation| PlayerActionFrameV1 {
                generation,
                ..PlayerActionFrameV1::default()
            })
            .chain(std::iter::once(jump_edge_frame(9)))
            .chain(std::iter::once(jump_edge_frame(10)))
            .chain((11..=40).map(|generation| held_frame(generation, PlayerActionV1::Jump)))
            .chain((41..=70).map(|generation| held_frame(generation, PlayerActionV1::Sneak))),
    );
    run_update_for_ticks(&mut app, 10);
    let takeoff_y = app
        .world()
        .entity(player)
        .get::<Transform>()
        .expect("player Transform remains present")
        .translation
        .y;
    run_update_for_ticks(&mut app, 30);
    let climbed_y = app
        .world()
        .entity(player)
        .get::<Transform>()
        .expect("player Transform remains present")
        .translation
        .y;
    assert!(
        climbed_y - takeoff_y > 4.5,
        "held jump while flying must climb: {takeoff_y} -> {climbed_y}"
    );
    run_update_for_ticks(&mut app, 30);
    let descended_y = app
        .world()
        .entity(player)
        .get::<Transform>()
        .expect("player Transform remains present")
        .translation
        .y;
    assert!(
        climbed_y - descended_y > 4.5,
        "held sneak while flying must descend: {climbed_y} -> {descended_y}"
    );
}

#[test]
fn landing_while_flying_cancels_flight() {
    let mut app = test_app();
    let player = spawn_player(&mut app);
    push_frames(
        &mut app,
        (1..=8)
            .map(|generation| PlayerActionFrameV1 {
                generation,
                ..PlayerActionFrameV1::default()
            })
            .chain(std::iter::once(jump_edge_frame(9)))
            .chain(std::iter::once(jump_edge_frame(10)))
            .chain((11..=80).map(|generation| held_frame(generation, PlayerActionV1::Sneak))),
    );
    run_update_for_ticks(&mut app, 80);
    let state = *app
        .world()
        .entity(player)
        .get::<PlayerControllerState>()
        .expect("player bundle has controller state");
    assert!(
        !state.flying(),
        "touching walkable ground must cancel flight"
    );
    assert!(state.grounded(), "cancelled flight must land on the floor");
}

fn coyote_jump_velocity(target_airborne_tick: u8) -> Vec3 {
    let mut app = headless_app();
    app.world_mut().spawn((
        RigidBody::Static,
        Collider::cuboid(2.0, 1.0, 8.0),
        Transform::from_xyz(0.0, -0.5, 0.0),
    ));
    prime_spatial_queries(&mut app);
    let player = spawn_player(&mut app);

    for generation in 1..=80_u64 {
        let state = *app
            .world()
            .entity(player)
            .get::<PlayerControllerState>()
            .expect("player bundle has controller state");
        let jump_now = !state.grounded()
            && state.ticks_since_grounded() == target_airborne_tick.saturating_sub(1);
        let mut started = PlayerActionButtonsV1::empty();
        if jump_now {
            started.insert(PlayerActionV1::Jump);
        }
        push_frames(
            &mut app,
            std::iter::once(PlayerActionFrameV1 {
                generation,
                movement: ActionAxis2V1 { x: 1.0, y: 0.0 },
                started,
                ..PlayerActionFrameV1::default()
            }),
        );
        run_update_for_ticks(&mut app, 1);
        if jump_now {
            return app
                .world()
                .entity(player)
                .get::<LinearVelocity>()
                .expect("player bundle has velocity")
                .0;
        }
    }
    panic!("player did not leave the finite coyote platform");
}

#[test]
fn command_input_coyote_window_accepts_six_ticks_and_rejects_seven() {
    let accepted = coyote_jump_velocity(6);
    let rejected = coyote_jump_velocity(7);
    assert!(
        accepted.y > 6.0,
        "sixth-tick coyote jump was lost: {accepted:?}"
    );
    assert!(
        rejected.y < 0.0,
        "seventh-tick coyote jump was accepted: {rejected:?}"
    );
}

fn first_falling_contact_tick() -> u32 {
    let mut app = test_app();
    let player = app
        .world_mut()
        .spawn(D2PlayerBundle::new(
            PlayerId::new(1),
            Transform::from_xyz(0.0, 3.0, 0.0),
        ))
        .id();
    for tick in 1..=120_u32 {
        push_frames(
            &mut app,
            std::iter::once(PlayerActionFrameV1 {
                generation: u64::from(tick),
                ..PlayerActionFrameV1::default()
            }),
        );
        run_update_for_ticks(&mut app, 1);
        if app
            .world()
            .entity(player)
            .get::<PlayerControllerState>()
            .expect("player bundle has controller state")
            .grounded()
        {
            return tick;
        }
    }
    panic!("falling player did not reach the floor");
}

fn buffered_landing_velocity(window_tick: u32) -> (Vec3, PlayerControllerState) {
    let contact_tick = first_falling_contact_tick();
    let resolution_tick = contact_tick;
    let jump_tick = resolution_tick
        .checked_sub(window_tick.saturating_sub(1))
        .expect("fall fixture is high enough for the buffer window");
    let mut app = test_app();
    let player = app
        .world_mut()
        .spawn(D2PlayerBundle::new(
            PlayerId::new(1),
            Transform::from_xyz(0.0, 3.0, 0.0),
        ))
        .id();
    for tick in 1..=resolution_tick {
        let mut started = PlayerActionButtonsV1::empty();
        if tick == jump_tick {
            started.insert(PlayerActionV1::Jump);
        }
        push_frames(
            &mut app,
            std::iter::once(PlayerActionFrameV1 {
                generation: u64::from(tick),
                started,
                ..PlayerActionFrameV1::default()
            }),
        );
        run_update_for_ticks(&mut app, 1);
    }
    (
        app.world()
            .entity(player)
            .get::<LinearVelocity>()
            .expect("player bundle has velocity")
            .0,
        *app.world()
            .entity(player)
            .get::<PlayerControllerState>()
            .expect("player bundle has controller state"),
    )
}

#[test]
fn command_input_jump_buffer_accepts_six_ticks_and_rejects_seven() {
    let (accepted_velocity, accepted_state) = buffered_landing_velocity(6);
    let (rejected_velocity, rejected_state) = buffered_landing_velocity(7);
    assert!(accepted_velocity.y > 6.0 && !accepted_state.grounded());
    assert!(
        rejected_velocity.y <= f32::EPSILON && rejected_state.grounded(),
        "seventh-tick buffer unexpectedly jumped: {rejected_velocity:?}"
    );
}

#[test]
fn controller_snaps_a_supported_capsule_instead_of_hovering() {
    let mut app = test_app();
    let player = app
        .world_mut()
        .spawn(D2PlayerBundle::new(
            PlayerId::new(1),
            Transform::from_xyz(0.0, 0.98, 0.0),
        ))
        .id();
    push_frames(
        &mut app,
        std::iter::once(PlayerActionFrameV1 {
            generation: 1,
            ..PlayerActionFrameV1::default()
        }),
    );

    run_update_for_ticks(&mut app, 1);
    let height = app
        .world()
        .entity(player)
        .get::<Transform>()
        .expect("player Transform remains present")
        .translation
        .y;
    assert!(
        (height - 0.91).abs() < 0.002,
        "capsule did not snap: {height}"
    );
}

#[test]
fn sensor_colliders_neither_support_nor_block_the_capsule() {
    let mut app = headless_app();
    app.world_mut().spawn((
        RigidBody::Static,
        Collider::cuboid(20.0, 1.0, 20.0),
        Sensor,
        Transform::from_xyz(0.0, -0.5, 0.0),
    ));
    prime_spatial_queries(&mut app);
    let player = spawn_player(&mut app);
    push_frames(
        &mut app,
        (1..=20).map(|generation| PlayerActionFrameV1 {
            generation,
            ..PlayerActionFrameV1::default()
        }),
    );

    run_update_for_ticks(&mut app, 20);
    let height = app
        .world()
        .entity(player)
        .get::<Transform>()
        .expect("player Transform remains present")
        .translation
        .y;
    assert!(
        height < 0.5,
        "sensor incorrectly supported capsule at {height}"
    );
}

#[test]
fn controller_crosses_an_exact_static_collider_seam() {
    let mut app = headless_app();
    for center_x in [-2.0, 2.0] {
        app.world_mut().spawn((
            RigidBody::Static,
            Collider::cuboid(4.0, 1.0, 12.0),
            Transform::from_xyz(center_x, -0.5, 0.0),
        ));
    }
    prime_spatial_queries(&mut app);
    let player = app
        .world_mut()
        .spawn(D2PlayerBundle::new(
            PlayerId::new(1),
            Transform::from_xyz(-3.0, 0.91, 0.0),
        ))
        .id();
    push_frames(
        &mut app,
        (1..=70).map(|generation| PlayerActionFrameV1 {
            generation,
            movement: ActionAxis2V1 { x: 1.0, y: 0.0 },
            ..PlayerActionFrameV1::default()
        }),
    );

    run_update_for_ticks(&mut app, 70);
    let position = app
        .world()
        .entity(player)
        .get::<Transform>()
        .expect("player Transform remains present")
        .translation;
    assert!(position.x > 2.0, "player stopped at seam: {position:?}");
    assert!(
        (position.y - 0.91).abs() < 0.03,
        "seam changed capsule height: {position:?}"
    );
}

fn run_step_case(step_height_m: f32) -> Vec3 {
    let mut app = test_app();
    app.world_mut().spawn((
        RigidBody::Static,
        Collider::cuboid(4.0, step_height_m, 4.0),
        Transform::from_xyz(0.0, step_height_m * 0.5, -4.0),
    ));
    prime_spatial_queries(&mut app);
    let player = spawn_player(&mut app);
    push_frames(
        &mut app,
        (1..=60).map(|generation| PlayerActionFrameV1 {
            generation,
            movement: ActionAxis2V1 { x: 0.0, y: 1.0 },
            ..PlayerActionFrameV1::default()
        }),
    );
    run_update_for_ticks(&mut app, 60);
    app.world()
        .entity(player)
        .get::<Transform>()
        .expect("player Transform remains present")
        .translation
}

#[test]
fn controller_steps_up_the_inclusive_sixty_centimeter_boundary() {
    let position = run_step_case(0.60);
    assert!(
        position.z < -3.5,
        "player did not traverse step: {position:?}"
    );
    assert!(
        position.y > 1.45,
        "player did not land on 0.60 m step: {position:?}"
    );
}

#[test]
fn controller_rejects_a_step_above_the_frozen_boundary() {
    let position = run_step_case(0.61);
    assert!(
        position.z > -1.8,
        "player traversed oversized step: {position:?}"
    );
    assert!(
        position.y < 1.05,
        "player climbed oversized step: {position:?}"
    );
}

fn run_slope_case(angle_degrees: f32) -> Vec3 {
    let mut app = test_app();
    let angle = angle_degrees.to_radians();
    let half_length = 3.0_f32;
    let half_thickness = 0.1_f32;
    let low_y_offset = half_thickness * angle.cos() - half_length * angle.sin();
    let low_z_offset = half_thickness * angle.sin() + half_length * angle.cos();
    app.world_mut().spawn((
        RigidBody::Static,
        Collider::cuboid(4.0, half_thickness * 2.0, half_length * 2.0),
        Transform::from_xyz(0.0, -low_y_offset, 1.5 - low_z_offset)
            .with_rotation(Quat::from_rotation_x(angle)),
    ));
    prime_spatial_queries(&mut app);
    let player = app
        .world_mut()
        .spawn(D2PlayerBundle::new(
            PlayerId::new(1),
            Transform::from_xyz(0.0, 0.91, 3.0),
        ))
        .id();
    push_frames(
        &mut app,
        (1..=60).map(|generation| PlayerActionFrameV1 {
            generation,
            movement: ActionAxis2V1 { x: 0.0, y: 1.0 },
            ..PlayerActionFrameV1::default()
        }),
    );
    run_update_for_ticks(&mut app, 60);
    app.world()
        .entity(player)
        .get::<Transform>()
        .expect("player Transform remains present")
        .translation
}

#[test]
fn controller_walks_the_inclusive_forty_five_degree_slope() {
    let position = run_slope_case(45.0);
    assert!(
        position.z < 1.1,
        "player did not advance up slope: {position:?}"
    );
    assert!(
        position.y > 1.5,
        "player did not rise up slope: {position:?}"
    );
}

#[test]
fn controller_rejects_the_forty_six_degree_slope() {
    let position = run_slope_case(46.0);
    assert!(
        position.z > 1.4,
        "player advanced up steep slope: {position:?}"
    );
    assert!(
        position.y < 1.05,
        "player rose up steep slope: {position:?}"
    );
}

#[derive(Debug)]
struct CapturingAuthority {
    requests: Arc<Mutex<Vec<AuthoritativeBlockEditRequestV1>>>,
}

impl BlockEditAuthority for CapturingAuthority {
    fn apply(
        &mut self,
        request: AuthoritativeBlockEditRequestV1,
    ) -> Result<BlockEditSuccessV1, BlockEditRejectV1> {
        self.requests
            .lock()
            .expect("test request mutex is not poisoned")
            .push(request);
        Ok(BlockEditSuccessV1 {
            position: BlockPosition { x: 0, y: 1, z: -1 },
            old_content: None,
            new_content: None,
            committed_chunk_revision: ChunkRevision::new(1),
        })
    }
}

#[derive(Debug)]
struct CountingAuthority {
    calls: Arc<AtomicUsize>,
}

impl BlockEditAuthority for CountingAuthority {
    fn apply(
        &mut self,
        _request: AuthoritativeBlockEditRequestV1,
    ) -> Result<BlockEditSuccessV1, BlockEditRejectV1> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        Ok(BlockEditSuccessV1 {
            position: BlockPosition { x: 0, y: 0, z: -1 },
            old_content: None,
            new_content: None,
            committed_chunk_revision: ChunkRevision::new(1),
        })
    }
}

fn edit_frame(generation: u64, action: PlayerActionV1) -> PlayerActionFrameV1 {
    let mut started = PlayerActionButtonsV1::empty();
    started.insert(action);
    PlayerActionFrameV1 {
        generation,
        started,
        ..PlayerActionFrameV1::default()
    }
}

#[test]
fn successful_break_and_place_share_the_twelve_tick_limiter() {
    let mut app = test_app();
    let player = spawn_player(&mut app);
    let calls = Arc::new(AtomicUsize::new(0));
    app.insert_resource(BlockEditAuthorityResource::new(CountingAuthority {
        calls: Arc::clone(&calls),
    }));

    let frames = std::iter::once(edit_frame(1, PlayerActionV1::BreakBlock))
        .chain((2..=11).map(|generation| PlayerActionFrameV1 {
            generation,
            ..PlayerActionFrameV1::default()
        }))
        .chain(std::iter::once(edit_frame(12, PlayerActionV1::PlaceBlock)))
        .chain(std::iter::once(edit_frame(13, PlayerActionV1::PlaceBlock)));
    push_frames(&mut app, frames);

    run_update_for_ticks(&mut app, 12);
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    run_update_for_ticks(&mut app, 1);
    assert_eq!(calls.load(Ordering::Relaxed), 2);
    assert_eq!(
        app.world()
            .entity(player)
            .get::<SuccessfulEditCooldownV1>()
            .expect("player bundle has edit limiter")
            .next_success_tick(),
        24
    );
}

#[test]
fn detached_spectator_has_zero_authoritative_diff_and_cannot_edit() {
    let mut app = test_app();
    let player = spawn_player(&mut app);
    let before_transform = *app
        .world()
        .entity(player)
        .get::<Transform>()
        .expect("player bundle has Transform");
    let before_velocity = *app
        .world()
        .entity(player)
        .get::<LinearVelocity>()
        .expect("player bundle has LinearVelocity");
    app.world_mut()
        .entity_mut(player)
        .insert(DetachedSpectator {
            position: Vec3::new(0.0, 1.62, 0.0),
            yaw_radians: 0.0,
            pitch_radians: 0.0,
        });

    let calls = Arc::new(AtomicUsize::new(0));
    app.insert_resource(BlockEditAuthorityResource::new(CountingAuthority {
        calls: Arc::clone(&calls),
    }));
    let mut frame = edit_frame(1, PlayerActionV1::BreakBlock);
    frame.movement = ActionAxis2V1 { x: 1.0, y: 1.0 };
    push_frames(&mut app, std::iter::once(frame));
    run_update_for_ticks(&mut app, 1);

    assert_eq!(calls.load(Ordering::Relaxed), 0);
    assert_eq!(
        app.world()
            .entity(player)
            .get::<Transform>()
            .expect("player Transform remains present"),
        &before_transform
    );
    assert_eq!(
        app.world()
            .entity(player)
            .get::<LinearVelocity>()
            .expect("player LinearVelocity remains present"),
        &before_velocity
    );
}

#[test]
fn authoritative_edit_interface_receives_fixed_eye_pose_and_five_meter_reach() {
    let mut app = test_app();
    let _player = spawn_player(&mut app);
    let requests = Arc::new(Mutex::new(Vec::new()));
    app.insert_resource(BlockEditAuthorityResource::new(CapturingAuthority {
        requests: Arc::clone(&requests),
    }));
    push_frames(
        &mut app,
        std::iter::once(edit_frame(73, PlayerActionV1::BreakBlock)),
    );

    run_update_for_ticks(&mut app, 1);
    let requests = requests.lock().expect("test request mutex is not poisoned");
    assert_eq!(requests.len(), 1);
    let request = &requests[0];
    assert_eq!(request.fixed_tick, 0);
    assert_eq!(request.intent.input_generation, 73);
    assert_eq!(request.maximum_reach_m(), 5.0);
    assert_eq!(request.eye_pose.forward, [0.0, 0.0, -1.0]);
    assert!(
        request.eye_pose.origin_m[0].abs() < f32::EPSILON
            && (request.eye_pose.origin_m[1] - 1.63).abs() < 0.001
            && request.eye_pose.origin_m[2].abs() < f32::EPSILON,
        "unexpected authoritative eye origin: {:?}",
        request.eye_pose.origin_m
    );

    assert_eq!(FIXED_HZ, 60.0);
    assert_eq!(latticeaxiom_player::MAX_BLOCK_EDIT_REACH_M, 5.0);
}
