//! Static/direct and portable/batched execution plus equivalence receipts.

use std::{collections::BTreeMap, panic::AssertUnwindSafe};

use latticeaxiom_abi::{
    BatchLimits, BatchShape, ColumnShape, CommandContract, CommandKind, CommandLimits,
    CommandRecord, HandleTableSnapshot, HeaderPolicy, LAX_ABI_MAJOR, LAX_ABI_MINOR,
    LAX_COLUMN_FLAG_READ, LAX_COLUMN_FLAG_WRITE, LaxAbiHeader, LaxEntityHandle, TargetRequirement,
    validate_batches, validate_commands, validate_header,
};
use latticeaxiom_compose::RegistrationKind;
use latticeaxiom_core::{CanonicalHash, StableId, canonical_json_bytes, canonical_json_hash};
use latticeaxiom_sdk::FixedTick;
use serde::{Deserialize, Serialize};

use crate::{
    AgentState, CanonicalCommand, FixtureError, MiningTarget, MotionIntent, VecCommandSink,
    fixture_artifacts, gameplay::gameplay_business_row, generated_contributions,
};

const BATCH_SIZE: usize = 256;
const BATCHES_PER_CALL: usize = 4;
const ENTITIES_PER_CALL: usize = BATCH_SIZE * BATCHES_PER_CALL;
const FIXED_DELTA_NANOS: u64 = 16_666_667;

/// Executable realization selected by the headless harness.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Realization {
    /// Direct typed row iteration with no C table or ABI repacking.
    StaticDirect,
    /// Safe reference of a generated C batch table and staged host bridge.
    PortableBatch,
}

/// Deliberate callback fault used only by deterministic negative fixtures.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum FaultInjection {
    /// Execute the callback normally.
    #[default]
    None,
    /// Panic after mutating the first staged row.
    PanicAfterFirstRow,
}

/// Fixed portable instance lifecycle phase.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LifecyclePhase {
    /// Instance exists but has not started callbacks.
    Created,
    /// Instance accepts callbacks.
    Started,
    /// New callbacks are rejected while outstanding work drains.
    Quiesced,
    /// Instance has stopped and remains mapped.
    Stopped,
    /// Instance resources were destroyed; library mapping remains intact.
    Destroyed,
    /// A callback fault made the instance unusable.
    Failed,
}

impl LifecyclePhase {
    const fn display(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Started => "started",
            Self::Quiesced => "quiesced",
            Self::Stopped => "stopped",
            Self::Destroyed => "destroyed",
            Self::Failed => "failed",
        }
    }
}

/// Safe reference module for generated ABI entry, lifecycle, and batch calls.
///
/// This type intentionally owns no operating-system library handle. The
/// production loader is still pending a reviewed safe boundary.
#[derive(Debug)]
pub struct ReferenceDynamicModule {
    phase: LifecyclePhase,
    entered: bool,
    manifest_hash: CanonicalHash,
    callback_map_hash: CanonicalHash,
    callback_calls: u64,
}

impl ReferenceDynamicModule {
    /// Creates an unentered mapped-module reference from generated artifacts.
    ///
    /// # Errors
    ///
    /// Returns an artifact generation error before any callback can run.
    pub fn new() -> Result<Self, FixtureError> {
        let artifacts = fixture_artifacts()?;
        Ok(Self {
            phase: LifecyclePhase::Created,
            entered: false,
            manifest_hash: artifacts.registration.manifest.semantic_hash,
            callback_map_hash: artifacts.callback_map.callback_map_hash,
            callback_calls: 0,
        })
    }

    /// Returns a valid ABI 0.1 entry header for this host target.
    ///
    /// # Errors
    ///
    /// Returns a wire-size error only on an unsupported platform size model.
    pub fn entry_header(&self) -> Result<LaxAbiHeader, FixtureError> {
        let size = u32::try_from(core::mem::size_of::<latticeaxiom_abi::LaxEntryRequestV0>())
            .map_err(|_| FixtureError::WireSizeOverflow {
                field: "entry request size",
            })?;
        Ok(LaxAbiHeader::new(LAX_ABI_MAJOR, LAX_ABI_MINOR, size, 0))
    }

