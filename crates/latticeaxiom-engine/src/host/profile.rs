//! Machine-readable V4/P4 streaming profile evidence.
//!
//! This is a correctness and coverage receipt for the current 32³ production
//! baseline. It does not claim the D2/D10 `desktop-reference-v1` working-set
//! gate and does not authorize P7 LOD or radius reduction.

use latticeaxiom_compose::PlayableWorldHardLimitsV1;
use latticeaxiom_worldgen::WorldgenConfigV1;
use serde::Serialize;

use super::stream::StreamClamps;
use super::worldgen::spine_config;

/// Schema identity for host-emitted streaming profile evidence.
pub const STREAMING_PROFILE_EVIDENCE_SCHEMA_V1: &str =
    "latticeaxiom:schema/streaming-profile-evidence@1";
/// ADR 0026 normative performance profile name.
pub const DESKTOP_REFERENCE_PROFILE_V1: &str = "desktop-reference-v1";
/// Evidence lifecycle. D10 freeze requires a reference-host 10-minute run.
pub const STREAMING_PROFILE_STATUS_D2_PROVISIONAL: &str = "d2-provisional";
/// ADR 0026 sizing premise chunk edge in voxels.
pub const ADR_0026_CHUNK_EDGE_VOXELS: u16 = 32;
/// ADR 0026 active world-space horizontal coverage in meters.
pub const ADR_0026_ACTIVE_COVERAGE_M: u32 = 128;
/// ADR 0026 resident world-space horizontal coverage in meters.
pub const ADR_0026_RESIDENT_COVERAGE_M: u32 = 192;
/// ADR 0026 active Chebyshev radius in 32³ chunks.
pub const ADR_0026_ACTIVE_RADIUS_CHUNKS: u32 = 4;
/// ADR 0026 resident Chebyshev radius in 32³ chunks.
pub const ADR_0026_RESIDENT_RADIUS_CHUNKS: u32 = 6;

/// Observed working-set occupancy copied from runtime diagnostics.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub struct StreamingProfileCountsV1 {
    /// Resident committed projections.
    pub resident: u32,
    /// Projections with mesh and collider last-applied keys.
    pub active: u32,
    /// Projections with a last-applied mesh key.
    pub visible: u32,
    /// Combined mesh and collider jobs currently in flight.
    pub in_flight: u32,
    /// Last desired interest set, including pins and prefetch.
    pub requested: u32,
    /// Dirty edited chunks that V4 must retain.
    pub dirty: u32,
    /// Combined reserved derived bytes.
    pub reserved_bytes: u64,
    /// Combined reserved-byte hard cap installed on this host.
    pub byte_budget: u64,
}

/// Coverage and count comparison between the live fixture and ADR 0026.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct StreamingProfileEvidenceV1 {
    /// Versioned evidence schema.
    pub schema: String,
    /// Evidence lifecycle status.
    pub status: String,
    /// Named performance profile.
    pub profile: String,
    /// Live cubic chunk edge in voxels.
    pub chunk_edge_voxels: u16,
    /// Inclusive world floor and ceiling in voxels.
    pub vertical_voxels: [i32; 2],
    /// Host capability cap for render-distance requests.
    pub view_distance_chunks: u32,
    /// Authored render-distance request before host admission.
    pub requested_render_distance_chunks: u32,
    /// Render-distance request admitted by the active host.
    pub admitted_render_distance_chunks: u32,
    /// Render radius supported by generation and resident budgets.
    pub effective_render_distance_chunks: u32,
    /// Radius in which authoritative simulation may tick.
    pub simulation_distance_chunks: u32,
    /// Base full-resolution resident radius.
    pub resident_distance_chunks: u32,
    /// Furthest directional prefetch distance.
    pub prefetch_distance_chunks: u32,
    /// Host generation radius in chunks.
    pub generation_radius_chunks: u32,
    /// Admitted Chebyshev interest radius after the resident budget clamp.
    pub interest_radius_chunks: u32,
    /// Hard active chunk cap.
    pub max_active_chunks: u32,
    /// Hard resident chunk cap.
    pub max_resident_chunks: u32,
    /// Hard in-flight chunk cap.
    pub max_in_flight_chunks: u32,
    /// World-space horizontal coverage of the live interest radius, in meters.
    pub world_space_interest_coverage_m: u32,
    /// World-space horizontal coverage of authoritative simulation, in meters.
    pub world_space_simulation_coverage_m: u32,
    /// World-space horizontal coverage of retained full-resolution chunks, in meters.
    pub world_space_resident_coverage_m: u32,
    /// World-space horizontal coverage of the live generation radius, in meters.
    pub world_space_generation_coverage_m: u32,
    /// ADR 0026 chunk-edge premise.
    pub adr_0026_chunk_edge_voxels: u16,
    /// ADR 0026 active coverage premise.
    pub adr_0026_active_coverage_m: u32,
    /// ADR 0026 resident coverage premise.
    pub adr_0026_resident_coverage_m: u32,
    /// Chebyshev radius that would match ADR 0026 active coverage at this edge.
    pub equivalent_active_radius_chunks: u32,
    /// Chebyshev radius that would match ADR 0026 resident coverage at this edge.
    pub equivalent_resident_radius_chunks: u32,
    /// Whether the live interest radius matches ADR 0026 world-space coverage.
    pub matches_adr_0026_world_space_coverage: bool,
    /// Whether this evidence may claim the D2/D10 working-set gate.
    pub claims_d2_working_set_gate: bool,
    /// Selected P4 profile choice.
    pub p4_choice: String,
    /// Observed occupancy.
    pub counts: StreamingProfileCountsV1,
    /// Operator-facing interpretation.
    pub notes: String,
}

