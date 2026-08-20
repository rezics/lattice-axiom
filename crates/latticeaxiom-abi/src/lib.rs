//! Portable Native ABI 0.1 wire contracts and safe pre-dereference validators.
//!
//! This crate is deliberately engine-independent. It mirrors no Bevy type and
//! does not load native libraries. The generated raw bindings describe trusted
//! in-process code; they are not a sandbox or a fault-isolation mechanism.

#![allow(
    unsafe_code,
    reason = "generated foreign callback types must require unsafe invocation; this crate contains no unsafe blocks"
)]

#[cfg(not(target_endian = "little"))]
compile_error!("Portable Native ABI 0.x supports little-endian targets only");
#[cfg(not(target_pointer_width = "64"))]
compile_error!("Portable Native ABI 0.x supports 64-bit pointer targets only");

mod interface;
mod recovery;
mod validation;
mod wire;

pub use interface::{
    BuiltInInterface, CanonicalInterfaceDescriptor, InterfaceClaim, InterfaceDescriptorError,
    InterfaceFieldDescriptor, InterfaceIdentity, InterfaceIdentityError, InterfaceName,
    builtin_interface_descriptor, validate_interface_claims,
};
pub use recovery::{
    CallbackFault, CallbackPolicy, CallbackRecoveryInput, FailureScope, RecoveryAction,
    RecoveryPolicyError, StagedOutputState, WriteCommitMode, decide_recovery,
};
pub use validation::{
    AbiContractError, BatchLimits, BatchShape, ColumnShape, CommandContract, CommandKind,
    CommandLimits, CommandRecord, HandleGeneration, HandleTableSnapshot, HeaderInspection,
    HeaderPolicy, MessageContract, MessageLimits, MessageRecord, OwnedBufferShape, ScratchRequest,
    TargetPolicy, TargetRequirement, validate_batches, validate_borrowed_bytes, validate_commands,
    validate_header, validate_messages, validate_owned_buffer, validate_scratch_request,
    validate_target,
};
pub use wire::*;

/// The generated C header corresponding exactly to the compiled Rust bindings.
pub const GENERATED_C_HEADER: &str = include_str!(concat!(env!("OUT_DIR"), "/latticeaxiom_abi.h"));