    /// Validates ABI and generated hashes before enabling instance start.
    ///
    /// # Errors
    ///
    /// Returns a stable ABI, manifest, callback-map, or lifecycle failure.
    pub fn entry(
        &mut self,
        header: &LaxAbiHeader,
        expected_manifest_hash: CanonicalHash,
        expected_callback_map_hash: CanonicalHash,
    ) -> Result<(), FixtureError> {
        if self.phase != LifecyclePhase::Created || self.entered {
            return Err(self.lifecycle_error("entry"));
        }
        let minimum_struct_size = u32::try_from(core::mem::size_of::<
            latticeaxiom_abi::LaxEntryRequestV0,
        >())
        .map_err(|_| FixtureError::WireSizeOverflow {
            field: "entry request size",
        })?;
        validate_header(
            header,
            HeaderPolicy {
                expected_major: LAX_ABI_MAJOR,
                min_minor: LAX_ABI_MINOR,
                max_minor: LAX_ABI_MINOR,
                minimum_struct_size,
                known_required_flags: 0,
            },
        )?;
        if expected_manifest_hash != self.manifest_hash {
            return Err(FixtureError::ManifestMismatch {
                expected: expected_manifest_hash,
                actual: self.manifest_hash,
            });
        }
        if expected_callback_map_hash != self.callback_map_hash {
            return Err(FixtureError::CallbackMapMismatch {
                expected: expected_callback_map_hash,
                actual: self.callback_map_hash,
            });
        }
        self.entered = true;
        Ok(())
    }

    /// Starts callback execution after successful entry validation.
    ///
    /// # Errors
    ///
    /// Returns a lifecycle failure when entry was skipped or start repeats.
    pub fn start(&mut self) -> Result<(), FixtureError> {
        if self.phase != LifecyclePhase::Created || !self.entered {
            return Err(self.lifecycle_error("start"));
        }
        self.phase = LifecyclePhase::Started;
        Ok(())
    }

    /// Quiesces the instance and rejects all later callbacks.
    ///
    /// # Errors
    ///
    /// Returns a lifecycle failure unless the instance is started.
    pub fn quiesce(&mut self) -> Result<(), FixtureError> {
        if self.phase != LifecyclePhase::Started {
            return Err(self.lifecycle_error("quiesce"));
        }
        self.phase = LifecyclePhase::Quiesced;
        Ok(())
    }

    /// Stops a quiesced instance without unloading its library.
    ///
    /// # Errors
    ///
    /// Returns a lifecycle failure unless quiesce completed.
    pub fn stop(&mut self) -> Result<(), FixtureError> {
        if self.phase != LifecyclePhase::Quiesced {
            return Err(self.lifecycle_error("stop"));
        }
        self.phase = LifecyclePhase::Stopped;
        Ok(())
    }

    /// Destroys stopped instance resources while retaining the mapped module.
    ///
    /// # Errors
    ///
    /// Returns a lifecycle failure unless stop completed.
    pub fn destroy(&mut self) -> Result<(), FixtureError> {
        if self.phase != LifecyclePhase::Stopped {
            return Err(self.lifecycle_error("destroy"));
        }
        self.phase = LifecyclePhase::Destroyed;
        Ok(())
    }

    /// Returns the current lifecycle phase.
    #[must_use]
    pub const fn phase(&self) -> LifecyclePhase {
        self.phase
    }

    /// Returns host-to-module system calls observed so far.
    #[must_use]
    pub const fn callback_calls(&self) -> u64 {
        self.callback_calls
    }

    fn lifecycle_error(&self, operation: &'static str) -> FixtureError {
        FixtureError::InvalidLifecycle {
            operation,
            phase: self.phase.display(),
        }
    }