impl StreamingProfileEvidenceV1 {
    /// Builds evidence from live clamps and occupancy.
    #[must_use]
    pub(super) fn capture(
        clamps: StreamClamps,
        config: &WorldgenConfigV1,
        counts: StreamingProfileCountsV1,
    ) -> Self {
        Self::from_parts(clamps, config, counts)
    }

    fn from_parts(
        clamps: StreamClamps,
        config: &WorldgenConfigV1,
        counts: StreamingProfileCountsV1,
    ) -> Self {
        let limits = clamps.hard_limits;
        let status = clamps.view_distance_status();
        let interest_radius = status.effective_render_distance().chunks();
        let edge = config.chunk_edge_voxels;
        let interest_coverage = coverage_m(interest_radius, edge);
        let simulation_coverage = coverage_m(status.simulation_distance().chunks(), edge);
        let resident_coverage = coverage_m(status.resident_distance().chunks(), edge);
        let generation_coverage = coverage_m(limits.generation_radius_chunks, edge);
        let equivalent_active = radius_for_coverage(ADR_0026_ACTIVE_COVERAGE_M, edge);
        let equivalent_resident = radius_for_coverage(ADR_0026_RESIDENT_COVERAGE_M, edge);
        let matches_coverage = simulation_coverage == ADR_0026_ACTIVE_COVERAGE_M
            && resident_coverage == ADR_0026_RESIDENT_COVERAGE_M
            && generation_coverage >= ADR_0026_RESIDENT_COVERAGE_M;
        Self {
            schema: STREAMING_PROFILE_EVIDENCE_SCHEMA_V1.to_owned(),
            status: STREAMING_PROFILE_STATUS_D2_PROVISIONAL.to_owned(),
            profile: DESKTOP_REFERENCE_PROFILE_V1.to_owned(),
            chunk_edge_voxels: edge,
            vertical_voxels: [config.world_floor_y, config.world_ceiling_y],
            view_distance_chunks: limits.view_distance_chunks,
            requested_render_distance_chunks: status.requested_render_distance().chunks(),
            admitted_render_distance_chunks: status.admitted_render_distance().chunks(),
            effective_render_distance_chunks: status.effective_render_distance().chunks(),
            simulation_distance_chunks: status.simulation_distance().chunks(),
            resident_distance_chunks: status.resident_distance().chunks(),
            prefetch_distance_chunks: status.prefetch_distance().chunks(),
            generation_radius_chunks: limits.generation_radius_chunks,
            interest_radius_chunks: interest_radius,
            max_active_chunks: limits.max_active_chunks,
            max_resident_chunks: limits.max_resident_chunks,
            max_in_flight_chunks: limits.max_in_flight_chunks,
            world_space_interest_coverage_m: interest_coverage,
            world_space_simulation_coverage_m: simulation_coverage,
            world_space_resident_coverage_m: resident_coverage,
            world_space_generation_coverage_m: generation_coverage,
            adr_0026_chunk_edge_voxels: ADR_0026_CHUNK_EDGE_VOXELS,
            adr_0026_active_coverage_m: ADR_0026_ACTIVE_COVERAGE_M,
            adr_0026_resident_coverage_m: ADR_0026_RESIDENT_COVERAGE_M,
            equivalent_active_radius_chunks: equivalent_active,
            equivalent_resident_radius_chunks: equivalent_resident,
            matches_adr_0026_world_space_coverage: matches_coverage,
            claims_d2_working_set_gate: false,
            p4_choice: "adopt-32-cubed-baseline".to_owned(),
            counts,
            notes: format!(
                "Live fixture uses {edge}³ chunks with simulation radius {} ({simulation_coverage} m), resident/render radius {} ({resident_coverage} m), and admitted interest radius {interest_radius} ({interest_coverage} m). ADR 0026 {ADR_0026_ACTIVE_COVERAGE_M} m / {ADR_0026_RESIDENT_COVERAGE_M} m coverage uses radii {ADR_0026_ACTIVE_RADIUS_CHUNKS} / {ADR_0026_RESIDENT_RADIUS_CHUNKS} at {ADR_0026_CHUNK_EDGE_VOXELS}³ and radii {equivalent_active} / {equivalent_resident} at this edge. Radius 1–2 is not a D2/D10 working-set pass. P7 LOD remains unauthorized.",
                status.simulation_distance().chunks(),
                status.resident_distance().chunks(),
            ),
        }
    }

