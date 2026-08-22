//! Canonical user-settings persistence across replacement-process hops.

use std::collections::BTreeSet;
use std::path::Path;

use latticeaxiom_input::{BindingProfileV1 as InputBindingProfile, InputBindingV1, KeyModifierV1};
use latticeaxiom_runtime_contracts::{
    BindingProfileV1 as StoredBindingProfile, FilesystemLocalSettingsStore,
    InputBindingV1 as StoredBinding, KeyboardModifiersV1, KnownInputBindingV1,
    LatticeLocalSettingsV1, LocalSettingsOrigin, LocalSettingsStore,
    MouseButtonV1 as StoredMouseButton,
};
use thiserror::Error;

/// Failure to load or persist local user settings.
#[derive(Debug, Error)]
pub enum HostSettingsError {
    /// The local-settings store rejected a read or write.
    #[error(transparent)]
    Persist(#[from] latticeaxiom_runtime_contracts::LocalSettingsPersistError),
    /// A stored binding could not be converted into the input catalog profile.
    #[error("stored binding profile is not representable: {reason}")]
    Binding {
        /// Diagnostic.
        reason: String,
    },
}

/// Loaded user settings that survive shell↔world process replacement.
#[derive(Clone, Debug, PartialEq)]
pub struct HostUserSettings {
    envelope: LatticeLocalSettingsV1,
    profile: InputBindingProfile,
}

impl HostUserSettings {
    /// Loads or creates the canonical local-settings envelope under `root`.
    ///
    /// # Errors
    ///
    /// Returns [`HostSettingsError`] when the store cannot be opened or decoded.
    pub fn load(root: impl AsRef<Path>) -> Result<Self, HostSettingsError> {
        let store = FilesystemLocalSettingsStore::open(root)?;
        let loaded = store.load()?;
        let envelope = match loaded.origin() {
            LocalSettingsOrigin::Complete => loaded.envelope().clone(),
            LocalSettingsOrigin::Missing | LocalSettingsOrigin::Isolated { .. } => {
                LatticeLocalSettingsV1::empty()
            }
        };
        let profile = stored_profile_to_input(envelope.binding_profile())?;
        Ok(Self { envelope, profile })
    }

    /// Returns the compiled-input binding profile.
    #[must_use]
    pub const fn binding_profile(&self) -> &InputBindingProfile {
        &self.profile
    }

    /// Returns the last confirmed settings transaction revision.
    #[must_use]
    pub fn transaction_revision(&self) -> u64 {
        self.envelope.transaction_revision().get()
    }

