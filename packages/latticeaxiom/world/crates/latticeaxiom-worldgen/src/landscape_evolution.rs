//! Bounded deterministic stream-power incision and hillslope diffusion.
//!
//! The v1 subset fixes the stream-power exponents at `m = 1/2`, `n = 1`.
//! Channel elevations are updated in downstream-to-upstream order with the
//! linear implicit `FastScape` form. Hillslopes use a two-buffer, fixed-boundary
//! four-neighbor diffusion sweep. Routing is rebuilt only after a closed outer
//! iteration, never during a sweep.

use std::{
    collections::{BTreeMap, BTreeSet},
    mem::size_of,
    sync::atomic::{AtomicBool, Ordering},
};

use bevy::tasks::TaskPool;
use latticeaxiom_core::canonical_json_bytes;
use serde::{Deserialize, Serialize};

use crate::{
    HydrologicDomainIdV1, HydrologicDomainInputV1, HydrologicDomainPlanV1,
    HydrologicTopologyConfigV1, HydrologicTopologyPlanV1, LandscapeEvolutionConfigHashV1,
    LandscapeEvolutionPlanHashV1, RiverSegmentIdV1, WorldgenError, WorldgenResult,
    build_hydrologic_topology_v1, hashes::domain_hash, plan_hydrologic_domain_with_cancellation_v1,
};

const LANDSCAPE_ALGORITHM: &str =
    "latticeaxiom:landscape-evolution/implicit-stream-power-linear-diffusion@1";
const LANDSCAPE_PROVENANCE_DOMAIN: &[u8] = b"latticeaxiom.landscape-evolution.provenance.v1\0";
const LANDSCAPE_CONFIG_DOMAIN: &[u8] = b"latticeaxiom.landscape-evolution.config.v1\0";
const LANDSCAPE_PLAN_DOMAIN: &[u8] = b"latticeaxiom.landscape-evolution.plan.v1\0";
const Q16_ONE: u64 = 65_536;
const MAX_OUTER_ITERATIONS: u16 = 64;
const MAX_INCISION_SWEEPS: u16 = 16;
const MAX_DIFFUSION_SWEEPS: u16 = 64;
const MAX_UPLIFT_Q8: i32 = 256;
const CANCELLATION_INTERVAL: u64 = 1_024;

/// Closed fixed-point policy and hard limits for one landscape evolution run.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LandscapeEvolutionConfigV1 {
    outer_iterations: u16,
    incision_sweeps: u16,
    diffusion_sweeps: u16,
    time_step_q16: u32,
    uplift_q8_per_step: i32,
    erodibility_q32: u32,
    diffusivity_q16: u32,
    convergence_q8: u32,
    max_work_units: u64,
    max_temporary_bytes: u64,
    max_result_bytes: u64,
}

impl Default for LandscapeEvolutionConfigV1 {
    fn default() -> Self {
        Self {
            outer_iterations: 4,
            incision_sweeps: 1,
            diffusion_sweeps: 2,
            time_step_q16: 65_536,
            uplift_q8_per_step: 2,
            erodibility_q32: 1 << 20,
            diffusivity_q16: 4_096,
            convergence_q8: 1,
            max_work_units: 100_000_000,
            max_temporary_bytes: 512 * 1024 * 1024,
            max_result_bytes: 1_024 * 1024 * 1024,
        }
    }
}

