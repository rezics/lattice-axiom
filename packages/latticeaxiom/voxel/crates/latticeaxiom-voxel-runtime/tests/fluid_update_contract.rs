//! Public revision/DDA hooks used by fluid update admission.
//!
//! Integrator wiring: re-export `fluid` from `lib.rs` so bounded water/lava
//! tick, queue, in-flight, and stale-revision tests can move here from the
//! crate-private module.

use latticeaxiom_storage::{ChunkCoordinate, ChunkRevision, VoxelRevision, WorldRevision};
use latticeaxiom_voxel_runtime::{
    FluidFlow, FluidLayerCell, FluidRevisionStamp, FluidRuntimeError, FluidUpdateBudget,
    SolidFluidRuntimeCell, StaleReason, admit_fluid_completion, plan_fluid_tick,
};
use std::num::NonZeroU32;

#[test]
fn stale_reason_discriminants_remain_stable_for_fluid_admission() {
    assert_ne!(StaleReason::WorldRevision, StaleReason::ChunkRevision);
    assert_ne!(StaleReason::ChunkRevision, StaleReason::VoxelRevision);
    let world = WorldRevision::new(4);
    let chunk = ChunkRevision::new(2);
    let voxel = VoxelRevision::new(2);
    assert!(world.get() > chunk.get());
    assert_eq!(chunk.get(), voxel.get());
}

#[test]
fn public_plan_spreads_down_and_stale_admission_rejects() {
    let captured = FluidRevisionStamp::new(
        WorldRevision::new(1),
        ChunkRevision::new(1),
        VoxelRevision::new(1),
    );
    let id: latticeaxiom_core::StableId = "fixture:fluid/alpha"
        .parse()
        .expect("fixture fluid identity");
    let mut cells = vec![SolidFluidRuntimeCell::empty(); 32 * 32 * 32];
    cells[1] = SolidFluidRuntimeCell {
        solid_occupied: false,
        fluid: FluidLayerCell::Fluid {
            id,
            level: 0,
            flow: FluidFlow::Still,
        },
    };
    let budget = FluidUpdateBudget::new(
        NonZeroU32::new(4096).expect("cells"),
        NonZeroU32::new(4096).expect("queue"),
        NonZeroU32::new(256 * 1024).expect("bytes"),
    );
    let plan = plan_fluid_tick(
        ChunkCoordinate::new(0, 0, 0),
        captured,
        &cells,
        &[(1, 0, 0)],
        budget,
    )
    .expect("plan");
    assert!(plan.cells_changed() > 0 || !plan.boundaries().is_empty());
    let newer = FluidRevisionStamp::new(
        WorldRevision::new(1),
        ChunkRevision::new(2),
        VoxelRevision::new(2),
    );
    assert!(matches!(
        admit_fluid_completion(plan.captured(), newer),
        Err(FluidRuntimeError::Stale {
            reason: StaleReason::ChunkRevision
        })
    ));
}