    fn invoke_system(
        &mut self,
        states: &mut [AgentState],
        intents: &[MotionIntent],
        targets: &[MiningTarget],
        tick: u64,
        fault: FaultInjection,
    ) -> Result<Vec<CanonicalCommand>, FixtureError> {
        if self.phase != LifecyclePhase::Started {
            return if matches!(
                self.phase,
                LifecyclePhase::Quiesced | LifecyclePhase::Stopped | LifecyclePhase::Destroyed
            ) {
                Err(FixtureError::CallbackAfterStop)
            } else {
                Err(self.lifecycle_error("callback"))
            };
        }
        if states.len() != intents.len() || states.len() != targets.len() {
            return Err(FixtureError::ColumnCountMismatch);
        }
        self.callback_calls = self.callback_calls.saturating_add(1);
        validate_callback_batches(states, intents, targets)?;
        let mut sink = VecCommandSink::default();
        let execution = std::panic::catch_unwind(AssertUnwindSafe(|| {
            for (index, ((state, intent), target)) in
                states.iter_mut().zip(intents).zip(targets).enumerate()
            {
                gameplay_business_row(
                    state,
                    intent,
                    target,
                    u64::try_from(index).unwrap_or(u64::MAX),
                    FixedTick {
                        tick,
                        delta_nanos: FIXED_DELTA_NANOS,
                    },
                    &mut sink,
                );
                if index == 0 && fault == FaultInjection::PanicAfterFirstRow {
                    panic!("D1 injected callback panic after staged mutation");
                }
            }
        }));
        if execution.is_err() {
            self.phase = LifecyclePhase::Failed;
            return Err(FixtureError::CallbackPanicked);
        }
        Ok(sink.into_commands())
    }
}

/// Safe generated portable function table used by the reference host.
#[derive(Clone, Copy, Debug)]
pub struct GeneratedPortableTable {
    invoke_system: fn(
        &mut ReferenceDynamicModule,
        &mut [AgentState],
        &[MotionIntent],
        &[MiningTarget],
        u64,
        FaultInjection,
    ) -> Result<Vec<CanonicalCommand>, FixtureError>,
}

impl GeneratedPortableTable {
    /// Constructs the generated table with its batch callback binding.
    #[must_use]
    pub const fn generated() -> Self {
        Self {
            invoke_system: ReferenceDynamicModule::invoke_system,
        }
    }

    /// Invokes one system callback containing one or more bounded batches.
    ///
    /// # Errors
    ///
    /// Returns deterministic lifecycle, ABI, or callback fault diagnostics.
    pub fn invoke(
        self,
        module: &mut ReferenceDynamicModule,
        states: &mut [AgentState],
        intents: &[MotionIntent],
        targets: &[MiningTarget],
        tick: u64,
        fault: FaultInjection,
    ) -> Result<Vec<CanonicalCommand>, FixtureError> {
        (self.invoke_system)(module, states, intents, targets, tick, fault)
    }
}

/// Normative static/dynamic equivalence receipt.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EquivalenceReceipt {
    /// SDK registration semantic hash shared by realizations.
    pub registration_semantic_hash: CanonicalHash,
    /// Per-kind dense numeric ID tables.
    pub numeric_ids: BTreeMap<RegistrationKind, BTreeMap<StableId, u32>>,
    /// Deterministic system schedule.
    pub schedule: Vec<StableId>,
    /// Canonical authoritative command stream across all ticks.
    pub commands: Vec<CanonicalCommand>,
    /// State hash after every authoritative barrier.
    pub state_hashes: Vec<CanonicalHash>,
    /// Effective typed setting read through the contribution callback.
    pub effective_max_mining_effort: u32,
    /// Batched diagnostic metric sample.
    pub diagnostic_break_command_count: u64,
    /// Batched inspect sample over final authoritative rows.
    pub inspect_state_hash: CanonicalHash,
    /// Canonical uncompressed normative snapshot bytes.
    pub snapshot_bytes: Vec<u8>,
    /// Content address of all preceding semantic fields.
    pub receipt_hash: CanonicalHash,
}

/// Execution evidence containing normative receipt and non-normative counters.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RunEvidence {
    /// Realization that produced this evidence.
    pub realization: Realization,
    /// Normative equivalence receipt.
    pub receipt: EquivalenceReceipt,
    /// Host-to-module ABI system calls; always zero for static direct.
    pub ffi_system_calls: u64,
}

/// Pure diagnostic proving call scaling is per batch, not per entity.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BatchCallDiagnostic {
    /// Input entity count.
    pub entity_count: usize,
    /// Generated archetype batch count.
    pub batch_count: usize,
    /// Generated portable callbacks after grouping bounded batches.
    pub portable_callback_count: usize,
    /// Deliberately invalid per-entity callback count.
    pub per_entity_counterexample: usize,
}