impl LandscapeEvolutionConfigV1 {
    /// Creates one closed landscape-evolution policy.
    #[allow(
        clippy::too_many_arguments,
        reason = "the constructor closes solver behavior and every resource limit"
    )]
    #[must_use]
    pub const fn new(
        outer_iterations: u16,
        incision_sweeps: u16,
        diffusion_sweeps: u16,
        time_step_q16: u32,
        uplift_q8_per_step: i32,
        erodibility_q32: u32,
        diffusivity_q16: u32,
        convergence_q8: u32,
        max_work_units: u64,
        max_temporary_bytes: u64,
        max_result_bytes: u64,
    ) -> Self {
        Self {
            outer_iterations,
            incision_sweeps,
            diffusion_sweeps,
            time_step_q16,
            uplift_q8_per_step,
            erodibility_q32,
            diffusivity_q16,
            convergence_q8,
            max_work_units,
            max_temporary_bytes,
            max_result_bytes,
        }
    }

    /// Returns the maximum closed rerouting iterations.
    #[must_use]
    pub const fn outer_iterations(&self) -> u16 {
        self.outer_iterations
    }

    /// Returns the deterministic channel sweeps per outer iteration.
    #[must_use]
    pub const fn incision_sweeps(&self) -> u16 {
        self.incision_sweeps
    }

    /// Returns the deterministic diffusion sweeps per outer iteration.
    #[must_use]
    pub const fn diffusion_sweeps(&self) -> u16 {
        self.diffusion_sweeps
    }

    /// Returns the closed Q16 timestep.
    #[must_use]
    pub const fn time_step_q16(&self) -> u32 {
        self.time_step_q16
    }

    /// Returns the canonical config hash.
    ///
    /// # Errors
    ///
    /// Returns a canonical-encoding error if this config cannot encode.
    pub fn canonical_hash(&self) -> WorldgenResult<LandscapeEvolutionConfigHashV1> {
        let bytes =
            canonical_json_bytes(self).map_err(|error| WorldgenError::CanonicalEncoding {
                kind: "LandscapeEvolutionConfigV1",
                reason: error.to_string(),
            })?;
        Ok(LandscapeEvolutionConfigHashV1::from_hash(domain_hash(
            LANDSCAPE_CONFIG_DOMAIN,
            &[&bytes],
        )))
    }

    fn validate(&self, domain: &HydrologicDomainInputV1) -> WorldgenResult<u64> {
        if self.outer_iterations > MAX_OUTER_ITERATIONS {
            return invalid(
                "landscape.outer_iterations",
                format!("must be <= {MAX_OUTER_ITERATIONS}"),
            );
        }
        if self.outer_iterations > 0 && (self.incision_sweeps == 0 || self.time_step_q16 == 0) {
            return invalid(
                "landscape.solver",
                "enabled evolution requires a nonzero timestep and incision sweep count",
            );
        }
        if self.incision_sweeps > MAX_INCISION_SWEEPS
            || self.diffusion_sweeps > MAX_DIFFUSION_SWEEPS
        {
            return invalid(
                "landscape.sweeps",
                format!(
                    "incision must be <= {MAX_INCISION_SWEEPS} and diffusion <= {MAX_DIFFUSION_SWEEPS}"
                ),
            );
        }
        if !(0..=MAX_UPLIFT_Q8).contains(&self.uplift_q8_per_step) {
            return invalid(
                "landscape.uplift_q8_per_step",
                format!("must be in 0..={MAX_UPLIFT_Q8}"),
            );
        }
        if self.max_work_units == 0 || self.max_temporary_bytes == 0 || self.max_result_bytes == 0 {
            return invalid(
                "landscape.limits",
                "work, temporary-byte, and result-byte limits must be nonzero",
            );
        }
        let spacing = u64::from(domain.grid().spacing_voxels().get());
        let spacing_squared =
            spacing
                .checked_mul(spacing)
                .ok_or(WorldgenError::ArithmeticOverflow {
                    operation: "landscape spacing squared",
                })?;
        let beta_q16 = u64::from(self.diffusivity_q16)
            .checked_mul(u64::from(self.time_step_q16))
            .and_then(|value| value.checked_div(Q16_ONE.checked_mul(spacing_squared)?))
            .ok_or(WorldgenError::ArithmeticOverflow {
                operation: "landscape diffusion stability coefficient",
            })?;
        if beta_q16 > Q16_ONE / 4 {
            return invalid(
                "landscape.diffusivity_q16",
                "explicit four-neighbor diffusion requires D*dt/dx^2 <= 1/4",
            );
        }
        Ok(beta_q16)
    }
}

/// Complete immutable request for one bounded evolution run.
#[derive(Clone, Debug)]
pub struct LandscapeEvolutionInputV1 {
    domain: HydrologicDomainInputV1,
    topology: HydrologicTopologyConfigV1,
    evolution: LandscapeEvolutionConfigV1,
}

impl LandscapeEvolutionInputV1 {
    /// Creates a request from the original DEM and closed solver policies.
    #[must_use]
    pub const fn new(
        domain: HydrologicDomainInputV1,
        topology: HydrologicTopologyConfigV1,
        evolution: LandscapeEvolutionConfigV1,
    ) -> Self {
        Self {
            domain,
            topology,
            evolution,
        }
    }

    /// Returns the stable finite-domain identity.
    #[must_use]
    pub fn domain_id(&self) -> HydrologicDomainIdV1 {
        self.domain.domain_id()
    }

    /// Returns the original hydrologic-domain input.
    #[must_use]
    pub const fn domain(&self) -> &HydrologicDomainInputV1 {
        &self.domain
    }

    /// Returns the closed evolution config.
    #[must_use]
    pub const fn evolution(&self) -> &LandscapeEvolutionConfigV1 {
        &self.evolution
    }
}

/// Deterministic work, convergence, and mass-change diagnostics.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LandscapeEvolutionAccountingV1 {
    iterations_completed: u16,
    reroutes_completed: u16,
    incision_updates: u64,
    diffusion_updates: u64,
    work_units: u64,
    temporary_bytes_peak: u64,
    max_delta_q8: u32,
    total_incision_q8: u64,
    total_uplift_q8: u64,
    diffusion_net_change_q8: i64,
    result_bytes: u64,
}

impl LandscapeEvolutionAccountingV1 {
    /// Returns the number of completed closed outer iterations.
    #[must_use]
    pub const fn iterations_completed(self) -> u16 {
        self.iterations_completed
    }

    /// Returns the number of post-update hydrology reroutes.
    #[must_use]
    pub const fn reroutes_completed(self) -> u16 {
        self.reroutes_completed
    }

    /// Returns the maximum final per-iteration elevation change in Q8.
    #[must_use]
    pub const fn max_delta_q8(self) -> u32 {
        self.max_delta_q8
    }

    /// Returns total channel incision in elevation-Q8 cell units.
    #[must_use]
    pub const fn total_incision_q8(self) -> u64 {
        self.total_incision_q8
    }

    /// Returns exact deterministic evolution and nested-routing work units.
    #[must_use]
    pub const fn work_units(self) -> u64 {
        self.work_units
    }

    /// Returns stabilized canonical result bytes.
    #[must_use]
    pub const fn result_bytes(self) -> u64 {
        self.result_bytes
    }
}

