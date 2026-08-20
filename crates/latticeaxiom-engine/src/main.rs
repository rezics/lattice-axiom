//! Thin client entry point for the single-session playable slice.

fn main() -> Result<(), latticeaxiom_engine::PlayableClientError> {
    latticeaxiom_engine::run_playable_client()
}
