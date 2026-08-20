//! Non-production local playable fixture.
//!
//! This extra binary keeps the trusted Terrenia seed slice reachable for
//! development. It is not the default engine client and does not boot from a
//! reopened product lock.

fn main() -> Result<(), latticeaxiom_engine::PlayableClientError> {
    latticeaxiom_engine::run_playable_client()
}