/// Compact evidence retained when an evolved domain is embedded elsewhere.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LandscapeEvolutionEvidenceV1 {
    config_hash: LandscapeEvolutionConfigHashV1,
    initial_domain_hash: crate::HydrologicDomainPlanHashV1,
    initial_topology_hash: crate::HydrologicTopologyHashV1,
    initial_elevation_q8: Vec<i32>,
    accounting: LandscapeEvolutionAccountingV1,
}

impl LandscapeEvolutionEvidenceV1 {
    /// Returns the exact closed evolution-policy hash.
    #[must_use]
    pub const fn config_hash(&self) -> LandscapeEvolutionConfigHashV1 {
        self.config_hash
    }

    /// Returns the untouched initial semantic DEM.
    #[must_use]
    pub fn initial_elevation_q8(&self) -> &[i32] {
        &self.initial_elevation_q8
    }

    /// Returns deterministic evolution accounting.
    #[must_use]
    pub const fn accounting(&self) -> LandscapeEvolutionAccountingV1 {
        self.accounting
    }
}

/// Immutable evolved DEM together with its final rerouted hydrology topology.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LandscapeEvolutionPlanV1 {
    algorithm: &'static str,
    evidence: LandscapeEvolutionEvidenceV1,
    final_domain: HydrologicDomainPlanV1,
    final_topology: HydrologicTopologyPlanV1,
}

impl LandscapeEvolutionPlanV1 {
    /// Returns the compact evolution evidence.
    #[must_use]
    pub const fn evidence(&self) -> &LandscapeEvolutionEvidenceV1 {
        &self.evidence
    }

    /// Returns the final depression-corrected and rerouted domain.
    #[must_use]
    pub const fn final_domain(&self) -> &HydrologicDomainPlanV1 {
        &self.final_domain
    }

    /// Returns the final connected river/lake/outlet topology.
    #[must_use]
    pub const fn final_topology(&self) -> &HydrologicTopologyPlanV1 {
        &self.final_topology
    }

    /// Consumes this artifact into embedding-friendly parts.
    #[must_use]
    pub fn into_parts(
        self,
    ) -> (
        HydrologicDomainPlanV1,
        HydrologicTopologyPlanV1,
        LandscapeEvolutionEvidenceV1,
    ) {
        (self.final_domain, self.final_topology, self.evidence)
    }

    /// Returns stable persisted bytes.
    ///
    /// # Errors
    ///
    /// Returns a canonical-encoding error if this plan cannot encode.
    pub fn canonical_bytes(&self) -> WorldgenResult<Vec<u8>> {
        canonical_json_bytes(self).map_err(|error| WorldgenError::CanonicalEncoding {
            kind: "LandscapeEvolutionPlanV1",
            reason: error.to_string(),
        })
    }

    /// Returns the canonical evolved-plan hash.
    ///
    /// # Errors
    ///
    /// Returns a canonical-encoding error if this plan cannot encode.
    pub fn canonical_hash(&self) -> WorldgenResult<LandscapeEvolutionPlanHashV1> {
        Ok(LandscapeEvolutionPlanHashV1::from_hash(domain_hash(
            LANDSCAPE_PLAN_DOMAIN,
            &[&self.canonical_bytes()?],
        )))
    }
}

#[derive(Debug)]
struct EvolutionState<'a> {
    config: &'a LandscapeEvolutionConfigV1,
    cancellation: Option<&'a AtomicBool>,
    accounting: LandscapeEvolutionAccountingV1,
}

impl<'a> EvolutionState<'a> {
    fn new(
        config: &'a LandscapeEvolutionConfigV1,
        cancellation: Option<&'a AtomicBool>,
        sample_count: usize,
    ) -> WorldgenResult<Self> {
        let per_sample = size_of::<i32>() * 3
            + size_of::<bool>()
            + size_of::<usize>() * 3
            + size_of::<Option<u32>>();
        let temporary = sample_count
            .checked_mul(per_sample)
            .and_then(|bytes| u64::try_from(bytes).ok())
            .ok_or(WorldgenError::ArithmeticOverflow {
                operation: "landscape temporary byte preflight",
            })?;
        if temporary > config.max_temporary_bytes {
            return Err(WorldgenError::BudgetExceeded {
                budget: "landscape temporary bytes",
                required: temporary,
                limit: config.max_temporary_bytes,
            });
        }
        let state = Self {
            config,
            cancellation,
            accounting: LandscapeEvolutionAccountingV1 {
                temporary_bytes_peak: temporary,
                ..LandscapeEvolutionAccountingV1::default()
            },
        };
        state.check_cancelled()?;
        Ok(state)
    }

    fn work(&mut self, units: u64) -> WorldgenResult<()> {
        let previous = self.accounting.work_units;
        self.accounting.work_units =
            previous
                .checked_add(units)
                .ok_or(WorldgenError::ArithmeticOverflow {
                    operation: "landscape work accounting",
                })?;
        if self.accounting.work_units > self.config.max_work_units {
            return Err(WorldgenError::BudgetExceeded {
                budget: "landscape work units",
                required: self.accounting.work_units,
                limit: self.config.max_work_units,
            });
        }
        if previous / CANCELLATION_INTERVAL != self.accounting.work_units / CANCELLATION_INTERVAL {
            self.check_cancelled()?;
        }
        Ok(())
    }

