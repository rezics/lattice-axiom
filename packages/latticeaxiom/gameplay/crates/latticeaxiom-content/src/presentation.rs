use latticeaxiom_core::StableId;
use serde::{Deserialize, Serialize};

/// Kind of catalog row that carries a presentation binding.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ContentPresentationKindV1 {
    /// An intrinsic block definition.
    Block,
    /// An intrinsic fluid definition.
    Fluid,
}

/// Presentation binding extracted from a compiled catalog.
///
/// These rows are excluded from the authoritative catalog hash. Headless
/// omission of a presentation package therefore cannot change world bytes.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct ContentPresentationBindingV1 {
    content: StableId,
    kind: ContentPresentationKindV1,
    binding: Option<StableId>,
}

impl ContentPresentationBindingV1 {
    pub(crate) const fn new(
        content: StableId,
        kind: ContentPresentationKindV1,
        binding: Option<StableId>,
    ) -> Self {
        Self {
            content,
            kind,
            binding,
        }
    }

    /// Exact content identity.
    #[must_use]
    pub const fn content(&self) -> &StableId {
        &self.content
    }

    /// Whether the row came from a block or fluid definition.
    #[must_use]
    pub const fn kind(&self) -> ContentPresentationKindV1 {
        self.kind
    }

    /// Optional exact presentation asset.
    #[must_use]
    pub const fn binding(&self) -> Option<&StableId> {
        self.binding.as_ref()
    }
}
