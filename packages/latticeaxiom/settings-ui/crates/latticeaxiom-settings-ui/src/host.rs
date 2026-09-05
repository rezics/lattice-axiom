//! Host apply contract for the official settings page.

use std::collections::BTreeMap;

use latticeaxiom_core::StableId;
use latticeaxiom_runtime_contracts::{
    AUTHORED_MAX_RENDER_DISTANCE_CHUNKS, admit_render_distance_chunks,
    clamp_full_detail_distance_chunks, clamp_requested_render_distance_chunks,
    clamp_simulation_distance_chunks, full_detail_distance_setting_id, render_distance_setting_id,
    simulation_distance_setting_id,
};
use serde_json::Value;

use crate::error::SettingsPageError;
use crate::surface::{SettingsSurfaceApplyRequest, SettingsSurfaceApplyResolution};

/// Executes one frozen settings apply request.
///
/// Presentation never writes files. A memory host is valid until a real
/// registry transaction is wired; both consume the same request type.
pub trait SettingsPageHost {
    /// Returns stored values that should overlay catalog defaults.
    ///
    /// # Errors
    ///
    /// Returns [`SettingsPageError::Host`] when the backing store cannot be read.
    fn load(&self) -> Result<BTreeMap<StableId, Value>, SettingsPageError>;

    /// Applies one single-domain request and returns a typed resolution.
    ///
    /// # Errors
    ///
    /// Returns [`SettingsPageError::Host`] when the backing store rejects the
    /// request before the presentation records completion.
    fn apply(
        &mut self,
        request: &SettingsSurfaceApplyRequest,
    ) -> Result<SettingsSurfaceApplyResolution, SettingsPageError>;

    /// Returns requested, admitted, and effective values for one setting.
    #[must_use]
    fn admission(&self, setting: &StableId, requested: &Value) -> SettingsValueAdmission {
        if *setting == render_distance_setting_id() {
            let requested_chunks = requested.as_i64().unwrap_or_default();
            let requested_distance = clamp_requested_render_distance_chunks(requested_chunks);
            let admitted = admit_render_distance_chunks(
                requested_distance,
                AUTHORED_MAX_RENDER_DISTANCE_CHUNKS,
            );
            return SettingsValueAdmission {
                requested: requested.clone(),
                admitted: Value::from(admitted.chunks()),
                effective: Value::from(admitted.chunks()),
                clamp_reason: (u32::try_from(requested_chunks).ok() != Some(admitted.chunks()))
                    .then(|| "host render-distance clamp".to_owned()),
            };
        }
        if *setting == simulation_distance_setting_id() {
            return integer_distance_admission(
                requested,
                clamp_simulation_distance_chunks(requested.as_i64().unwrap_or_default()).chunks(),
                "host simulation-distance clamp",
            );
        }
        if *setting == full_detail_distance_setting_id() {
            return integer_distance_admission(
                requested,
                clamp_full_detail_distance_chunks(requested.as_i64().unwrap_or_default()).chunks(),
                "certified full-detail-distance clamp",
            );
        }
        SettingsValueAdmission {
            requested: requested.clone(),
            admitted: requested.clone(),
            effective: requested.clone(),
            clamp_reason: None,
        }
    }
}

fn integer_distance_admission(
    requested: &Value,
    admitted_chunks: u32,
    clamp_reason: &'static str,
) -> SettingsValueAdmission {
    SettingsValueAdmission {
        requested: requested.clone(),
        admitted: Value::from(admitted_chunks),
        effective: Value::from(admitted_chunks),
        clamp_reason: (requested.as_u64() != Some(u64::from(admitted_chunks)))
            .then(|| clamp_reason.to_owned()),
    }
}

/// Requested versus host-admitted projection for one row.
#[derive(Clone, Debug, PartialEq)]
pub struct SettingsValueAdmission {
    /// Value requested by the user draft.
    pub requested: Value,
    /// Value admitted after host constraints.
    pub admitted: Value,
    /// Effective runtime value.
    pub effective: Value,
    /// Why requested and admitted differ.
    pub clamp_reason: Option<String>,
}

/// In-memory host used when a durable registry transaction is unavailable.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MemorySettingsHost {
    values: BTreeMap<StableId, Value>,
}

impl MemorySettingsHost {
    /// Creates an empty host.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the stored overlay.
    #[must_use]
    pub const fn values(&self) -> &BTreeMap<StableId, Value> {
        &self.values
    }

    /// Seeds one overlay value without an apply transaction.
    pub fn seed(&mut self, setting: StableId, value: Value) {
        self.values.insert(setting, value);
    }
}

impl SettingsPageHost for MemorySettingsHost {
    fn load(&self) -> Result<BTreeMap<StableId, Value>, SettingsPageError> {
        Ok(self.values.clone())
    }

    fn apply(
        &mut self,
        request: &SettingsSurfaceApplyRequest,
    ) -> Result<SettingsSurfaceApplyResolution, SettingsPageError> {
        for (setting, value) in &request.proposed {
            self.values.insert(setting.clone(), value.clone());
        }
        Ok(SettingsSurfaceApplyResolution::Committed)
    }
}
