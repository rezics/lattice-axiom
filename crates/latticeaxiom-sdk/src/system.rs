//! Portable row-kernel parameter and system-signature contracts.

use std::{
    collections::BTreeSet,
    marker::PhantomData,
    num::NonZeroU32,
    ops::{Deref, DerefMut},
};

use latticeaxiom_core::{CanonicalHash, SchemaId, StableId};
use serde::{Deserialize, Serialize};

use crate::component::{ComponentContract, ComponentSeed};

/// Required or optional component presence for one row borrow.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ComponentRequirement {
    /// The queried row must contain the component.
    Required,
    /// The queried row may omit the component.
    Optional,
}

/// Read or write authority for one row component.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ComponentAccessKind {
    /// Immutable component access.
    Read,
    /// Mutable component access.
    Write,
}

/// Stable component access included in a canonical system signature.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ComponentAccess {
    /// Stable component registration ID.
    pub component: StableId,
    /// Public ABI schema, when the component crosses a portable boundary.
    pub schema: Option<SchemaId>,
    /// Whether the component is required on every row.
    pub requirement: ComponentRequirement,
    /// Read or write authority.
    pub access: ComponentAccessKind,
}

/// Stable query-presence filter included in a canonical signature.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case", tag = "filter")]
pub enum QueryFilter {
    /// Rows must contain this component.
    With {
        /// Stable component registration ID.
        component: StableId,
    },
    /// Rows must not contain this component.
    Without {
        /// Stable component registration ID.
        component: StableId,
    },
}

/// Stable reason why a generated system has no portable callback.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum NativeStaticOnlyReason {
    /// The author explicitly selected a static-only realization.
    ExplicitPolicy,
    /// At least one function argument is outside the dual parameter whitelist.
    UnsupportedSystemParameter,
}

/// Realizations allowed by one validated system declaration.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case", tag = "kind")]
pub enum SystemPortability {
    /// The same row kernel may back static and portable batch adapters.
    Dual,
    /// Only a directly compiled static adapter may use the kernel.
    NativeStaticOnly {
        /// Stable, sorted reason codes.
        reasons: BTreeSet<NativeStaticOnlyReason>,
    },
}

/// One ordered parameter in the canonical row-kernel signature.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case", tag = "kind")]
pub enum SystemParameter {
    /// Required or optional component row access.
    Component {
        /// Source parameter ordinal.
        ordinal: u32,
        /// Stable access contract.
        value: ComponentAccess,
    },
    /// `With` or `Without` row filter.
    Filter {
        /// Source parameter ordinal.
        ordinal: u32,
        /// Stable filter contract.
        value: QueryFilter,
    },
    /// Opaque row entity key.
    RowEntity {
        /// Source parameter ordinal.
        ordinal: u32,
    },
    /// Fixed-tick context passed by value.
    FixedTick {
        /// Source parameter ordinal.
        ordinal: u32,
    },
    /// Authoritative SDK command sink.
    CommandSink {
        /// Source parameter ordinal.
        ordinal: u32,
    },
    /// Static-only host parameter excluded from the portable ABI.
    StaticOnly {
        /// Source parameter ordinal.
        ordinal: u32,
        /// Stable portability reason.
        reason: NativeStaticOnlyReason,
    },
}

/// The single authoritative access and scheduling signature for one system.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SystemSignature {
    /// Exact system registration ID.
    pub system: StableId,
    /// Versioned callback key.
    pub callback: StableId,
    /// Versioned semantic stage.
    pub stage: StableId,
    /// Optional package-local set bound to the same stage.
    pub set: Option<StableId>,
    /// Same-stage systems or sets that run before this system.
    pub after: BTreeSet<StableId>,
    /// Same-stage systems or sets that run after this system.
    pub before: BTreeSet<StableId>,
    /// Ordered row-kernel parameter model.
    pub parameters: Vec<SystemParameter>,
    /// Realizations permitted by the validated parameter model.
    pub portability: SystemPortability,
    /// Canonical hash of every preceding semantic field.
    pub signature_hash: CanonicalHash,
}

/// Opaque, generational identity for the current row entity.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RowEntityKey {
    index: u64,
    generation: NonZeroU32,
}

impl RowEntityKey {
    /// Creates a validated row entity key.
    #[must_use]
    pub const fn new(index: u64, generation: NonZeroU32) -> Self {
        Self { index, generation }
    }

    /// Returns the host-assigned table index.
    #[must_use]
    pub const fn index(self) -> u64 {
        self.index
    }

    /// Returns the non-zero generation.
    #[must_use]
    pub const fn generation(self) -> NonZeroU32 {
        self.generation
    }
}

/// Row-kernel parameter carrying the current opaque entity key.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RowEntity(pub RowEntityKey);

/// Fixed-step context copied into every row invocation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FixedTick {
    /// Monotonic authoritative tick index.
    pub tick: u64,
    /// Exact fixed delta in nanoseconds.
    pub delta_nanos: u64,
}

/// Required immutable component borrow for one row.
#[derive(Debug)]
pub struct Read<'a, T> {
    value: &'a T,
}

impl<'a, T> Read<'a, T> {
    /// Wraps a typed static or validated batch-row borrow.
    #[must_use]
    #[doc(hidden)]
    pub const fn new(value: &'a T) -> Self {
        Self { value }
    }
}

impl<T> Deref for Read<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.value
    }
}

/// Optional immutable component borrow for one row.
#[derive(Debug)]
pub struct OptionalRead<'a, T> {
    value: Option<&'a T>,
}

impl<'a, T> OptionalRead<'a, T> {
    /// Wraps an optional typed static or validated batch-row borrow.
    #[must_use]
    #[doc(hidden)]
    pub const fn new(value: Option<&'a T>) -> Self {
        Self { value }
    }

