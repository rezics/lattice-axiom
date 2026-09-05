//! GPU-free selection of shell vs game process from locked capability evidence.
//!
//! Compiles with `--no-default-features`; it does not construct a Bevy window
//! or GPU device.
#![allow(clippy::expect_used)]

use std::collections::{BTreeMap, BTreeSet};

use latticeaxiom_compose::{LOCK_SCHEMA_VERSION, LockedGameGraph, NickelEvaluationLimits};
use latticeaxiom_core::{CanonicalHash, CapabilityId, PackageName};
use latticeaxiom_engine::{ProductionMemoryStart, ProductionMemoryStartError};

const CLIENT_SHELL_CAPABILITY: &str = "latticeaxiom:capability/client-shell@1";

fn package(value: &str) -> PackageName {
    value
        .parse()
        .unwrap_or_else(|error| panic!("fixture package `{value}` is canonical: {error}"))
}

fn selection_graph(roots: &[&str], providers: Option<&[&str]>) -> LockedGameGraph {
    let mut capability_providers = BTreeMap::new();
    if let Some(providers) = providers {
        capability_providers.insert(
            CLIENT_SHELL_CAPABILITY
                .parse::<CapabilityId>()
                .expect("the client-shell fixture capability is canonical"),
            providers.iter().map(|provider| package(provider)).collect(),
        );
    }
    // Cardinality failures are intentionally constructed at this public
    // selector boundary even though full preparation rejects them earlier.
    let mut graph = LockedGameGraph {
        schema_version: LOCK_SCHEMA_VERSION,
        composition_hash: CanonicalHash::digest(b"selection-composition"),
        composition_provenance_hash: CanonicalHash::digest(b"selection-provenance"),
        evaluation_policy: "latticeaxiom:nickel-evaluation-policy/r0@1"
            .parse()
            .expect("the fixture evaluation policy is canonical"),
        evaluation_limits: NickelEvaluationLimits::default(),
        roots: roots.iter().map(|root| package(root)).collect(),
        packages: BTreeMap::new(),
        capability_providers,
        namespace_grants: BTreeSet::new(),
        explanation: Vec::new(),
        graph_hash: CanonicalHash::digest(b"selection-graph"),
        lock_hash: CanonicalHash::digest(b"selection-lock"),
    };
    graph.graph_hash = graph
        .recompute_graph_hash()
        .expect("the selector fixture graph hashes");
    graph.lock_hash = graph
        .recompute_lock_hash()
        .expect("the selector fixture lock hashes");
    graph
}

fn invalid_provider_evidence(graph: &LockedGameGraph) -> Vec<PackageName> {
    match ProductionMemoryStart::lock_graph_selects_shell(graph) {
        Err(ProductionMemoryStartError::InvalidClientShellProviderEvidence { providers }) => {
            providers
        }
        result => panic!("invalid client-shell evidence must fail closed, got {result:?}"),
    }
}

#[test]
fn absent_client_shell_provider_selects_game_even_for_the_former_shell_root() {
    let graph = selection_graph(&["@latticeaxiom/front-end"], None);
    assert!(
        !ProductionMemoryStart::lock_graph_selects_shell(&graph)
            .expect("absent client-shell evidence has unambiguous game disposition")
    );
}

#[test]
fn one_current_client_shell_provider_selects_shell() {
    let graph = selection_graph(&[], Some(&["@latticeaxiom/front-end"]));
    assert!(
        ProductionMemoryStart::lock_graph_selects_shell(&graph)
            .expect("one client-shell provider is exactly-one evidence")
    );
}

#[test]
fn one_substitute_client_shell_provider_selects_shell() {
    let graph = selection_graph(&[], Some(&["substitute-shell"]));
    assert!(
        ProductionMemoryStart::lock_graph_selects_shell(&graph)
            .expect("one substitute client-shell provider is exactly-one evidence")
    );
}

#[test]
fn present_empty_client_shell_provider_evidence_fails_closed() {
    let graph = selection_graph(&[], Some(&[]));
    assert!(invalid_provider_evidence(&graph).is_empty());
}

#[test]
fn duplicated_client_shell_provider_evidence_fails_closed() {
    let graph = selection_graph(
        &[],
        Some(&["@latticeaxiom/front-end", "@latticeaxiom/front-end"]),
    );
    assert_eq!(
        invalid_provider_evidence(&graph),
        vec![
            package("@latticeaxiom/front-end"),
            package("@latticeaxiom/front-end")
        ]
    );
}

#[test]
fn multiple_client_shell_providers_fail_closed_with_ordered_evidence() {
    let graph = selection_graph(&[], Some(&["@latticeaxiom/front-end", "substitute-shell"]));
    assert_eq!(
        invalid_provider_evidence(&graph),
        vec![
            package("@latticeaxiom/front-end"),
            package("substitute-shell")
        ]
    );
}

#[test]
fn game_root_does_not_override_exactly_one_client_shell_provider() {
    let graph = selection_graph(&["terrenia"], Some(&["substitute-shell"]));
    assert!(
        ProductionMemoryStart::lock_graph_selects_shell(&graph)
            .expect("one client-shell provider remains exactly-one beside any root")
    );
}