/// Builds deterministic gameplay rows for a requested entity count.
#[must_use]
pub fn canonical_fixture_rows(
    entity_count: usize,
) -> (Vec<AgentState>, Vec<MotionIntent>, Vec<MiningTarget>) {
    let mut states = Vec::with_capacity(entity_count);
    let mut intents = Vec::with_capacity(entity_count);
    let mut targets = Vec::with_capacity(entity_count);
    for index in 0..entity_count {
        let ordinal = i64::try_from(index).unwrap_or(i64::MAX);
        let ordinal_i32 = i32::try_from(index).unwrap_or(i32::MAX);
        states.push(AgentState {
            x_millimeters: ordinal.saturating_mul(10),
            y_millimeters: 64_000,
            z_millimeters: ordinal.saturating_neg(),
            stamina: 1_000,
            break_cooldown: u32::try_from(index % 3).unwrap_or(0),
        });
        intents.push(MotionIntent {
            velocity_x: 60,
            velocity_y: 0,
            velocity_z: -60,
            effort: 50_u32.saturating_add(u32::try_from(index % 3).unwrap_or(0) * 10),
        });
        targets.push(MiningTarget {
            x: ordinal_i32,
            y: 63,
            z: ordinal_i32.saturating_neg(),
            hardness: if index % 2 == 0 { 50 } else { 80 },
        });
    }
    (states, intents, targets)
}

/// Computes portable and per-entity callback counts without timing noise.
#[must_use]
pub const fn ffi_batch_call_diagnostic(entity_count: usize) -> BatchCallDiagnostic {
    let batch_count = entity_count.div_ceil(BATCH_SIZE);
    BatchCallDiagnostic {
        entity_count,
        batch_count,
        portable_callback_count: batch_count.div_ceil(BATCHES_PER_CALL),
        per_entity_counterexample: entity_count,
    }
}

/// Runs one realization for a deterministic N-tick authoritative fixture.
///
/// # Errors
///
/// Returns a generated artifact, ABI validation, lifecycle, or canonical
/// receipt error. Dynamic output is staged and commits only on success.
pub fn run_realization(
    realization: Realization,
    ticks: u64,
    entity_count: usize,
) -> Result<RunEvidence, FixtureError> {
    let artifacts = fixture_artifacts()?;
    let contributions = generated_contributions()?;
    if contributions.static_callbacks != contributions.dynamic_callbacks {
        return Err(FixtureError::ReceiptMismatch);
    }
    let (mut states, intents, targets) = canonical_fixture_rows(entity_count);
    let mut all_commands = Vec::new();
    let mut state_hashes = Vec::new();
    let mut ffi_system_calls = 0_u64;
    let mut dynamic_module = if realization == Realization::PortableBatch {
        let mut module = ReferenceDynamicModule::new()?;
        let header = module.entry_header()?;
        module.entry(
            &header,
            artifacts.registration.manifest.semantic_hash,
            artifacts.callback_map.callback_map_hash,
        )?;
        module.start()?;
        Some(module)
    } else {
        None
    };
    for tick in 0..ticks {
        let commands = match realization {
            Realization::StaticDirect => static_tick(&mut states, &intents, &targets, tick),
            Realization::PortableBatch => {
                let mut staged_states = states.clone();
                let mut staged_commands = Vec::new();
                let Some(module) = dynamic_module.as_mut() else {
                    return Err(FixtureError::InvalidLifecycle {
                        operation: "callback",
                        phase: "missing-module",
                    });
                };
                let table = GeneratedPortableTable::generated();
                for start in (0..entity_count).step_by(ENTITIES_PER_CALL) {
                    let end = start.saturating_add(ENTITIES_PER_CALL).min(entity_count);
                    let mut callback_commands = table.invoke(
                        module,
                        &mut staged_states[start..end],
                        &intents[start..end],
                        &targets[start..end],
                        tick,
                        FaultInjection::None,
                    )?;
                    for command in &mut callback_commands {
                        command.row_entity = command
                            .row_entity
                            .saturating_add(u64::try_from(start).unwrap_or(u64::MAX));
                        command.sequence = command.row_entity.saturating_add(1);
                    }
                    staged_commands.append(&mut callback_commands);
                }
                validate_canonical_commands(&staged_commands)?;
                states = staged_states;
                staged_commands
            }
        };
        all_commands.extend(commands);
        state_hashes.push(canonical_json_hash(&states)?);
    }
    if let Some(module) = dynamic_module.as_mut() {
        ffi_system_calls = module.callback_calls();
        module.quiesce()?;
        module.stop()?;
        module.destroy()?;
    }
    let numeric_ids = numeric_ids(&artifacts);
    let schedule = artifacts
        .registration
        .systems
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    let snapshot = CanonicalSnapshot {
        schema_version: 1,
        completed_ticks: ticks,
        states: &states,
        commands: &all_commands,
    };
    let snapshot_bytes = canonical_json_bytes(&snapshot)?;
    let effective_max_mining_effort = 64;
    let diagnostic_break_command_count =
        u64::try_from(all_commands.len()).map_err(|_| FixtureError::WireSizeOverflow {
            field: "diagnostic command count",
        })?;
    let inspect_state_hash = canonical_json_hash(&states)?;
    let identity = ReceiptIdentity {
        registration_semantic_hash: artifacts.registration.manifest.semantic_hash,
        numeric_ids: &numeric_ids,
        schedule: &schedule,
        commands: &all_commands,
        state_hashes: &state_hashes,
        effective_max_mining_effort,
        diagnostic_break_command_count,
        inspect_state_hash,
        snapshot_bytes: &snapshot_bytes,
    };
    let receipt_hash = canonical_json_hash(&identity)?;
    Ok(RunEvidence {
        realization,
        receipt: EquivalenceReceipt {
            registration_semantic_hash: artifacts.registration.manifest.semantic_hash,
            numeric_ids,
            schedule,
            commands: all_commands,
            state_hashes,
            effective_max_mining_effort,
            diagnostic_break_command_count,
            inspect_state_hash,
            snapshot_bytes,
            receipt_hash,
        },
        ffi_system_calls,
    })
}