    fn check_cancelled(&self) -> WorldgenResult<()> {
        if self
            .cancellation
            .is_some_and(|flag| flag.load(Ordering::Relaxed))
        {
            return Err(WorldgenError::HydrologicPlanningCancelled {
                completed_work_units: self.accounting.work_units,
            });
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug)]
struct ChannelNode {
    cell: usize,
    downstream_cell: usize,
    discharge_q16: u64,
    length_q8: u64,
}

/// Evolves one finite DEM and validates hydrology after every closed iteration.
///
/// # Errors
///
/// Returns validation, arithmetic, cancellation, routing, topology, or hard
/// resource-limit errors. No partial artifact is returned.
pub fn evolve_hydrologic_landscape_v1(
    input: &LandscapeEvolutionInputV1,
) -> WorldgenResult<LandscapeEvolutionPlanV1> {
    evolve_hydrologic_landscape_with_cancellation_v1(input, None)
}

/// Evolves one finite DEM while observing a caller-owned cancellation flag.
///
/// # Errors
///
/// Returns validation, arithmetic, cancellation, routing, topology, or hard
/// resource-limit errors. No partial artifact is returned.
pub fn evolve_hydrologic_landscape_with_cancellation_v1(
    input: &LandscapeEvolutionInputV1,
    cancellation: Option<&AtomicBool>,
) -> WorldgenResult<LandscapeEvolutionPlanV1> {
    let beta_q16 = input.evolution.validate(&input.domain)?;
    let config_hash = input.evolution.canonical_hash()?;
    let topology_config_hash = input.topology.canonical_hash()?;
    let source_hash = input.domain.input_hash()?;
    let provenance = domain_hash(
        LANDSCAPE_PROVENANCE_DOMAIN,
        &[
            source_hash.as_bytes(),
            config_hash.as_bytes(),
            topology_config_hash.as_bytes(),
        ],
    );
    let mut domain = plan_hydrologic_domain_with_cancellation_v1(&input.domain, cancellation)?;
    let mut topology = build_hydrologic_topology_v1(&domain, &input.topology)?;
    let initial_domain_hash = domain.canonical_hash()?;
    let initial_topology_hash = topology.canonical_hash()?;
    let initial_elevation = input.domain.initial_elevation_q8().to_vec();
    let mut elevation = initial_elevation.clone();
    let mut scratch = elevation.clone();
    let mut state = EvolutionState::new(
        &input.evolution,
        cancellation,
        input.domain.grid().sample_count(),
    )?;
    state.work(domain.accounting().work_units())?;

    for _ in 0..input.evolution.outer_iterations {
        let before = elevation.clone();
        let graph = channel_graph(&domain, &topology)?;
        let mut fixed = fixed_mask(&input.domain, &topology)?;
        apply_uplift(&mut elevation, &fixed, &mut state)?;
        for _ in 0..input.evolution.incision_sweeps {
            apply_implicit_incision(&mut elevation, &graph, &fixed, &mut state)?;
        }
        for (cell, position) in topology
            .channel_segment_positions_by_cell()
            .iter()
            .enumerate()
        {
            if position.is_some() {
                fixed[cell] = true;
            }
        }
        for _ in 0..input.evolution.diffusion_sweeps {
            diffuse_once(
                &mut elevation,
                &mut scratch,
                input.domain.grid(),
                &fixed,
                beta_q16,
                &mut state,
            )?;
        }
        let max_delta = maximum_absolute_delta(&before, &elevation)?;
        state.accounting.max_delta_q8 = max_delta;
        state.accounting.iterations_completed =
            state.accounting.iterations_completed.checked_add(1).ok_or(
                WorldgenError::ArithmeticOverflow {
                    operation: "landscape iteration accounting",
                },
            )?;

        let revised = input
            .domain
            .with_revised_elevation(elevation.clone(), provenance)?;
        domain = plan_hydrologic_domain_with_cancellation_v1(&revised, cancellation)?;
        state.work(domain.accounting().work_units())?;
        topology = build_hydrologic_topology_v1(&domain, &input.topology)?;
        state.accounting.reroutes_completed =
            state.accounting.reroutes_completed.checked_add(1).ok_or(
                WorldgenError::ArithmeticOverflow {
                    operation: "landscape reroute accounting",
                },
            )?;
        state.work(u64::from(topology.accounting().segment_count()))?;
        if max_delta <= input.evolution.convergence_q8 {
            break;
        }
    }

    let evidence = LandscapeEvolutionEvidenceV1 {
        config_hash,
        initial_domain_hash,
        initial_topology_hash,
        initial_elevation_q8: initial_elevation,
        accounting: state.accounting,
    };
    let mut plan = LandscapeEvolutionPlanV1 {
        algorithm: LANDSCAPE_ALGORITHM,
        evidence,
        final_domain: domain,
        final_topology: topology,
    };
    stabilize_result_bytes(&mut plan, input.evolution.max_result_bytes)?;
    Ok(plan)
}

/// Evolves independent domains on a Bevy task pool and returns a stable merge.
///
/// Incision within one domain is downstream-dependent. Parallelism is applied
/// across independently bounded domains; result order never follows task
/// completion order.
#[must_use]
pub fn evolve_hydrologic_landscapes_parallel_v1(
    pool: &TaskPool,
    mut inputs: Vec<LandscapeEvolutionInputV1>,
) -> Vec<(
    HydrologicDomainIdV1,
    WorldgenResult<LandscapeEvolutionPlanV1>,
)> {
    inputs.sort_by_key(LandscapeEvolutionInputV1::domain_id);
    let mut results = pool.scope_with_executor(false, None, |scope| {
        for input in inputs {
            scope.spawn(async move {
                let id = input.domain_id();
                (id, evolve_hydrologic_landscape_v1(&input))
            });
        }
    });
    results.sort_by_key(|(id, _)| *id);
    results
}

#[allow(
    clippy::too_many_lines,
    reason = "the bounded adapter validates raster, identity, endpoint, and topological-order invariants together"
)]
fn channel_graph(
    domain: &HydrologicDomainPlanV1,
    topology: &HydrologicTopologyPlanV1,
) -> WorldgenResult<Vec<ChannelNode>> {
    let segments = topology.segments();
    let mut cell_by_position = vec![usize::MAX; segments.len()];
    for (cell, position) in topology
        .channel_segment_positions_by_cell()
        .iter()
        .enumerate()
    {
        let Some(position) = position else {
            continue;
        };
        let position =
            usize::try_from(*position).map_err(|_| WorldgenError::ArithmeticOverflow {
                operation: "landscape channel position conversion",
            })?;
        let slot = cell_by_position.get_mut(position).ok_or_else(|| {
            WorldgenError::InvalidHydrologicDomain {
                field: "landscape.channel",
                reason: "channel raster position lies outside segment storage".to_owned(),
            }
        })?;
        if *slot != usize::MAX {
            return invalid(
                "landscape.channel",
                "two raster cells reference the same v1 channel segment",
            );
        }
        *slot = cell;
    }
    if cell_by_position.contains(&usize::MAX) {
        return invalid(
            "landscape.channel",
            "a v1 channel segment has no source raster cell",
        );
    }
    let position_by_id = segments
        .iter()
        .enumerate()
        .map(|(position, segment)| (segment.id(), position))
        .collect::<BTreeMap<_, _>>();
    let mut node_by_position = Vec::with_capacity(segments.len());
    for (position, segment) in segments.iter().enumerate() {
        let downstream_cell = if let Some(downstream) = segment.downstream() {
            let downstream_position = *position_by_id.get(&downstream).ok_or_else(|| {
                WorldgenError::InvalidHydrologicDomain {
                    field: "landscape.channel",
                    reason: "segment downstream identity is missing".to_owned(),
                }
            })?;
            cell_by_position[downstream_position]
        } else {
            let target = segment.centerline().get(1).ok_or_else(|| {
                WorldgenError::InvalidHydrologicDomain {
                    field: "landscape.channel",
                    reason: "v1 segment is missing its downstream profile point".to_owned(),
                }
            })?;
            let coordinate = domain
                .grid()
                .nearest_coordinate(target.world_x(), target.world_z())
                .ok_or_else(|| WorldgenError::InvalidHydrologicDomain {
                    field: "landscape.channel",
                    reason: "terminal profile point lies outside the source grid".to_owned(),
                })?;
            domain.grid().index_of(coordinate).ok_or_else(|| {
                WorldgenError::InvalidHydrologicDomain {
                    field: "landscape.channel",
                    reason: "terminal coordinate has no row-major index".to_owned(),
                }
            })?
        };
        node_by_position.push(ChannelNode {
            cell: cell_by_position[position],
            downstream_cell,
            discharge_q16: segment.discharge_q16(),
            length_q8: segment.length_q8().max(1),
        });
    }

    let mut ready = BTreeSet::<(RiverSegmentIdV1, usize)>::new();
    for (position, segment) in segments.iter().enumerate() {
        if segment.downstream().is_none() {
            ready.insert((segment.id(), position));
        }
    }
    let mut ordered = Vec::with_capacity(segments.len());
    while let Some(entry) = ready.pop_first() {
        let position = entry.1;
        ordered.push(node_by_position[position]);
        for upstream in segments[position].upstream() {
            let upstream_position = *position_by_id.get(upstream).ok_or_else(|| {
                WorldgenError::InvalidHydrologicDomain {
                    field: "landscape.channel",
                    reason: "segment upstream identity is missing".to_owned(),
                }
            })?;
            ready.insert((*upstream, upstream_position));
        }
    }
    if ordered.len() != segments.len() {
        return invalid(
            "landscape.channel",
            "single-receiver channel graph is cyclic or disconnected from a terminal",
        );
    }
    Ok(ordered)
}

