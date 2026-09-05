use serde::{Deserialize, Serialize};

use crate::{RenderDataDecl, RenderFeatureDecl, RenderPassDecl, RenderProviderDecl};

/// A tagged render registration preserving the four distinct extension levels.
///
/// Consumers should not treat this as one interchangeable hook collection:
/// data is passive presentation input, a feature is additive behavior, a pass is
/// one feature-owned execution node, and a provider owns an exactly-one mechanism.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum RenderRegistration {
    /// Typed presentation input that creates no pass.
    Data(RenderDataDecl),
    /// Additive feature that may own zero or more passes.
    Feature(RenderFeatureDecl),
    /// Feature-owned semantic execution node.
    Pass(RenderPassDecl),
    /// Exactly-one complete mechanism provider.
    Provider(RenderProviderDecl),
}
