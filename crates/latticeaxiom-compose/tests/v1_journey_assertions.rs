//! V0 machine-readable v1 journey assertions.
//!
//! Local serde types pin the fixture only. They are not compose session
//! contracts and must not be treated as a production world header.

use std::fs;
use std::path::Path;

use serde::Deserialize;

const FIXTURE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/playable/v1-journey-assertions.json"
);
const SCHEMA_ID: &str = "latticeaxiom.v1-journey-assertions.v1";
const REQUIRED_STEP_IDS: [&str; 11] = [
    "create",
    "spawn",
    "explore",
    "enter-cave",
    "gather",
    "craft",
    "mine",
    "place",
    "checkpoint",
    "exit",
    "reopen",
];
const FINITE_FIXTURE_SMOKE_STEPS: [&str; 4] = ["create", "spawn", "explore", "place"];
/// In-memory production host steps evidenced by headless host tests.
/// These flags are not a durable-writer or crash-recovery claim.
const MEMORY_HOST_IMPLEMENTED_STEPS: [&str; 6] =
    ["explore", "enter-cave", "gather", "craft", "mine", "place"];
const REQUIRED_ASSERTION_PHRASES: [&str; 9] = [
    "frozen lock preflight",
    "Y-up",
    "chunk streaming",
    "cave entrance",
    "wood, soil, stone, and at least one ore",
    "workbench",
    "wooden and stone tools",
    "durability",
    "exact generation epoch",
];

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct JourneyAssertions {
    schema_id: String,
    steps: Vec<JourneyStep>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct JourneyStep {
    id: String,
    description: String,
    client_expectation: String,
    headless_expectation: String,
    authoritative_assertions: Vec<String>,
    implemented: bool,
}

#[test]
fn v1_journey_assertions_fixture_records_v0_without_production_claims() {
    let path = Path::new(FIXTURE_PATH);
    let contents = fs::read_to_string(path).unwrap_or_else(|error| {
        panic!(
            "journey fixture must be readable at {}: {error}",
            path.display()
        )
    });
    let fixture: JourneyAssertions = serde_json::from_str(&contents).unwrap_or_else(|error| {
        panic!(
            "journey fixture must decode with deny_unknown_fields at {}: {error}",
            path.display()
        )
    });

    assert_eq!(fixture.schema_id, SCHEMA_ID);
    assert_eq!(
        fixture
            .steps
            .iter()
            .map(|step| step.id.as_str())
            .collect::<Vec<_>>(),
        REQUIRED_STEP_IDS
    );

    let corpus = fixture_corpus(&fixture);
    let assertions = joined_assertions(&fixture);
    assert!(
        corpus.contains("does not claim a production lock"),
        "fixture must deny a production lock"
    );
    assert!(
        corpus.contains("does not claim a durable writer"),
        "fixture must deny a durable writer"
    );

    for step in &fixture.steps {
        let memory_implemented = MEMORY_HOST_IMPLEMENTED_STEPS.contains(&step.id.as_str());
        assert_eq!(
            step.implemented, memory_implemented,
            "step `{}` implemented flag must match the memory production host",
            step.id
        );
        assert!(
            !step.description.is_empty()
                && !step.client_expectation.is_empty()
                && !step.headless_expectation.is_empty()
                && !step.authoritative_assertions.is_empty(),
            "V0 step `{}` must record client, headless, and authoritative expectations",
            step.id
        );
        if FINITE_FIXTURE_SMOKE_STEPS.contains(&step.id.as_str()) {
            let smoke_note = format!(
                "{} {} {}",
                step.description, step.client_expectation, step.headless_expectation
            );
            assert!(
                smoke_note.contains("non-production") && smoke_note.contains("smoke"),
                "step `{}` may only note the current finite fixture as a non-production smoke",
                step.id
            );
        }
    }

    for phrase in REQUIRED_ASSERTION_PHRASES {
        assert!(
            assertions.contains(phrase),
            "authoritative assertions must mention `{phrase}`"
        );
    }
}

fn fixture_corpus(fixture: &JourneyAssertions) -> String {
    let mut corpus = fixture.schema_id.clone();
    for step in &fixture.steps {
        corpus.push('\n');
        corpus.push_str(&step.id);
        corpus.push('\n');
        corpus.push_str(&step.description);
        corpus.push('\n');
        corpus.push_str(&step.client_expectation);
        corpus.push('\n');
        corpus.push_str(&step.headless_expectation);
        for assertion in &step.authoritative_assertions {
            corpus.push('\n');
            corpus.push_str(assertion);
        }
    }
    corpus
}

fn joined_assertions(fixture: &JourneyAssertions) -> String {
    fixture
        .steps
        .iter()
        .flat_map(|step| step.authoritative_assertions.iter().map(String::as_str))
        .collect::<Vec<_>>()
        .join("\n")
}