fn fixed_mask(
    input: &HydrologicDomainInputV1,
    topology: &HydrologicTopologyPlanV1,
) -> WorldgenResult<Vec<bool>> {
    let grid = input.grid();
    let width = usize::from(grid.width().get());
    let height = usize::from(grid.height().get());
    let halo = usize::from(grid.halo_samples()).max(1);
    if halo.saturating_mul(2) >= width || halo.saturating_mul(2) >= height {
        return invalid(
            "landscape.grid.halo_samples",
            "fixed halo must leave at least one evolvable core sample",
        );
    }
    let mut fixed = vec![false; grid.sample_count()];
    for z in 0..height {
        for x in 0..width {
            if x < halo || z < halo || x >= width - halo || z >= height - halo {
                fixed[x + z * width] = true;
            }
        }
    }
    if topology.channel_segment_positions_by_cell().len() != fixed.len() {
        return invalid(
            "landscape.channel",
            "topology raster length differs from the domain grid",
        );
    }
    Ok(fixed)
}

fn apply_uplift(
    elevation: &mut [i32],
    fixed: &[bool],
    state: &mut EvolutionState<'_>,
) -> WorldgenResult<()> {
    if state.config.uplift_q8_per_step == 0 {
        return Ok(());
    }
    for (height, &is_fixed) in elevation.iter_mut().zip(fixed) {
        if is_fixed {
            continue;
        }
        *height = height.checked_add(state.config.uplift_q8_per_step).ok_or(
            WorldgenError::ArithmeticOverflow {
                operation: "landscape uplift elevation",
            },
        )?;
        state.accounting.total_uplift_q8 = state
            .accounting
            .total_uplift_q8
            .checked_add(state.config.uplift_q8_per_step.cast_unsigned().into())
            .ok_or(WorldgenError::ArithmeticOverflow {
                operation: "landscape uplift accounting",
            })?;
        state.work(1)?;
    }
    Ok(())
}

