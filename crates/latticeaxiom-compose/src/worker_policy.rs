//! Versioned receipts for Nickel worker resource enforcement.

use latticeaxiom_core::{StableId, TargetTriple};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{NickelEvaluationLimits, R0_NICKEL_EVALUATION_POLICY};

/// R0 hard-memory backend used by workers whose containment host is Windows.
pub const R0_WINDOWS_MEMORY_BACKEND: &str = "latticeaxiom:worker-memory-backend/windows-job@1";

/// R0 hard-memory backend used by workers whose containment host is Linux.
pub const R0_LINUX_MEMORY_BACKEND: &str = "latticeaxiom:worker-memory-backend/linux-cgroup-v2@1";

/// Strength with which a worker backend enforces one resource boundary.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum EnforcementStrength {
    /// The backend prevents work from continuing beyond the boundary.
    Hard,
    /// The backend observes the boundary but cannot reliably terminate work.
    Soft,
    /// The platform or evaluator cannot meter this resource.
    Unsupported,
}

/// One versioned resource-enforcement capability.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EnforcementCapability {
    /// Effective enforcement strength.
    pub strength: EnforcementStrength,
    /// Versioned implementation and accounting backend.
    pub backend: Option<StableId>,
}

impl EnforcementCapability {
    /// Constructs a hard capability backed by a versioned implementation.
    #[must_use]
    pub const fn hard(backend: StableId) -> Self {
        Self {
            strength: EnforcementStrength::Hard,
            backend: Some(backend),
        }
    }

    /// Constructs an unsupported capability with no claimed backend.
    #[must_use]
    pub const fn unsupported() -> Self {
        Self {
            strength: EnforcementStrength::Unsupported,
            backend: None,
        }
    }

    fn validate(&self, resource: WorkerResource) -> Result<(), WorkerPolicyError> {
        match (self.strength, &self.backend) {
            (EnforcementStrength::Unsupported, None) => Ok(()),
            (EnforcementStrength::Hard | EnforcementStrength::Soft, Some(backend)) => {
                if backend.kind() == resource.backend_kind() && backend.major().is_some() {
                    Ok(())
                } else {
                    Err(WorkerPolicyError::InvalidBackend {
                        resource,
                        backend: backend.to_string(),
                    })
                }
            }
            _ => Err(WorkerPolicyError::BackendStrengthMismatch { resource }),
        }
    }
}

/// Worker resource with an independently versioned enforcement backend.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkerResource {
    /// Complete-composition wall-clock deadline.
    Deadline,
    /// Worker containment-domain memory ceiling.
    Memory,
    /// Active Nickel function and contract frame depth.
    Recursion,
}

impl WorkerResource {
    const fn backend_kind(self) -> &'static str {
        match self {
            Self::Deadline => "worker-deadline-backend",
            Self::Memory => "worker-memory-backend",
            Self::Recursion => "worker-recursion-backend",
        }
    }
}

/// Exact evaluator policy and enforcement facts recorded for one evaluation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EvaluationPolicyReceipt {
    /// Versioned resource policy selected by the trusted profile.
    pub policy: StableId,
    /// Target on which the controller and worker execute.
    ///
    /// This is the containment host target, not a potentially cross-compiled
    /// realization target from `CompositionSpec` or `BuildPlan`.
    pub target: TargetTriple,
    /// Exact effective limits used by the controller and worker.
    pub limits: NickelEvaluationLimits,
    /// Deadline enforcement capability.
    pub deadline: EnforcementCapability,
    /// Memory enforcement capability.
    pub memory: EnforcementCapability,
    /// Function and contract recursion enforcement capability.
    pub recursion: EnforcementCapability,
}