    /// Returns the optional component borrow.
    #[must_use]
    pub const fn get(&self) -> Option<&T> {
        self.value
    }
}

/// Required mutable component borrow for one row.
#[derive(Debug)]
pub struct Write<'a, T> {
    value: &'a mut T,
}

impl<'a, T> Write<'a, T> {
    /// Wraps a typed static or validated batch-row mutable borrow.
    #[must_use]
    #[doc(hidden)]
    pub const fn new(value: &'a mut T) -> Self {
        Self { value }
    }
}

impl<T> Deref for Write<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.value
    }
}

impl<T> DerefMut for Write<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.value
    }
}

/// Optional mutable component borrow for one row.
#[derive(Debug)]
pub struct OptionalWrite<'a, T> {
    value: Option<&'a mut T>,
}

impl<'a, T> OptionalWrite<'a, T> {
    /// Wraps an optional mutable row borrow.
    #[must_use]
    #[doc(hidden)]
    pub const fn new(value: Option<&'a mut T>) -> Self {
        Self { value }
    }

    /// Returns the optional mutable component borrow.
    #[must_use]
    pub fn get_mut(&mut self) -> Option<&mut T> {
        self.value.as_deref_mut()
    }
}

/// Zero-sized `With<T>` filter marker accepted by the row-kernel macro.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct With<T>(PhantomData<fn() -> T>);

/// Zero-sized `Without<T>` filter marker accepted by the row-kernel macro.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Without<T>(PhantomData<fn() -> T>);

/// Bounded SDK command sink marker for a row kernel.
///
/// The command vocabulary and transaction live in their versioned contract;
/// this value deliberately exposes no Bevy `Commands` or `World` access.
#[derive(Debug)]
pub struct CommandSink<'a>(PhantomData<&'a mut ()>);

/// Macro-level portability policy before parameter validation.
#[derive(Clone, Copy, Debug)]
#[doc(hidden)]
pub enum SystemPolicySeed {
    /// Reject any non-portable parameter during macro expansion.
    Dual,
    /// Select dual when possible and otherwise emit a stable static-only reason.
    Auto,
    /// Explicitly emit only static glue.
    NativeStaticOnly,
}

/// Macro-emitted parameter seed before stable identifier parsing.
#[derive(Clone, Debug)]
#[doc(hidden)]
pub enum ParameterSeed {
    Component {
        component: ComponentSeed,
        requirement: ComponentRequirement,
        access: ComponentAccessKind,
    },
    Filter {
        component: ComponentSeed,
        with: bool,
    },
    RowEntity,
    FixedTick,
    CommandSink,
    StaticOnly {
        reason: NativeStaticOnlyReason,
    },
}

/// Macro-emitted system seed before sealed IR validation.
#[derive(Clone, Debug)]
#[doc(hidden)]
pub struct SystemSeed {
    pub(crate) id: &'static str,
    pub(crate) callback: &'static str,
    pub(crate) stage: &'static str,
    pub(crate) set: Option<&'static str>,
    pub(crate) after: Vec<&'static str>,
    pub(crate) before: Vec<&'static str>,
    pub(crate) policy: SystemPolicySeed,
    pub(crate) parameters: Vec<ParameterSeed>,
    pub(crate) row_kernel_symbol: &'static str,
    pub(crate) rust_signature_descriptor: &'static str,
}

impl SystemSeed {
    /// Creates a seed for generated code.
    #[must_use]
    #[allow(
        clippy::too_many_arguments,
        reason = "the fields mirror the frozen system signature"
    )]
    #[doc(hidden)]
    pub fn new(
        id: &'static str,
        callback: &'static str,
        stage: &'static str,
        set: Option<&'static str>,
        after: Vec<&'static str>,
        before: Vec<&'static str>,
        policy: SystemPolicySeed,
        parameters: Vec<ParameterSeed>,
        row_kernel_symbol: &'static str,
        rust_signature_descriptor: &'static str,
    ) -> Self {
        Self {
            id,
            callback,
            stage,
            set,
            after,
            before,
            policy,
            parameters,
            row_kernel_symbol,
            rust_signature_descriptor,
        }
    }
}

/// Emits a component access seed for generated code.
#[must_use]
#[doc(hidden)]
pub fn component_parameter<T: ComponentContract>(
    requirement: ComponentRequirement,
    access: ComponentAccessKind,
) -> ParameterSeed {
    ParameterSeed::Component {
        component: T::__registration_seed(),
        requirement,
        access,
    }
}

/// Emits a component filter seed for generated code.
#[must_use]
#[doc(hidden)]
pub fn filter_parameter<T: ComponentContract>(with: bool) -> ParameterSeed {
    ParameterSeed::Filter {
        component: T::__registration_seed(),
        with,
    }
}

/// Emits an opaque entity parameter seed for generated code.
#[must_use]
#[doc(hidden)]
pub const fn row_entity_parameter() -> ParameterSeed {
    ParameterSeed::RowEntity
}

/// Emits a fixed-tick parameter seed for generated code.
#[must_use]
#[doc(hidden)]
pub const fn fixed_tick_parameter() -> ParameterSeed {
    ParameterSeed::FixedTick
}

/// Emits an SDK command-sink parameter seed for generated code.
#[must_use]
#[doc(hidden)]
pub const fn command_sink_parameter() -> ParameterSeed {
    ParameterSeed::CommandSink
}

/// Emits a static-only parameter seed for generated code.
#[must_use]
#[doc(hidden)]
pub const fn static_only_parameter(reason: NativeStaticOnlyReason) -> ParameterSeed {
    ParameterSeed::StaticOnly { reason }
}