fn apply_implicit_incision(
    elevation: &mut [i32],
    graph: &[ChannelNode],
    fixed: &[bool],
    state: &mut EvolutionState<'_>,
) -> WorldgenResult<()> {
    for node in graph {
        if fixed[node.cell] {
            continue;
        }
        let current = i64::from(elevation[node.cell]);
        let receiver = i64::from(elevation[node.downstream_cell]).min(current);
        let alpha_q16 = incision_alpha_q16(
            state.config.erodibility_q32,
            node.discharge_q16,
            state.config.time_step_q16,
            node.length_q8,
        )?;
        let numerator = i128::from(current)
            .checked_mul(i128::from(Q16_ONE))
            .and_then(|value| {
                i128::from(alpha_q16)
                    .checked_mul(i128::from(receiver))
                    .and_then(|weighted| value.checked_add(weighted))
            })
            .ok_or(WorldgenError::ArithmeticOverflow {
                operation: "landscape implicit incision numerator",
            })?;
        let denominator =
            Q16_ONE
                .checked_add(alpha_q16)
                .ok_or(WorldgenError::ArithmeticOverflow {
                    operation: "landscape implicit incision denominator",
                })?;
        let evolved = round_divide_i128(numerator, i128::from(denominator))?
            .clamp(i128::from(receiver), i128::from(current));
        let evolved = i32::try_from(evolved).map_err(|_| WorldgenError::ArithmeticOverflow {
            operation: "landscape implicit incision elevation",
        })?;
        let incision = current.abs_diff(i64::from(evolved));
        elevation[node.cell] = evolved;
        state.accounting.total_incision_q8 = state
            .accounting
            .total_incision_q8
            .checked_add(incision)
            .ok_or(WorldgenError::ArithmeticOverflow {
                operation: "landscape incision accounting",
            })?;
        state.accounting.incision_updates =
            state.accounting.incision_updates.checked_add(1).ok_or(
                WorldgenError::ArithmeticOverflow {
                    operation: "landscape incision update accounting",
                },
            )?;
        state.work(1)?;
    }
    Ok(())
}

fn incision_alpha_q16(
    erodibility_q32: u32,
    discharge_q16: u64,
    time_step_q16: u32,
    length_q8: u64,
) -> WorldgenResult<u64> {
    let root_q8 = integer_sqrt_u128(u128::from(discharge_q16));
    let product = u128::from(erodibility_q32)
        .checked_mul(root_q8)
        .and_then(|value| value.checked_mul(u128::from(time_step_q16)))
        .ok_or(WorldgenError::ArithmeticOverflow {
            operation: "landscape incision coefficient",
        })?;
    let denominator = u128::from(length_q8.max(1))
        .checked_mul(1_u128 << 32)
        .ok_or(WorldgenError::ArithmeticOverflow {
            operation: "landscape incision coefficient denominator",
        })?;
    let alpha = product / denominator;
    u64::try_from(alpha).map_err(|_| WorldgenError::ArithmeticOverflow {
        operation: "landscape incision alpha Q16",
    })
}

fn diffuse_once(
    elevation: &mut Vec<i32>,
    scratch: &mut Vec<i32>,
    grid: &crate::HydrologicDomainGridV1,
    fixed: &[bool],
    beta_q16: u64,
    state: &mut EvolutionState<'_>,
) -> WorldgenResult<()> {
    scratch.clone_from(elevation);
    if beta_q16 == 0 {
        return Ok(());
    }
    let width = usize::from(grid.width().get());
    let height = usize::from(grid.height().get());
    for z in 1..height - 1 {
        for x in 1..width - 1 {
            let index = x + z * width;
            if fixed[index] {
                continue;
            }
            let center = i128::from(elevation[index]);
            let neighbor_sum = i128::from(elevation[index - 1])
                + i128::from(elevation[index + 1])
                + i128::from(elevation[index - width])
                + i128::from(elevation[index + width]);
            let laplacian = neighbor_sum - center * 4;
            let change = round_divide_i128(
                laplacian.checked_mul(i128::from(beta_q16)).ok_or(
                    WorldgenError::ArithmeticOverflow {
                        operation: "landscape diffusion numerator",
                    },
                )?,
                i128::from(Q16_ONE),
            )?;
            let next = center
                .checked_add(change)
                .and_then(|value| i32::try_from(value).ok())
                .ok_or(WorldgenError::ArithmeticOverflow {
                    operation: "landscape diffusion elevation",
                })?;
            scratch[index] = next;
            let signed_change =
                i64::try_from(change).map_err(|_| WorldgenError::ArithmeticOverflow {
                    operation: "landscape diffusion change",
                })?;
            state.accounting.diffusion_net_change_q8 = state
                .accounting
                .diffusion_net_change_q8
                .checked_add(signed_change)
                .ok_or(WorldgenError::ArithmeticOverflow {
                    operation: "landscape diffusion accounting",
                })?;
            state.accounting.diffusion_updates =
                state.accounting.diffusion_updates.checked_add(1).ok_or(
                    WorldgenError::ArithmeticOverflow {
                        operation: "landscape diffusion update accounting",
                    },
                )?;
            state.work(1)?;
        }
    }
    std::mem::swap(elevation, scratch);
    Ok(())
}

