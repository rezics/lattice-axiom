//! Authoritative mining progress projected as a crosshair ring.

use std::f32::consts::{FRAC_PI_2, PI, TAU};

use bevy::{
    math::Rot2,
    prelude::{
        BackgroundColor, BorderRadius, Color, Component, Display, MessageReader, Name, Node,
        Pickable, Query, Res, ResMut, Resource, Time, UiTransform, Val, Vec2, With,
    },
    ui::FocusPolicy,
};
use latticeaxiom_player::{
    BlockEditActionV1, BlockEditReceiptV1, BlockEditRejectV1, ClientInputOwnership, D2Player,
    LocalPlayerInput, MiningCancelReceiptV1, MiningProgressV1,
};

const RING_SEGMENTS: u8 = 32;
const RING_DIAMETER_PX: f32 = 48.0;
const RING_RADIUS_PX: f32 = 20.0;
const SEGMENT_WIDTH_PX: f32 = 4.5;
const SEGMENT_HEIGHT_PX: f32 = 2.2;
const COMPLETION_ANIMATION_SECONDS: f32 = 0.32;

/// Root node of the mining-progress ring.
#[derive(Clone, Copy, Component, Debug, Default, Eq, PartialEq)]
pub(super) struct ProductionMiningRing;

/// Clockwise segment index beginning at twelve o'clock.
#[derive(Clone, Copy, Component, Debug, Eq, PartialEq)]
pub(super) struct ProductionMiningRingSegment(u8);

