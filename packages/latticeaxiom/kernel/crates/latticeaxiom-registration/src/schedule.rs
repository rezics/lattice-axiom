//! Deterministic semantic-stage schedule compilation.

use std::collections::{BTreeMap, BTreeSet};

use latticeaxiom_core::StableId;

use crate::{RegistrationCompileError, SystemDeclaration};

const STAGES: [&str; 10] = [
    "latticeaxiom:system-stage/input/sample@1",
    "latticeaxiom:system-stage/gameplay/fixed-pre@1",
    "latticeaxiom:system-stage/gameplay/fixed@1",
    "latticeaxiom:system-stage/physics/integrate@1",
    "latticeaxiom:system-stage/world/commands-apply@1",
    "latticeaxiom:system-stage/world/revision-commit@1",
    "latticeaxiom:system-stage/derived-work/queue@1",
    "latticeaxiom:system-stage/persistence/capture@1",
    "latticeaxiom:system-stage/async-results/observe@1",
    "latticeaxiom:system-stage/presentation/update@1",
];

pub(crate) fn compile_schedule(
    systems: &BTreeMap<StableId, &SystemDeclaration>,
) -> Result<Vec<StableId>, RegistrationCompileError> {
    for system in systems.values() {
        if stage_index(&system.stage).is_none() {
            return Err(RegistrationCompileError::UnknownStage {
                system: system.id.clone(),
                stage: system.stage.clone(),
            });
        }
    }

    let mut output = Vec::with_capacity(systems.len());
    for stage in STAGES {
        let stage_systems = systems
            .iter()
            .filter(|(_, system)| system.stage.as_str() == stage)
            .map(|(id, system)| (id.clone(), *system))
            .collect::<BTreeMap<_, _>>();
        if stage_systems.is_empty() {
            continue;
        }
        output.extend(compile_stage(&stage_systems)?);
    }
    Ok(output)
}

fn compile_stage(
    systems: &BTreeMap<StableId, &SystemDeclaration>,
) -> Result<Vec<StableId>, RegistrationCompileError> {
    let mut outgoing = systems
        .keys()
        .cloned()
        .map(|id| (id, BTreeSet::new()))
        .collect::<BTreeMap<_, _>>();
    let mut incoming = systems
        .keys()
        .cloned()
        .map(|id| (id, 0_usize))
        .collect::<BTreeMap<_, _>>();

    for system in systems.values() {
        for target in &system.before {
            add_edge(systems, &mut outgoing, &mut incoming, &system.id, target)?;
        }
        for target in &system.after {
            add_edge(systems, &mut outgoing, &mut incoming, target, &system.id)?;
        }
    }

    let mut ready = incoming
        .iter()
        .filter(|(_, count)| **count == 0)
        .map(|(id, _)| id.clone())
        .collect::<BTreeSet<_>>();
    let mut ordered = Vec::with_capacity(systems.len());
    while let Some(next) = ready.pop_first() {
        ordered.push(next.clone());
        if let Some(targets) = outgoing.get(&next) {
            for target in targets {
                let Some(count) = incoming.get_mut(target) else {
                    return Err(RegistrationCompileError::UnknownScheduleTarget {
                        system: next.clone(),
                        target: target.clone(),
                    });
                };
                *count = count.saturating_sub(1);
                if *count == 0 {
                    ready.insert(target.clone());
                }
            }
        }
    }

    if ordered.len() != systems.len() {
        let stage = systems
            .values()
            .next()
            .map(|system| system.stage.clone())
            .ok_or(RegistrationCompileError::LimitExceeded {
                limit: "schedule-cycle-diagnostic",
                observed: 0,
                maximum: 0,
            })?;
        let cyclic = incoming
            .into_iter()
            .filter(|(_, count)| *count > 0)
            .map(|(id, _)| id)
            .collect();
        return Err(RegistrationCompileError::ScheduleCycle {
            stage,
            systems: cyclic,
        });
    }
    Ok(ordered)
}

fn add_edge(
    systems: &BTreeMap<StableId, &SystemDeclaration>,
    outgoing: &mut BTreeMap<StableId, BTreeSet<StableId>>,
    incoming: &mut BTreeMap<StableId, usize>,
    from: &StableId,
    to: &StableId,
) -> Result<(), RegistrationCompileError> {
    let Some(from_system) = systems.get(from) else {
        return Err(RegistrationCompileError::UnknownScheduleTarget {
            system: to.clone(),
            target: from.clone(),
        });
    };
    let Some(to_system) = systems.get(to) else {
        return Err(RegistrationCompileError::UnknownScheduleTarget {
            system: from.clone(),
            target: to.clone(),
        });
    };
    if from_system.stage != to_system.stage {
        return Err(RegistrationCompileError::CrossStageScheduleEdge {
            system: from.clone(),
            stage: from_system.stage.clone(),
            target: to.clone(),
            target_stage: to_system.stage.clone(),
        });
    }
    let inserted = outgoing
        .get_mut(from)
        .is_some_and(|targets| targets.insert(to.clone()));
    if inserted {
        let Some(count) = incoming.get_mut(to) else {
            return Err(RegistrationCompileError::UnknownScheduleTarget {
                system: from.clone(),
                target: to.clone(),
            });
        };
        *count = count
            .checked_add(1)
            .ok_or(RegistrationCompileError::LimitExceeded {
                limit: "schedule-edges",
                observed: usize::MAX,
                maximum: usize::MAX - 1,
            })?;
    }
    Ok(())
}

fn stage_index(stage: &StableId) -> Option<usize> {
    STAGES
        .iter()
        .position(|candidate| *candidate == stage.as_str())
}