    /// Replaces the binding profile and persists it atomically.
    ///
    /// # Errors
    ///
    /// Returns [`HostSettingsError`] when conversion or publication fails.
    pub fn persist_binding_profile(
        &mut self,
        root: impl AsRef<Path>,
        profile: InputBindingProfile,
    ) -> Result<(), HostSettingsError> {
        let stored = input_profile_to_stored(&profile)?;
        self.envelope = self
            .envelope
            .with_user_commit(self.envelope.user().clone(), stored);
        self.profile = profile;
        let store = FilesystemLocalSettingsStore::open(root)?;
        store.persist(&self.envelope)?;
        Ok(())
    }
}

fn stored_profile_to_input(
    stored: &StoredBindingProfile,
) -> Result<InputBindingProfile, HostSettingsError> {
    let mut profile = InputBindingProfile::empty();
    for (action, bindings) in stored.overrides() {
        let converted = bindings
            .iter()
            .map(stored_binding_to_input)
            .collect::<Result<Vec<_>, _>>()?;
        profile.set_override(action.clone(), converted);
    }
    Ok(profile)
}

fn input_profile_to_stored(
    profile: &InputBindingProfile,
) -> Result<StoredBindingProfile, HostSettingsError> {
    let mut overrides = std::collections::BTreeMap::new();
    for (action, bindings) in &profile.overrides {
        let converted = bindings
            .iter()
            .map(input_binding_to_stored)
            .collect::<Result<Vec<_>, _>>()?;
        overrides.insert(action.clone(), converted);
    }
    StoredBindingProfile::new(1, overrides).map_err(|error| HostSettingsError::Binding {
        reason: error.to_string(),
    })
}

fn stored_binding_to_input(binding: &StoredBinding) -> Result<InputBindingV1, HostSettingsError> {
    match binding {
        StoredBinding::Known(KnownInputBindingV1::Keyboard { usage, modifiers }) => {
            Ok(InputBindingV1::Keyboard {
                usage: usage.clone(),
                modifiers: stored_modifiers(*modifiers),
            })
        }
        StoredBinding::Known(KnownInputBindingV1::MouseButton { button }) => {
            Ok(InputBindingV1::MouseButton {
                button: match button {
                    StoredMouseButton::Left => latticeaxiom_input::MouseButtonV1::Left,
                    StoredMouseButton::Right => latticeaxiom_input::MouseButtonV1::Right,
                    StoredMouseButton::Middle => latticeaxiom_input::MouseButtonV1::Middle,
                    StoredMouseButton::Extra(_) => {
                        return Err(HostSettingsError::Binding {
                            reason: "extra mouse buttons are not in the V1 input catalog"
                                .to_owned(),
                        });
                    }
                },
            })
        }
        StoredBinding::Known(KnownInputBindingV1::MouseMotion) => {
            Ok(InputBindingV1::MouseMotion { sensitivity: 0.002 })
        }
        StoredBinding::Known(_) | StoredBinding::Unknown(_) => Err(HostSettingsError::Binding {
            reason: "stored binding kind cannot be compiled into the V1 catalog".to_owned(),
        }),
    }
}

fn input_binding_to_stored(binding: &InputBindingV1) -> Result<StoredBinding, HostSettingsError> {
    match binding {
        InputBindingV1::Keyboard { usage, modifiers } => {
            Ok(StoredBinding::Known(KnownInputBindingV1::Keyboard {
                usage: usage.clone(),
                modifiers: input_modifiers(modifiers),
            }))
        }
        InputBindingV1::MouseButton { button } => {
            Ok(StoredBinding::Known(KnownInputBindingV1::MouseButton {
                button: match button {
                    latticeaxiom_input::MouseButtonV1::Left => StoredMouseButton::Left,
                    latticeaxiom_input::MouseButtonV1::Right => StoredMouseButton::Right,
                    latticeaxiom_input::MouseButtonV1::Middle => StoredMouseButton::Middle,
                    latticeaxiom_input::MouseButtonV1::Back
                    | latticeaxiom_input::MouseButtonV1::Forward => {
                        return Err(HostSettingsError::Binding {
                            reason: "forward/back mouse buttons are not stored in V1 settings"
                                .to_owned(),
                        });
                    }
                },
            }))
        }
        _ => Err(HostSettingsError::Binding {
            reason: "gamepad and axis recipes stay package-fixed in V1".to_owned(),
        }),
    }
}

fn stored_modifiers(modifiers: KeyboardModifiersV1) -> BTreeSet<KeyModifierV1> {
    let mut set = BTreeSet::new();
    if modifiers.shift {
        set.insert(KeyModifierV1::Shift);
    }
    if modifiers.control {
        set.insert(KeyModifierV1::Control);
    }
    if modifiers.alt {
        set.insert(KeyModifierV1::Alt);
    }
    if modifiers.super_key {
        set.insert(KeyModifierV1::Super);
    }
    set
}

fn input_modifiers(modifiers: &BTreeSet<KeyModifierV1>) -> KeyboardModifiersV1 {
    KeyboardModifiersV1 {
        shift: modifiers.contains(&KeyModifierV1::Shift),
        control: modifiers.contains(&KeyModifierV1::Control),
        alt: modifiers.contains(&KeyModifierV1::Alt),
        super_key: modifiers.contains(&KeyModifierV1::Super),
    }
}