fn maximum_absolute_delta(before: &[i32], after: &[i32]) -> WorldgenResult<u32> {
    if before.len() != after.len() {
        return invalid(
            "landscape.convergence",
            "before and after DEM lengths differ",
        );
    }
    let maximum = before
        .iter()
        .zip(after)
        .map(|(&left, &right)| i64::from(left).abs_diff(i64::from(right)))
        .max()
        .unwrap_or_default();
    u32::try_from(maximum).map_err(|_| WorldgenError::ArithmeticOverflow {
        operation: "landscape convergence delta",
    })
}

fn integer_sqrt_u128(value: u128) -> u128 {
    if value < 2 {
        return value;
    }
    let mut low = 1_u128;
    let mut high = value.min(u128::from(u64::MAX)).saturating_add(1);
    while low.saturating_add(1) < high {
        let middle = low + (high - low) / 2;
        if middle <= value / middle {
            low = middle;
        } else {
            high = middle;
        }
    }
    low
}

fn round_divide_i128(numerator: i128, denominator: i128) -> WorldgenResult<i128> {
    if denominator <= 0 {
        return invalid(
            "landscape.rounding",
            "fixed-point denominator must be positive",
        );
    }
    let half = denominator / 2;
    if numerator >= 0 {
        numerator
            .checked_add(half)
            .map(|value| value / denominator)
            .ok_or(WorldgenError::ArithmeticOverflow {
                operation: "landscape positive rounding",
            })
    } else {
        numerator
            .checked_sub(half)
            .map(|value| value / denominator)
            .ok_or(WorldgenError::ArithmeticOverflow {
                operation: "landscape negative rounding",
            })
    }
}

fn stabilize_result_bytes(plan: &mut LandscapeEvolutionPlanV1, limit: u64) -> WorldgenResult<()> {
    for _ in 0..8 {
        let bytes = u64::try_from(plan.canonical_bytes()?.len()).map_err(|_| {
            WorldgenError::ArithmeticOverflow {
                operation: "landscape result byte accounting",
            }
        })?;
        if bytes > limit {
            return Err(WorldgenError::BudgetExceeded {
                budget: "landscape result bytes",
                required: bytes,
                limit,
            });
        }
        if plan.evidence.accounting.result_bytes == bytes {
            return Ok(());
        }
        plan.evidence.accounting.result_bytes = bytes;
    }
    invalid(
        "landscape.accounting.result_bytes",
        "canonical result-byte accounting did not reach a fixed point",
    )
}

fn invalid<T>(field: &'static str, reason: impl Into<String>) -> WorldgenResult<T> {
    Err(WorldgenError::InvalidHydrologicDomain {
        field,
        reason: reason.into(),
    })
}

#[cfg(test)]
mod tests {
    use std::{
        num::{NonZeroU16, NonZeroU32},
        str::FromStr,
    };

    use bevy::tasks::{TaskPool, TaskPoolBuilder};
    use latticeaxiom_core::CanonicalHash;
    use latticeaxiom_storage::DimensionId;

    use super::*;
    use crate::{
        GenerationEpochIdV1, HydrologicDomainConfigV1, HydrologicDomainGridV1, WorldgenResult,
        plan_hydrologic_domain_v1,
    };

    fn plane_input(domain_x: i64) -> HydrologicDomainInputV1 {
        let width = 17_u16;
        let height = 17_u16;
        let mut elevation = Vec::with_capacity(usize::from(width) * usize::from(height));
        for z in 0..height {
            for x in 0..width {
                elevation.push(
                    40_000_i32
                        .saturating_sub(i32::from(x) * 96)
                        .saturating_sub(i32::from(z) * 64),
                );
            }
        }
        HydrologicDomainInputV1::new(
            DimensionId::from_str("latticeaxiom:dimension/evolution-test")
                .expect("fixture dimension is valid"),
            GenerationEpochIdV1::from_hash(CanonicalHash::digest("evolution-epoch")),
            domain_x,
            0,
            None,
            CanonicalHash::digest("evolution-input"),
            HydrologicDomainGridV1::new(
                domain_x * 1_024,
                0,
                NonZeroU32::new(8).expect("spacing is nonzero"),
                NonZeroU16::new(width).expect("width is nonzero"),
                NonZeroU16::new(height).expect("height is nonzero"),
                2,
            ),
            36_000,
            elevation,
            vec![65_536; usize::from(width) * usize::from(height)],
            Vec::new(),
            HydrologicDomainConfigV1::default(),
        )
        .expect("plane input is valid")
    }

    fn active_config() -> LandscapeEvolutionConfigV1 {
        LandscapeEvolutionConfigV1::new(
            3,
            2,
            2,
            65_536,
            0,
            1 << 26,
            8_192,
            0,
            10_000_000,
            16 * 1024 * 1024,
            64 * 1024 * 1024,
        )
    }

    fn evolve(domain_x: i64, config: LandscapeEvolutionConfigV1) -> LandscapeEvolutionPlanV1 {
        evolve_hydrologic_landscape_v1(&LandscapeEvolutionInputV1::new(
            plane_input(domain_x),
            HydrologicTopologyConfigV1::default(),
            config,
        ))
        .expect("fixture landscape evolves")
    }