    /// Evidence for the compiled V4 spine configuration without a live session.
    ///
    /// # Errors
    ///
    /// Returns [`super::ProductionHostError`] when the compiled stream clamps
    /// cannot be constructed from `limits`.
    pub fn for_spine_config(
        limits: PlayableWorldHardLimitsV1,
    ) -> Result<Self, super::ProductionHostError> {
        let config = spine_config();
        let clamps = StreamClamps::new(limits, &config)?;
        Ok(Self::capture(
            clamps,
            &config,
            StreamingProfileCountsV1::default(),
        ))
    }
}

/// World-space Chebyshev coverage of `radius` chunks of `edge` meters.
#[must_use]
#[allow(clippy::cast_lossless, reason = "u16 voxel edges fit in u32 meters")]
pub const fn coverage_m(radius_chunks: u32, edge_voxels: u16) -> u32 {
    radius_chunks.saturating_mul(edge_voxels as u32)
}

/// Smallest Chebyshev radius whose coverage is at least `coverage_m`.
#[must_use]
#[allow(clippy::cast_lossless, reason = "u16 voxel edges fit in u32 meters")]
pub const fn radius_for_coverage(coverage_m: u32, edge_voxels: u16) -> u32 {
    let edge = if edge_voxels == 0 {
        1
    } else {
        edge_voxels as u32
    };
    coverage_m.div_ceil(edge)
}

#[cfg(test)]
mod tests {
    use latticeaxiom_compose::PlayableWorldHardLimitsV1;

    use super::{
        ADR_0026_ACTIVE_COVERAGE_M, ADR_0026_ACTIVE_RADIUS_CHUNKS, ADR_0026_CHUNK_EDGE_VOXELS,
        ADR_0026_RESIDENT_COVERAGE_M, ADR_0026_RESIDENT_RADIUS_CHUNKS, StreamingProfileEvidenceV1,
        coverage_m, radius_for_coverage,
    };

    #[test]
    fn adr_0026_coverage_math_is_stable_for_32_and_8_cubed() {
        assert_eq!(
            coverage_m(ADR_0026_ACTIVE_RADIUS_CHUNKS, ADR_0026_CHUNK_EDGE_VOXELS),
            ADR_0026_ACTIVE_COVERAGE_M
        );
        assert_eq!(
            coverage_m(ADR_0026_RESIDENT_RADIUS_CHUNKS, ADR_0026_CHUNK_EDGE_VOXELS),
            ADR_0026_RESIDENT_COVERAGE_M
        );
        assert_eq!(radius_for_coverage(ADR_0026_ACTIVE_COVERAGE_M, 8), 16);
        assert_eq!(radius_for_coverage(ADR_0026_RESIDENT_COVERAGE_M, 8), 24);
        assert_eq!(coverage_m(2, 8), 16);
    }

    #[test]
    fn thirty_two_cubed_baseline_reports_matching_coverage_without_claiming_the_d2_gate() {
        let limits =
            PlayableWorldHardLimitsV1::new(32, 32, 405, 1_183, 8, 4).expect("nonzero clamps");
        let evidence = StreamingProfileEvidenceV1::for_spine_config(limits).expect("clamps");
        assert_eq!(evidence.chunk_edge_voxels, 32);
        assert_eq!(evidence.requested_render_distance_chunks, 8);
        assert_eq!(evidence.admitted_render_distance_chunks, 8);
        assert_eq!(evidence.effective_render_distance_chunks, 6);
        assert_eq!(evidence.simulation_distance_chunks, 4);
        assert_eq!(evidence.resident_distance_chunks, 6);
        assert_eq!(evidence.prefetch_distance_chunks, 7);
        assert_eq!(evidence.max_active_chunks, 405);
        assert_eq!(evidence.max_resident_chunks, 1_183);
        assert_eq!(evidence.world_space_simulation_coverage_m, 128);
        assert_eq!(evidence.world_space_resident_coverage_m, 192);
        assert_eq!(evidence.equivalent_active_radius_chunks, 4);
        assert_eq!(evidence.equivalent_resident_radius_chunks, 6);
        assert!(evidence.matches_adr_0026_world_space_coverage);
        assert!(!evidence.claims_d2_working_set_gate);
        assert_eq!(evidence.p4_choice, "adopt-32-cubed-baseline");
        assert_eq!(evidence.status, "d2-provisional");
        let encoded = serde_json::to_value(&evidence).expect("evidence is JSON");
        assert_eq!(
            encoded["schema"],
            "latticeaxiom:schema/streaming-profile-evidence@1"
        );
        assert_eq!(encoded["counts"]["resident"], 0);
    }
}