/// Runs and compares static direct and portable batch realizations.
///
/// # Errors
///
/// Returns a fixture failure or receipt mismatch.
pub fn run_equivalence(
    ticks: u64,
    entity_count: usize,
) -> Result<EquivalenceReceipt, FixtureError> {
    let static_evidence = run_realization(Realization::StaticDirect, ticks, entity_count)?;
    let dynamic_evidence = run_realization(Realization::PortableBatch, ticks, entity_count)?;
    if static_evidence.receipt != dynamic_evidence.receipt {
        return Err(FixtureError::ReceiptMismatch);
    }
    Ok(static_evidence.receipt)
}

fn static_tick(
    states: &mut [AgentState],
    intents: &[MotionIntent],
    targets: &[MiningTarget],
    tick: u64,
) -> Vec<CanonicalCommand> {
    let mut sink = VecCommandSink::default();
    for (index, ((state, intent), target)) in
        states.iter_mut().zip(intents).zip(targets).enumerate()
    {
        gameplay_business_row(
            state,
            intent,
            target,
            u64::try_from(index).unwrap_or(u64::MAX),
            FixedTick {
                tick,
                delta_nanos: FIXED_DELTA_NANOS,
            },
            &mut sink,
        );
    }
    sink.into_commands()
}

fn validate_callback_batches(
    states: &[AgentState],
    intents: &[MotionIntent],
    targets: &[MiningTarget],
) -> Result<(), FixtureError> {
    for start in (0..states.len()).step_by(BATCH_SIZE) {
        let end = start.saturating_add(BATCH_SIZE).min(states.len());
        let columns = [
            column_shape(1, &states[start..end], true)?,
            column_shape(2, &targets[start..end], false)?,
            column_shape(3, &intents[start..end], false)?,
        ];
        let count = u64::try_from(end.saturating_sub(start)).map_err(|_| {
            FixtureError::WireSizeOverflow {
                field: "batch entity count",
            }
        })?;
        validate_batches(
            &[BatchShape {
                entity_count: count,
                columns: &columns,
            }],
            BatchLimits {
                max_batches: 1,
                max_entities_per_batch: 1_024,
                max_total_entities: 1_024,
                max_columns_per_batch: 8,
                max_total_columns: 8,
                max_total_bytes: 64 * 1_024,
            },
        )?;
    }
    Ok(())
}