impl EvaluationPolicyReceipt {
    /// Validates owned policy identity, backend identity, and production R0
    /// fail-closed requirements.
    ///
    /// Alternative versioned policies may record soft or unsupported
    /// capabilities, but the production R0 policy requires hard enforcement
    /// for deadline, memory, and recursion.
    ///
    /// # Errors
    ///
    /// Returns [`WorkerPolicyError`] for malformed backend receipts, invalid
    /// limits, or a production R0 capability that is not hard enforced.
    pub fn validate(&self) -> Result<(), WorkerPolicyError> {
        if self.policy.namespace() != "latticeaxiom"
            || self.policy.kind() != "nickel-evaluation-policy"
            || self.policy.major().is_none()
        {
            return Err(WorkerPolicyError::InvalidEvaluationPolicy {
                policy: self.policy.to_string(),
            });
        }
        self.limits
            .validate()
            .map_err(|error| WorkerPolicyError::InvalidLimits {
                reason: error.to_string(),
            })?;
        self.deadline.validate(WorkerResource::Deadline)?;
        self.memory.validate(WorkerResource::Memory)?;
        self.recursion.validate(WorkerResource::Recursion)?;

        if self.policy.as_str() == R0_NICKEL_EVALUATION_POLICY {
            if self.limits != NickelEvaluationLimits::default() {
                return Err(WorkerPolicyError::R0LimitsMismatch);
            }
            for (resource, capability) in [
                (WorkerResource::Deadline, &self.deadline),
                (WorkerResource::Memory, &self.memory),
                (WorkerResource::Recursion, &self.recursion),
            ] {
                if capability.strength != EnforcementStrength::Hard {
                    return Err(WorkerPolicyError::R0CapabilityUnavailable { resource });
                }
            }
            self.validate_r0_memory_backend_allowlist()?;
        }
        Ok(())
    }

    /// Validates a worker-returned receipt against the controller's request.
    ///
    /// Both receipts must be independently valid. Every policy, worker-host
    /// target, limit, and capability field must then match exactly; a worker
    /// cannot weaken or substitute a backend while returning an otherwise
    /// well-formed receipt. The controller must construct `expected` from the
    /// containment backend it actually installed. Structural receipt
    /// validation alone cannot prove backend installation or host-platform
    /// support.
    ///
    /// # Errors
    ///
    /// Returns [`WorkerPolicyError`] when either receipt is invalid or when the
    /// returned receipt differs from `expected`.
    pub fn validate_against_expected(&self, expected: &Self) -> Result<(), WorkerPolicyError> {
        expected.validate()?;
        self.validate()?;

        let mismatch = if self.policy != expected.policy {
            Some("policy")
        } else if self.target != expected.target {
            Some("target")
        } else if self.limits != expected.limits {
            Some("limits")
        } else if self.deadline != expected.deadline {
            Some("deadline")
        } else if self.memory != expected.memory {
            Some("memory")
        } else if self.recursion != expected.recursion {
            Some("recursion")
        } else {
            None
        };
        if let Some(field) = mismatch {
            Err(WorkerPolicyError::ReceiptMismatch { field })
        } else {
            Ok(())
        }
    }

    fn validate_r0_memory_backend_allowlist(&self) -> Result<(), WorkerPolicyError> {
        let actual = self.memory.backend.as_ref().map(StableId::as_str);
        if actual == Some(R0_WINDOWS_MEMORY_BACKEND) || actual == Some(R0_LINUX_MEMORY_BACKEND) {
            Ok(())
        } else {
            Err(WorkerPolicyError::InvalidR0MemoryBackend {
                actual: actual.map(str::to_owned),
            })
        }
    }
}

/// Stable worker policy validation failure.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum WorkerPolicyError {
    /// The policy did not use the owned versioned evaluation-policy kind.
    #[error("invalid versioned Nickel evaluation policy `{policy}`")]
    InvalidEvaluationPolicy {
        /// Rejected policy identity.
        policy: String,
    },
    /// Effective evaluator limits were invalid.
    #[error("invalid effective evaluator limits: {reason}")]
    InvalidLimits {
        /// Underlying typed-limit validation message.
        reason: String,
    },
    /// A capability strength did not agree with backend presence.
    #[error("{resource:?} enforcement strength disagrees with backend presence")]
    BackendStrengthMismatch {
        /// Resource whose receipt was inconsistent.
        resource: WorkerResource,
    },
    /// A backend ID did not use the resource's versioned backend kind.
    #[error("invalid {resource:?} enforcement backend `{backend}`")]
    InvalidBackend {
        /// Resource whose backend was invalid.
        resource: WorkerResource,
        /// Rejected backend ID.
        backend: String,
    },
    /// The frozen R0 policy was paired with different effective limits.
    #[error("the production R0 policy requires its frozen evaluation limits")]
    R0LimitsMismatch,
    /// The production R0 policy lacked one hard enforcement capability.
    #[error("the production R0 policy requires hard {resource:?} enforcement")]
    R0CapabilityUnavailable {
        /// Resource that was not hard enforced.
        resource: WorkerResource,
    },
    /// The selected R0 memory backend was not one of the frozen hard backends.
    #[error("invalid production R0 memory backend {actual:?}")]
    InvalidR0MemoryBackend {
        /// Backend carried by the receipt, if any.
        actual: Option<String>,
    },
    /// A valid worker receipt differed from the controller's expected receipt.
    #[error("worker policy receipt field `{field}` differs from the controller request")]
    ReceiptMismatch {
        /// First mismatched field in stable comparison order.
        field: &'static str,
    },
}

