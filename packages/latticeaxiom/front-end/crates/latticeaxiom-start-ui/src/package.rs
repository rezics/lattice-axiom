//! Package-provider validation for the client shell graph.

use std::collections::BTreeMap;

use latticeaxiom_core::PackageName;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Exactly-one capability required by the first-version shell.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ShellCapability {
    /// Settings registry and transaction provider.
    SettingsRegistry,
    /// Mechanically generated settings surface.
    SettingsSurface,
    /// Diagnostic registry used by recovery surfaces.
    DiagnosticRegistry,
    /// Bounded world catalog and lifecycle provider.
    WorldCatalog,
    /// Root navigation, loading, and error surface.
    ClientShell,
}

impl ShellCapability {
    /// All capabilities required before a client-shell surface may be built.
    pub const REQUIRED: [Self; 5] = [
        Self::SettingsRegistry,
        Self::SettingsSurface,
        Self::DiagnosticRegistry,
        Self::WorldCatalog,
        Self::ClientShell,
    ];
}

/// One resolved package provider projected from the shell lock.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ShellPackageProvider {
    /// Logical package selected by the resolver.
    pub package: PackageName,
    /// Exactly-one capability provided by this row.
    pub capability: ShellCapability,
}

/// Validated stable package-to-capability map for `ClientShellGraph`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClientShellGraph {
    providers: BTreeMap<ShellCapability, PackageName>,
}

impl ClientShellGraph {
    /// Validates exactly one provider for every required shell capability.
    ///
    /// # Errors
    ///
    /// Returns [`ClientShellGraphError`] for a missing or duplicate provider.
    pub fn resolve(
        providers: impl IntoIterator<Item = ShellPackageProvider>,
    ) -> Result<Self, ClientShellGraphError> {
        let mut providers = providers.into_iter().collect::<Vec<_>>();
        providers.sort_by(|left, right| {
            (left.capability, &left.package).cmp(&(right.capability, &right.package))
        });

        let mut resolved = BTreeMap::new();
        for provider in providers {
            if let Some(first) = resolved.insert(provider.capability, provider.package.clone()) {
                return Err(ClientShellGraphError::DuplicateProvider {
                    capability: provider.capability,
                    first,
                    second: provider.package,
                });
            }
        }
        for capability in ShellCapability::REQUIRED {
            if !resolved.contains_key(&capability) {
                return Err(ClientShellGraphError::MissingProvider { capability });
            }
        }
        Ok(Self {
            providers: resolved,
        })
    }

    /// Returns providers in stable capability order.
    #[must_use]
    pub const fn providers(&self) -> &BTreeMap<ShellCapability, PackageName> {
        &self.providers
    }
}

/// Invalid package-driven shell closure.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum ClientShellGraphError {
    /// A mandatory capability has no provider.
    #[error("shell capability `{capability:?}` has no provider")]
    MissingProvider {
        /// Missing capability.
        capability: ShellCapability,
    },
    /// More than one provider claims an exactly-one capability.
    #[error("shell capability `{capability:?}` has duplicate providers `{first}` and `{second}`")]
    DuplicateProvider {
        /// Conflicting capability.
        capability: ShellCapability,
        /// Stable first provider after canonical input ordering.
        first: PackageName,
        /// Rejected second provider.
        second: PackageName,
    },
}
