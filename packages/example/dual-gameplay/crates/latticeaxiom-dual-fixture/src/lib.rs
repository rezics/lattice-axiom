//! Headless D1 dual-realization gameplay conformance fixture.
//!
//! The fixture keeps gameplay behavior in one row kernel and exercises it
//! through a static direct adapter and a safe reference representation of the
//! generated portable batch table. It does not load or unload native code.

#![forbid(unsafe_code)]
#![allow(
    clippy::manual_assert,
    reason = "the negative fixture deliberately panics after one staged row"
)]
#![allow(
    clippy::needless_pass_by_value,
    reason = "the SDK macro requires owned marker parameters in the declared row signature"
)]
#![allow(
    clippy::type_complexity,
    reason = "the generated safe table preserves its complete callback signature"
)]
#![allow(
    clippy::too_many_lines,
    reason = "the equivalence pipeline remains together for cold-path auditability"
)]

use latticeaxiom_core::CanonicalHash;

mod error;
mod gameplay;
mod generated;
mod harness;

pub use error::FixtureError;
pub use gameplay::{
    AgentState, CanonicalCommand, CommandEmitter, MiningTarget, MotionIntent, VecCommandSink,
};
pub use generated::{
    CallbackContribution, CallbackContributionKind, GeneratedContributions, fixture_artifacts,
    fixture_registration_ir, generated_c_binding, generated_contributions,
};
pub use harness::{
    BatchCallDiagnostic, EquivalenceReceipt, FaultInjection, GeneratedPortableTable,
    LifecyclePhase, Realization, ReferenceDynamicModule, RunEvidence, canonical_fixture_rows,
    ffi_batch_call_diagnostic, run_equivalence, run_realization,
};

/// Returns the registration identity consumed by generated `NativeStatic` glue.
///
/// This entry is a direct Rust call. It does not cross the portable C ABI and
/// derives its value from the same generated registration artifacts as the
/// portable realization.
///
/// # Errors
///
/// Returns [`FixtureError`] if registration generation or verification fails.
pub fn native_static_registration_hash() -> Result<CanonicalHash, FixtureError> {
    Ok(fixture_artifacts()?.registration.manifest.semantic_hash)
}
#[cfg(test)]
mod tests;