impl WorkerPolicyError {
    /// Returns the stable diagnostic code for worker policy failures.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::ReceiptMismatch { .. } => "compose.worker_protocol",
            _ => "compose.worker_capability",
        }
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;

    #[test]
    fn production_r0_requires_three_hard_versioned_backends() {
        let receipt = production_receipt();
        assert!(receipt.validate().is_ok());

        let mut unsupported = receipt.clone();
        unsupported.recursion = EnforcementCapability::unsupported();
        assert!(matches!(
            unsupported.validate(),
            Err(WorkerPolicyError::R0CapabilityUnavailable {
                resource: WorkerResource::Recursion
            })
        ));
        assert_eq!(
            unsupported.validate().err().map(|error| error.code()),
            Some("compose.worker_capability")
        );
    }

    #[test]
    fn backend_kind_and_strength_are_part_of_the_receipt_contract() {
        let mut receipt = production_receipt();
        receipt.memory.backend = Some(stable_id(
            "latticeaxiom:worker-deadline-backend/not-memory@1",
        ));
        assert!(matches!(
            receipt.validate(),
            Err(WorkerPolicyError::InvalidBackend {
                resource: WorkerResource::Memory,
                ..
            })
        ));

        receipt.memory = EnforcementCapability {
            strength: EnforcementStrength::Unsupported,
            backend: Some(stable_id(
                "latticeaxiom:worker-memory-backend/windows-job@1",
            )),
        };
        assert!(matches!(
            receipt.validate(),
            Err(WorkerPolicyError::BackendStrengthMismatch {
                resource: WorkerResource::Memory
            })
        ));
    }

    #[test]
    fn production_memory_backend_uses_the_exact_frozen_allowlist() {
        let windows = production_receipt();
        assert!(windows.validate().is_ok());

        let mut linux = windows.clone();
        linux.target = target("x86_64-unknown-linux-gnu");
        linux.memory = EnforcementCapability::hard(stable_id(R0_LINUX_MEMORY_BACKEND));
        assert!(linux.validate().is_ok());

        let mut invented = linux;
        invented.memory = EnforcementCapability::hard(stable_id(
            "latticeaxiom:worker-memory-backend/sampled-rss@1",
        ));
        assert!(matches!(
            invented.validate(),
            Err(WorkerPolicyError::InvalidR0MemoryBackend { .. })
        ));

        let mut self_asserted_host = windows;
        self_asserted_host.target = target("aarch64-apple-darwin");
        // Structural validation cannot prove the controller's platform or
        // installed backend; the controller's expected receipt supplies that
        // authority and must be compared before accepting worker output.
        assert!(self_asserted_host.validate().is_ok());
    }

    #[test]
    fn every_policy_requires_the_owned_kind_and_explicit_major() {
        let mut wrong_kind = production_receipt();
        wrong_kind.policy = stable_id("latticeaxiom:worker-policy/r0@1");
        assert!(matches!(
            wrong_kind.validate(),
            Err(WorkerPolicyError::InvalidEvaluationPolicy { .. })
        ));

        let mut wrong_namespace = production_receipt();
        wrong_namespace.policy = stable_id("example:nickel-evaluation-policy/tool@1");
        assert!(matches!(
            wrong_namespace.validate(),
            Err(WorkerPolicyError::InvalidEvaluationPolicy { .. })
        ));

        let mut unversioned = production_receipt();
        unversioned.policy = stable_id("latticeaxiom:nickel-evaluation-policy/tool");
        assert!(matches!(
            unversioned.validate(),
            Err(WorkerPolicyError::InvalidEvaluationPolicy { .. })
        ));

        let alternative = EvaluationPolicyReceipt {
            policy: stable_id("latticeaxiom:nickel-evaluation-policy/tool@1"),
            target: target("wasm32-unknown"),
            limits: NickelEvaluationLimits::default(),
            deadline: EnforcementCapability::unsupported(),
            memory: EnforcementCapability::unsupported(),
            recursion: EnforcementCapability::unsupported(),
        };
        assert!(alternative.validate().is_ok());
    }

    #[test]
    fn controller_rejects_any_valid_but_unexpected_receipt_field() {
        let expected = production_receipt();
        assert!(expected.validate_against_expected(&expected).is_ok());

        let mut changed_capability = expected.clone();
        changed_capability.deadline = EnforcementCapability::hard(stable_id(
            "latticeaxiom:worker-deadline-backend/alternate-monotonic@1",
        ));
        let error = changed_capability
            .validate_against_expected(&expected)
            .err()
            .unwrap_or_else(|| panic!("changed capability unexpectedly matched"));
        assert_eq!(
            error,
            WorkerPolicyError::ReceiptMismatch { field: "deadline" }
        );
        assert_eq!(error.code(), "compose.worker_protocol");

        let mut linux_expected = expected.clone();
        linux_expected.target = target("x86_64-unknown-linux-gnu");
        linux_expected.memory = EnforcementCapability::hard(stable_id(R0_LINUX_MEMORY_BACKEND));
        let mut lied_about_backend = linux_expected.clone();
        lied_about_backend.memory =
            EnforcementCapability::hard(stable_id(R0_WINDOWS_MEMORY_BACKEND));
        assert!(matches!(
            lied_about_backend.validate_against_expected(&linux_expected),
            Err(WorkerPolicyError::ReceiptMismatch { field: "memory" })
        ));

        let mut unsupported_host = expected.clone();
        unsupported_host.target = target("aarch64-apple-darwin");
        unsupported_host.memory = EnforcementCapability::unsupported();
        assert!(matches!(
            expected.validate_against_expected(&unsupported_host),
            Err(WorkerPolicyError::R0CapabilityUnavailable {
                resource: WorkerResource::Memory
            })
        ));
    }

    #[test]
    fn receipt_serde_requires_target_and_denies_unknown_fields() {
        let receipt = production_receipt();
        let value = serde_json::to_value(&receipt)
            .unwrap_or_else(|error| panic!("receipt serialization failed: {error}"));
        assert_eq!(
            serde_json::from_value::<EvaluationPolicyReceipt>(value.clone()).ok(),
            Some(receipt)
        );

        let mut unknown = value.clone();
        let serde_json::Value::Object(fields) = &mut unknown else {
            panic!("receipt did not serialize as an object");
        };
        fields.insert("ambient_backend".to_owned(), serde_json::Value::Null);
        assert!(serde_json::from_value::<EvaluationPolicyReceipt>(unknown).is_err());

        let mut missing_target = value;
        let serde_json::Value::Object(fields) = &mut missing_target else {
            panic!("receipt did not serialize as an object");
        };
        fields.remove("target");
        assert!(serde_json::from_value::<EvaluationPolicyReceipt>(missing_target).is_err());
    }

    fn production_receipt() -> EvaluationPolicyReceipt {
        EvaluationPolicyReceipt {
            policy: stable_id(R0_NICKEL_EVALUATION_POLICY),
            target: target("x86_64-pc-windows-msvc"),
            limits: NickelEvaluationLimits::default(),
            deadline: EnforcementCapability::hard(stable_id(
                "latticeaxiom:worker-deadline-backend/supervisor-monotonic@1",
            )),
            memory: EnforcementCapability::hard(stable_id(
                "latticeaxiom:worker-memory-backend/windows-job@1",
            )),
            recursion: EnforcementCapability::hard(stable_id(
                "latticeaxiom:worker-recursion-backend/nickel-vm-frames@1",
            )),
        }
    }

    fn target(value: &str) -> TargetTriple {
        value
            .parse()
            .unwrap_or_else(|error| panic!("fixture target `{value}` is invalid: {error}"))
    }

    fn stable_id(value: &str) -> StableId {
        StableId::from_str(value)
            .unwrap_or_else(|error| panic!("fixture stable ID `{value}` is invalid: {error}"))
    }
}