fn column_shape<T>(
    numeric_id: u32,
    values: &[T],
    writable: bool,
) -> Result<ColumnShape, FixtureError> {
    let element_size =
        u64::try_from(core::mem::size_of::<T>()).map_err(|_| FixtureError::WireSizeOverflow {
            field: "column element size",
        })?;
    let count = u64::try_from(values.len()).map_err(|_| FixtureError::WireSizeOverflow {
        field: "column count",
    })?;
    let data_address = if values.is_empty() {
        None
    } else {
        Some(
            u64::try_from(values.as_ptr().addr()).map_err(|_| FixtureError::WireSizeOverflow {
                field: "column address",
            })?,
        )
    };
    let alignment =
        u32::try_from(core::mem::align_of::<T>()).map_err(|_| FixtureError::WireSizeOverflow {
            field: "column alignment",
        })?;
    Ok(ColumnShape {
        component_numeric_id: numeric_id,
        layout_hash: CanonicalHash::digest(format!(
            "latticeaxiom.abi-layout.v1:{numeric_id}:{element_size}:{alignment}"
        ))
        .into_bytes(),
        data_address,
        count,
        stride: element_size,
        element_size,
        alignment,
        flags: if writable {
            LAX_COLUMN_FLAG_READ | LAX_COLUMN_FLAG_WRITE
        } else {
            LAX_COLUMN_FLAG_READ
        },
        reserved: 0,
    })
}

fn validate_canonical_commands(commands: &[CanonicalCommand]) -> Result<(), FixtureError> {
    let payloads = commands
        .iter()
        .map(canonical_json_bytes)
        .collect::<Result<Vec<_>, _>>()?;
    let layout_hash = CanonicalHash::digest(b"example:schema/break-block-command@1").into_bytes();
    let records = commands
        .iter()
        .zip(&payloads)
        .map(|(command, payload)| CommandRecord {
            sequence: command.sequence,
            opcode: CommandKind::World.wire_code(),
            flags: 0,
            target: LaxEntityHandle::from_wire(0, 0, 0),
            subject_numeric_id: 1,
            reserved: 0,
            layout_hash,
            payload,
        })
        .collect::<Vec<_>>();
    validate_commands(
        &records,
        &[CommandContract {
            kind: CommandKind::World,
            subject_numeric_id: 1,
            layout_hash,
            min_payload_bytes: 1,
            max_payload_bytes: 1_024,
            target: TargetRequirement::None,
        }],
        HandleTableSnapshot {
            table_id: 1,
            live: &[],
        },
        CommandLimits {
            max_commands: 16_384,
            max_contracts: 16,
            max_live_handles: 16_384,
            max_payload_bytes_per_command: 1_024,
            max_total_payload_bytes: 1024 * 1024,
        },
    )?;
    Ok(())
}

fn numeric_ids(
    artifacts: &latticeaxiom_sdk::GeneratedRegistrationArtifacts,
) -> BTreeMap<RegistrationKind, BTreeMap<StableId, u32>> {
    let mut grouped = BTreeMap::<RegistrationKind, Vec<StableId>>::new();
    for row in &artifacts.registration.manifest.fragment.registrations {
        grouped.entry(row.kind).or_default().push(row.id.clone());
    }
    grouped
        .into_iter()
        .map(|(kind, mut ids)| {
            ids.sort();
            let table = ids
                .into_iter()
                .enumerate()
                .map(|(index, id)| (id, u32::try_from(index).unwrap_or(u32::MAX)))
                .collect();
            (kind, table)
        })
        .collect()
}

#[derive(Serialize)]
struct CanonicalSnapshot<'a> {
    schema_version: u32,
    completed_ticks: u64,
    states: &'a [AgentState],
    commands: &'a [CanonicalCommand],
}

#[derive(Serialize)]
struct ReceiptIdentity<'a> {
    registration_semantic_hash: CanonicalHash,
    numeric_ids: &'a BTreeMap<RegistrationKind, BTreeMap<StableId, u32>>,
    schedule: &'a [StableId],
    commands: &'a [CanonicalCommand],
    state_hashes: &'a [CanonicalHash],
    effective_max_mining_effort: u32,
    diagnostic_break_command_count: u64,
    inspect_state_hash: CanonicalHash,
    snapshot_bytes: &'a [u8],
}