    #[test]
    fn disabled_evolution_preserves_direct_domain_and_topology_bytes() {
        let input = plane_input(0);
        let direct_domain = plan_hydrologic_domain_v1(&input).expect("direct domain plans");
        let direct_topology =
            build_hydrologic_topology_v1(&direct_domain, &HydrologicTopologyConfigV1::default())
                .expect("direct topology builds");
        let disabled = LandscapeEvolutionConfigV1::new(
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            10_000_000,
            16 * 1024 * 1024,
            64 * 1024 * 1024,
        );
        let evolved = evolve_hydrologic_landscape_v1(&LandscapeEvolutionInputV1::new(
            input,
            HydrologicTopologyConfigV1::default(),
            disabled,
        ))
        .expect("disabled evolution builds evidence");
        assert_eq!(
            evolved.final_domain().canonical_bytes().ok(),
            direct_domain.canonical_bytes().ok()
        );
        assert_eq!(
            evolved.final_topology().canonical_bytes().ok(),
            direct_topology.canonical_bytes().ok()
        );
        assert_eq!(evolved.evidence().accounting().iterations_completed(), 0);
    }

    #[test]
    fn implicit_incision_lowers_channels_and_final_profiles_remain_monotone() {
        let evolved = evolve(0, active_config());
        assert!(evolved.evidence().accounting().total_incision_q8() > 0);
        let before = evolved.evidence().initial_elevation_q8();
        let after = evolved.final_domain().initial_elevation_q8();
        assert!(before.iter().zip(after).any(|(left, right)| right < left));
        for segment in evolved.final_topology().segments() {
            let [source, target] = segment.centerline() else {
                panic!("v1 segment has two profile points");
            };
            assert!(source.surface_y_q8() >= target.surface_y_q8());
        }
    }

    #[test]
    fn diffusion_smooths_a_peak_and_keeps_fixed_halo_exact() -> WorldgenResult<()> {
        let input = plane_input(0);
        let mut elevation = vec![0_i32; input.grid().sample_count()];
        let center = 8 + 8 * 17;
        elevation[center] = 4_096;
        let before = elevation.clone();
        let mut scratch = elevation.clone();
        let topology_domain = plan_hydrologic_domain_v1(&input)?;
        let topology =
            build_hydrologic_topology_v1(&topology_domain, &HydrologicTopologyConfigV1::default())?;
        let fixed = fixed_mask(&input, &topology)?;
        let config = LandscapeEvolutionConfigV1::default();
        let mut state = EvolutionState::new(&config, None, elevation.len())?;
        diffuse_once(
            &mut elevation,
            &mut scratch,
            input.grid(),
            &fixed,
            Q16_ONE / 8,
            &mut state,
        )?;
        assert!(elevation[center] < before[center]);
        for (index, &is_fixed) in fixed.iter().enumerate() {
            if is_fixed {
                assert_eq!(elevation[index], before[index]);
            }
        }
        Ok(())
    }

    #[test]
    fn hard_limits_and_explicit_diffusion_stability_fail_closed() {
        let input = plane_input(0);
        let too_many = LandscapeEvolutionConfigV1::new(
            MAX_OUTER_ITERATIONS + 1,
            1,
            0,
            65_536,
            0,
            1,
            0,
            0,
            1,
            1,
            1,
        );
        assert!(too_many.validate(&input).is_err());
        let unstable = LandscapeEvolutionConfigV1::new(
            1,
            1,
            1,
            65_536,
            0,
            1,
            u32::MAX,
            0,
            1_000_000,
            1_000_000,
            1_000_000,
        );
        assert!(unstable.validate(&input).is_err());
    }

    #[test]
    fn parallel_completion_order_and_plan_hash_are_stable() {
        let pool: TaskPool = TaskPoolBuilder::new()
            .num_threads(4)
            .thread_name("landscape-test".to_owned())
            .build();
        let requests = [0_i64, 1, -1]
            .into_iter()
            .map(|domain_x| {
                LandscapeEvolutionInputV1::new(
                    plane_input(domain_x),
                    HydrologicTopologyConfigV1::default(),
                    active_config(),
                )
            })
            .collect::<Vec<_>>();
        let forward = evolve_hydrologic_landscapes_parallel_v1(&pool, requests.clone());
        let reverse =
            evolve_hydrologic_landscapes_parallel_v1(&pool, requests.into_iter().rev().collect());
        assert_eq!(
            forward
                .iter()
                .map(|(id, result)| {
                    (
                        *id,
                        result
                            .as_ref()
                            .expect("parallel fixture evolves")
                            .canonical_hash()
                            .expect("parallel fixture canonicalizes"),
                    )
                })
                .collect::<Vec<_>>(),
            reverse
                .iter()
                .map(|(id, result)| {
                    (
                        *id,
                        result
                            .as_ref()
                            .expect("parallel fixture evolves")
                            .canonical_hash()
                            .expect("parallel fixture canonicalizes"),
                    )
                })
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn evolved_plane_has_stable_known_answer_hash() {
        assert_eq!(
            evolve(0, active_config())
                .canonical_hash()
                .expect("evolved plan canonicalizes")
                .to_string(),
            "6a1335a0637311d48e13ac442cf5efcf1bdbd3c59a5ff4a3f17c905b129933c2"
        );
    }
}
