//! Production client entry point.
//!
//! Ordinary launch reopens `latticeaxiom.lock`, freeze-verifies catalog CAS,
//! and starts the V2/V4 production host through Bevy `DefaultPlugins`. The
//! non-production playable fixture is the extra binary
//! `latticeaxiom-playable-fixture`.

fn main() -> Result<(), latticeaxiom_engine::ProductionClientError> {
    latticeaxiom_engine::run_client_host_from_lock()
}