#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum MiningRingPhase {
    #[default]
    Hidden,
    Active(MiningProgressV1),
    Completing {
        elapsed_seconds: f32,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MiningRingSignal {
    Progress(MiningProgressV1),
    Completed,
    Hide,
}

/// Presentation state driven only by local authoritative edit receipts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Resource)]
pub(super) struct ProductionMiningRingState {
    phase: MiningRingPhase,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct MiningRingPresentation {
    filled_segments: u8,
    scale: f32,
    opacity: f32,
}

impl ProductionMiningRingState {
    fn apply(&mut self, signal: MiningRingSignal) {
        self.phase = match signal {
            MiningRingSignal::Progress(progress) => MiningRingPhase::Active(progress),
            MiningRingSignal::Completed => MiningRingPhase::Completing {
                elapsed_seconds: 0.0,
            },
            MiningRingSignal::Hide => MiningRingPhase::Hidden,
        };
    }

    fn hide(&mut self) {
        self.phase = MiningRingPhase::Hidden;
    }

    fn advance(&mut self, delta_seconds: f32) {
        let MiningRingPhase::Completing { elapsed_seconds } = &mut self.phase else {
            return;
        };
        *elapsed_seconds += delta_seconds.max(0.0);
        if *elapsed_seconds >= COMPLETION_ANIMATION_SECONDS {
            self.hide();
        }
    }

    fn presentation(self) -> Option<MiningRingPresentation> {
        match self.phase {
            MiningRingPhase::Hidden => None,
            MiningRingPhase::Active(progress) => Some(MiningRingPresentation {
                filled_segments: active_segment_count(progress),
                scale: 1.0,
                opacity: 1.0,
            }),
            MiningRingPhase::Completing { elapsed_seconds } => {
                let phase = (elapsed_seconds / COMPLETION_ANIMATION_SECONDS).clamp(0.0, 1.0);
                let fade = 1.0 - ((phase - 0.58) / 0.42).clamp(0.0, 1.0);
                Some(MiningRingPresentation {
                    filled_segments: RING_SEGMENTS,
                    scale: 1.0 + phase.mul_add(PI, 0.0).sin() * 0.14,
                    opacity: fade,
                })
            }
        }
    }
}

fn active_segment_count(progress: MiningProgressV1) -> u8 {
    let visible_capacity = u64::from(RING_SEGMENTS - 1);
    let completed = u64::from(progress.completed_work());
    let required = u64::from(progress.required_work());
    let rounded_up = completed
        .saturating_mul(visible_capacity)
        .div_ceil(required);
    u8::try_from(rounded_up.clamp(1, visible_capacity)).unwrap_or(RING_SEGMENTS - 1)
}

fn edit_signal(receipt: &BlockEditReceiptV1) -> Option<MiningRingSignal> {
    if receipt.action != BlockEditActionV1::Break {
        return None;
    }
    Some(match &receipt.result {
        Ok(_) => MiningRingSignal::Completed,
        Err(BlockEditRejectV1::RequiresProgress { progress }) => {
            MiningRingSignal::Progress(*progress)
        }
        Err(_) => MiningRingSignal::Hide,
    })
}

fn retain_latest_signal(
    latest: &mut Option<((u64, u64, u8), MiningRingSignal)>,
    stamp: (u64, u64, u8),
    signal: MiningRingSignal,
) {
    if latest.as_ref().is_none_or(|(current, _)| stamp > *current) {
        *latest = Some((stamp, signal));
    }
}

/// Spawns the initially hidden ring around the crosshair center.
pub(super) fn spawn_mining_ring(parent: &mut bevy::ecs::hierarchy::ChildSpawnerCommands<'_>) {
    parent
        .spawn((
            ProductionMiningRing,
            Name::new("Mining progress ring"),
            Node {
                position_type: bevy::prelude::PositionType::Absolute,
                left: Val::Px(0.0),
                top: Val::Px(0.0),
                width: Val::Px(RING_DIAMETER_PX),
                height: Val::Px(RING_DIAMETER_PX),
                display: Display::None,
                ..Node::default()
            },
            UiTransform::default(),
            FocusPolicy::Pass,
            Pickable::IGNORE,
        ))
        .with_children(|ring| {
            for index in 0..RING_SEGMENTS {
                let angle = TAU * f32::from(index) / f32::from(RING_SEGMENTS) - FRAC_PI_2;
                let center = RING_DIAMETER_PX * 0.5;
                let left = center + angle.cos() * RING_RADIUS_PX - SEGMENT_WIDTH_PX * 0.5;
                let top = center + angle.sin() * RING_RADIUS_PX - SEGMENT_HEIGHT_PX * 0.5;
                ring.spawn((
                    ProductionMiningRingSegment(index),
                    Name::new("Mining progress ring segment"),
                    Node {
                        position_type: bevy::prelude::PositionType::Absolute,
                        left: Val::Px(left),
                        top: Val::Px(top),
                        width: Val::Px(SEGMENT_WIDTH_PX),
                        height: Val::Px(SEGMENT_HEIGHT_PX),
                        border_radius: BorderRadius::MAX,
                        ..Node::default()
                    },
                    UiTransform::from_rotation(Rot2::radians(angle + FRAC_PI_2)),
                    BackgroundColor(unfilled_color(1.0)),
                    FocusPolicy::Pass,
                    Pickable::IGNORE,
                ));
            }
        });
}

/// Projects newest local authoritative mining receipts into the ring.
#[allow(clippy::needless_pass_by_value)] // Bevy systems receive SystemParams by value.
#[allow(clippy::too_many_arguments)] // One projection owns receipt ordering and all ring nodes.
pub(super) fn sync_mining_ring(
    time: Res<'_, Time>,
    ownership: Res<'_, ClientInputOwnership>,
    players: Query<'_, '_, &D2Player, With<LocalPlayerInput>>,
    mut edit_receipts: MessageReader<'_, '_, BlockEditReceiptV1>,
    mut cancel_receipts: MessageReader<'_, '_, MiningCancelReceiptV1>,
    mut state: ResMut<'_, ProductionMiningRingState>,
    mut root: Query<'_, '_, (&mut Node, &mut UiTransform), With<ProductionMiningRing>>,
    mut segments: Query<'_, '_, (&ProductionMiningRingSegment, &mut BackgroundColor)>,
) {
    let local_player = players.single().ok().map(|player| player.player_id);
    let mut latest = None;
    for receipt in edit_receipts.read() {
        if Some(receipt.player) == local_player
            && let Some(signal) = edit_signal(receipt)
        {
            retain_latest_signal(
                &mut latest,
                (receipt.fixed_tick, receipt.input_generation, 0),
                signal,
            );
        }
    }
    for receipt in cancel_receipts.read() {
        if Some(receipt.player) == local_player {
            // Cancellation is emitted after release detection and wins a tie
            // with an edit receipt from the same fixed input generation.
            retain_latest_signal(
                &mut latest,
                (receipt.fixed_tick, receipt.input_generation, 1),
                MiningRingSignal::Hide,
            );
        }
    }

    if !ownership.owns_gameplay_input() || local_player.is_none() {
        state.hide();
    } else if let Some((_, signal)) = latest {
        state.apply(signal);
    }

    let Ok((mut root_node, mut root_transform)) = root.single_mut() else {
        state.advance(time.delta_secs());
        return;
    };
    let Some(presentation) = state.presentation() else {
        root_node.display = Display::None;
        root_transform.scale = Vec2::ONE;
        state.advance(time.delta_secs());
        return;
    };

    root_node.display = Display::Flex;
    root_transform.scale = Vec2::splat(presentation.scale);
    for (segment, mut background) in &mut segments {
        background.0 = if segment.0 < presentation.filled_segments {
            filled_color(presentation.opacity)
        } else {
            unfilled_color(presentation.opacity)
        };
    }
    state.advance(time.delta_secs());
}

fn filled_color(opacity: f32) -> Color {
    Color::srgba(0.38, 0.96, 0.74, 0.96 * opacity)
}

fn unfilled_color(opacity: f32) -> Color {
    Color::srgba(0.82, 0.88, 0.82, 0.16 * opacity)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn incomplete_progress_reserves_the_last_segment_for_authority() {
        let early = MiningProgressV1::new(1, 100).expect("one of one hundred is incomplete");
        let late = MiningProgressV1::new(99, 100).expect("ninety-nine is incomplete");
        assert_eq!(active_segment_count(early), 1);
        assert_eq!(active_segment_count(late), RING_SEGMENTS - 1);

        let mut state = ProductionMiningRingState::default();
        state.apply(MiningRingSignal::Progress(late));
        assert_eq!(
            state
                .presentation()
                .expect("active progress is visible")
                .filled_segments,
            RING_SEGMENTS - 1
        );
        state.apply(MiningRingSignal::Completed);
        assert_eq!(
            state
                .presentation()
                .expect("completion animation is visible")
                .filled_segments,
            RING_SEGMENTS
        );
    }

    #[test]
    fn completion_pulses_then_hides_after_its_bounded_animation() {
        let mut state = ProductionMiningRingState::default();
        state.apply(MiningRingSignal::Completed);
        state.advance(COMPLETION_ANIMATION_SECONDS * 0.5);
        let middle = state.presentation().expect("mid-animation ring is visible");
        assert!(middle.scale > 1.1);
        state.advance(COMPLETION_ANIMATION_SECONDS * 0.5);
        assert!(state.presentation().is_none());
    }

    #[test]
    fn newer_signals_replace_older_signals_and_cancel_wins_a_tie() {
        let progress = MiningProgressV1::new(2, 5).expect("fixture progress is incomplete");
        let mut latest = None;
        retain_latest_signal(&mut latest, (4, 7, 0), MiningRingSignal::Progress(progress));
        retain_latest_signal(&mut latest, (3, 9, 1), MiningRingSignal::Hide);
        assert_eq!(
            latest.map(|(_, signal)| signal),
            Some(MiningRingSignal::Progress(progress))
        );
        retain_latest_signal(&mut latest, (4, 7, 1), MiningRingSignal::Hide);
        assert_eq!(
            latest.map(|(_, signal)| signal),
            Some(MiningRingSignal::Hide)
        );
    }
}
