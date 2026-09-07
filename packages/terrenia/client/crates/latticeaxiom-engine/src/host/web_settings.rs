//! Lock-selected Web settings, backed by the canonical local settings store.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use bevy::prelude::{Resource, World};
use latticeaxiom_compose::{SettingAuthority, SettingPredicate, SettingScope};
use latticeaxiom_core::{CanonicalHash, StableId};
use latticeaxiom_input::{
    ActionKindV1, BindingProfileV1, CompiledInputCatalogV1, InputBindingV1, RebindablePolicyV1,
};
use serde_json::{Value, json};

use crate::{
    LockVerifiedComposeImages,
    settings::{HostSettingsCatalog, HostUserSettings},
};

#[derive(Debug, Resource)]
struct WebSettings {
    root: PathBuf,
    user: HostUserSettings,
    catalog: HostSettingsCatalog,
    active_lock: CanonicalHash,
    applied: BTreeMap<StableId, Value>,
    draft: BTreeMap<StableId, Value>,
    error: Option<String>,
    restart_required: bool,
    next_request: u64,
    open: bool,
    draft_profile: BindingProfileV1,
    input_images: Option<LockVerifiedComposeImages>,
    draft_input: Option<CompiledInputCatalogV1>,
    applied_input: Option<CompiledInputCatalogV1>,
    owners: Vec<Value>,
}

/// Installs the actual catalog and persisted settings for this client lock.
pub(super) fn install(
    world: &mut World,
    workspace: &Path,
    images: &LockVerifiedComposeImages,
) -> Result<(), String> {
    let catalog = crate::settings::compile_lock_selected_settings(images)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "The selected client has no settings registry".to_owned())?;
    let root = workspace.join("run/user");
    let user = HostUserSettings::load(&root).map_err(|error| error.to_string())?;
    let mut state = WebSettings::new(root, user, catalog, images.product_lock_hash())?;
    state.draft_input = crate::input::compile_lock_selected_input(images, &state.draft_profile)
        .map_err(|error| error.to_string())?;
    state.input_images = Some(images.clone());
    state.applied_input = state.draft_input.clone();
    state.owners = images.images().graph().packages.keys().map(|owner| json!({
        "id":owner.as_str(),"name":owner.as_str(),
        "hasSettings":state.catalog.as_validated().as_catalog().runtime.values().any(|spec|spec.declared_by==*owner)
    })).collect();
    super::pause::apply_web_runtime_values(world, &state.applied, state.next_request)
        .map_err(|error| error.to_string())?;
    if let Some(compiled) = &state.draft_input {
        apply_bindings(world, compiled);
    }
    world.insert_resource(state);
    Ok(())
}

impl WebSettings {
    fn new(
        root: PathBuf,
        user: HostUserSettings,
        catalog: HostSettingsCatalog,
        active_lock: CanonicalHash,
    ) -> Result<Self, String> {
        let applied = user
            .effective_snapshot(&catalog, active_lock)
            .map_err(|error| error.to_string())?
            .values()
            .iter()
            .map(|(id, value)| (id.clone(), value.value.clone()))
            .collect::<BTreeMap<_, _>>();
        super::pause::validate_web_runtime_values(&applied).map_err(|error| error.to_string())?;
        Ok(Self {
            root,
            draft_profile: user.binding_profile().clone(),
            user,
            catalog,
            active_lock,
            draft: applied.clone(),
            applied,
            error: None,
            restart_required: false,
            next_request: 1,
            open: false,
            input_images: None,
            draft_input: None,
            applied_input: None,
            owners: Vec::new(),
        })
    }

    fn bind(&mut self, method: &str, params: &Value) -> Result<(), String> {
        let id = params
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| "An action ID is required".to_owned())?
            .parse::<StableId>()
            .map_err(|error| error.to_string())?;
        let images = self
            .input_images
            .as_ref()
            .ok_or_else(|| "Input catalog is unavailable".to_owned())?;
        let action = self
            .draft_input
            .as_ref()
            .and_then(|catalog| catalog.action_by_id(&id))
            .ok_or_else(|| format!("Unknown input action `{id}`"))?;
        if action.rebindable != RebindablePolicyV1::KeyboardMouse
            || action.kind != ActionKindV1::Button
        {
            return Err("This input action is fixed by its package".to_owned());
        }
        let mut profile = self.draft_profile.clone();
        match method {
            "settings.resetBinding" => profile.clear_override(&id),
            "settings.unbind" => profile.set_override(id, Vec::new()),
            _ => {
                let code = params
                    .get("code")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "A physical key code is required".to_owned())?;
                let modifiers = params
                    .get("modifiers")
                    .and_then(Value::as_array)
                    .map(|values| {
                        values
                            .iter()
                            .map(|value| {
                                match value
                                    .as_str()
                                    .unwrap_or_default()
                                    .to_ascii_lowercase()
                                    .as_str()
                                {
                                    "shift" => Ok("Shift"),
                                    "control" | "ctrl" => Ok("Control"),
                                    "alt" => Ok("Alt"),
                                    "super" | "meta" => Ok("Super"),
                                    _ => Err("Unknown keyboard modifier".to_owned()),
                                }
                            })
                            .collect::<Result<Vec<_>, _>>()
                    })
                    .transpose()?
                    .unwrap_or_default();
                let binding = InputBindingV1::try_from(
                    json!({"kind":"keyboard","usage":code,"modifiers":modifiers}),
                )
                .map_err(|error| error.to_string())?;
                profile.set_override(id, vec![binding]);
            }
        }
        // Compilation validates keys, action kind, locked actions and same-context
        // conflicts before accepting a draft or changing the live input map.
        let compiled = crate::input::compile_lock_selected_input(images, &profile)
            .map_err(|error| error.to_string())?;
        self.draft_profile = profile;
        self.draft_input = compiled;
        Ok(())
    }

    fn cancel(&mut self) -> Result<(), String> {
        self.draft.clone_from(&self.applied);
        self.draft_profile = self.user.binding_profile().clone();
        if let Some(images) = &self.input_images {
            self.draft_input =
                crate::input::compile_lock_selected_input(images, &self.draft_profile)
                    .map_err(|error| error.to_string())?;
        }
        Ok(())
    }

    fn set(&mut self, id: &str, value: Value) -> Result<(), String> {
        let id = id.parse::<StableId>().map_err(|error| error.to_string())?;
        let spec = self
            .catalog
            .as_validated()
            .as_catalog()
            .runtime
            .get(&id)
            .ok_or_else(|| format!("Unknown setting `{id}`"))?;
        if spec.authority != SettingAuthority::LocalUser
            || !spec.allowed_scopes.contains(&SettingScope::User)
        {
            return Err(format!("Setting `{id}` cannot be edited by this client"));
        }
        if !predicate(spec.visibility.as_ref(), &self.draft)
            || !predicate(spec.enabled_when.as_ref(), &self.draft)
        {
            return Err(format!("Setting `{id}` is currently unavailable"));
        }
        spec.value_type
            .validate_value(&value)
            .map_err(|error| format!("{id}: {error}"))?;
        self.draft.insert(id, value);
        Ok(())
    }

    fn reset(&mut self, id: Option<&str>, owner: Option<&str>) -> Result<(), String> {
        let rows = &self.catalog.as_validated().as_catalog().runtime;
        if let Some(id) = id {
            let id = id.parse::<StableId>().map_err(|error| error.to_string())?;
            if !rows.contains_key(&id) {
                return Err(format!("Unknown setting `{id}`"));
            }
        }
        for (key, spec) in rows {
            if id.is_none_or(|id| key.as_str() == id)
                && owner.is_none_or(|owner| spec.declared_by.as_str() == owner)
                && spec.authority == SettingAuthority::LocalUser
                && spec.allowed_scopes.contains(&SettingScope::User)
            {
                self.draft.insert(key.clone(), spec.default.clone());
            }
        }
        Ok(())
    }

    fn apply(&mut self, world: &mut World) -> Result<(), String> {
        let proposed = self
            .draft
            .iter()
            .filter(|(id, value)| self.applied.get(*id) != Some(*value))
            .map(|(id, value)| (id.clone(), value.clone()))
            .collect::<BTreeMap<_, _>>();
        if proposed.is_empty() && self.draft_profile == *self.user.binding_profile() {
            return Ok(());
        }
        super::pause::validate_web_runtime_values(&self.draft)
            .map_err(|error| error.to_string())?;
        self.next_request = self.next_request.saturating_add(1);
        super::pause::apply_web_runtime_values(world, &self.draft, self.next_request)
            .map_err(|error| error.to_string())?;
        match self.user.persist_user_draft(
            &self.root,
            &self.catalog,
            self.active_lock,
            &proposed,
            self.draft_profile.clone(),
        ) {
            Ok(_) => {
                self.applied.clone_from(&self.draft);
                self.applied_input = self.draft_input.clone();
                if let Some(compiled) = &self.draft_input {
                    apply_bindings(world, compiled);
                }
                Ok(())
            }
            Err(error) => {
                if error.requires_safe_process_restart() {
                    self.restart_required = true;
                    if error.proposed_value_is_visible_but_durability_uncertain() {
                        self.applied.clone_from(&self.draft);
                    }
                    return Err(format!(
                        "Settings publication needs a safe restart: {error}"
                    ));
                }
                self.next_request = self.next_request.saturating_add(1);
                if let Err(rollback) =
                    super::pause::apply_web_runtime_values(world, &self.applied, self.next_request)
                {
                    self.restart_required = true;
                    return Err(format!(
                        "Settings were not saved and runtime rollback failed: {error}; {rollback}"
                    ));
                }
                Err(format!(
                    "Settings were not saved; runtime restored: {error}"
                ))
            }
        }
    }
}

/// Projects only real package declarations, effective values and current drafts.
pub(super) fn snapshot(world: &World) -> Value {
    let Some(state) = world.get_resource::<WebSettings>() else {
        return json!({"revision":0,"rows":[],"dirty":false,"message":"Settings are unavailable"});
    };
    let mut specs = state
        .catalog
        .as_validated()
        .as_catalog()
        .runtime
        .values()
        .collect::<Vec<_>>();
    specs.sort_by(|a, b| {
        (&a.declared_by, &a.category, a.order, &a.id).cmp(&(
            &b.declared_by,
            &b.category,
            b.order,
            &b.id,
        ))
    });
    let rows = specs.into_iter().filter(|spec| predicate(spec.visibility.as_ref(), &state.draft)).map(|spec| json!({
        "id":spec.id,"owner":spec.declared_by,"category":spec.category,
        "label":label(spec.id.as_str()),"description":description(spec.id.as_str()),
        "value":state.draft.get(&spec.id),"applied":state.applied.get(&spec.id),"defaultValue":spec.default,
        "schema":spec.value_type,"impact":spec.apply_impact,
        "control":match spec.value_type { latticeaxiom_compose::ValueType::Bool => "toggle", latticeaxiom_compose::ValueType::Enum { .. } => "select", latticeaxiom_compose::ValueType::Integer { .. } | latticeaxiom_compose::ValueType::Number { .. } => "number", _ => "text" },
        "editable":!state.restart_required && spec.authority == SettingAuthority::LocalUser
            && spec.allowed_scopes.contains(&SettingScope::User) && predicate(spec.enabled_when.as_ref(), &state.draft),
        "visible":predicate(spec.visibility.as_ref(), &state.draft)
        ,"reason":if state.restart_required {Some("Restart required")} else if !predicate(spec.enabled_when.as_ref(), &state.draft) {Some("Enable the related setting first")} else {None}
    })).collect::<Vec<_>>();
    let bindings = state.draft_input.as_ref().map(|catalog| catalog.controls_rows().into_iter().map(|row| {
        let keyboard = row.keyboard_mouse_bindings.iter().find_map(|binding| match binding { InputBindingV1::Keyboard { usage, modifiers } => Some((usage, modifiers)), _ => None });
        json!({
        "id":row.action_id,"label":label(row.action_id.as_str()),"context":row.context,
        "description":"Select a key, then Apply to save. Conflicting bindings must be cleared first.",
        "display":if row.keyboard_mouse_bindings.is_empty() {"Unbound".to_owned()} else {row.keyboard_mouse_bindings.iter().map(binding_label).collect::<Vec<_>>().join(" / ")},
        "code":keyboard.map(|(usage, _)| usage),"modifiers":keyboard.map(|(_, modifiers)| modifiers.iter().map(|modifier|format!("{modifier:?}").to_ascii_lowercase()).collect::<Vec<_>>()).unwrap_or_default(),
        "bindings":row.keyboard_mouse_bindings,"editable":!state.restart_required && catalog.action_by_id(&row.action_id).is_some_and(|action|action.kind == ActionKindV1::Button),
    })}).collect::<Vec<_>>()).unwrap_or_default();
    json!({"open":state.open,"revision":state.user.transaction_revision(),"rows":rows,"bindings":bindings,"owners":state.owners,"dirty":state.applied != state.draft || state.draft_profile != *state.user.binding_profile(),
        "message":state.error,"restartRequired":state.restart_required})
}

/// Handles a bounded settings operation from the Web bridge.
pub(super) fn handle(world: &mut World, method: &str, params: &Value) -> Result<(), String> {
    let Some(mut state) = world.remove_resource::<WebSettings>() else {
        return Err("Settings are unavailable".to_owned());
    };
    let result = if state.restart_required && !matches!(method, "settings.open" | "settings.cancel")
    {
        Err("Settings require a safe restart before further edits".to_owned())
    } else {
        match method {
            "settings.open" => {
                let result = if state.open { Ok(()) } else { state.cancel() };
                state.open = true;
                result
            }
            "settings.set" => params
                .get("id")
                .and_then(Value::as_str)
                .ok_or_else(|| "A setting ID is required".to_owned())
                .and_then(|id| {
                    params
                        .get("value")
                        .cloned()
                        .ok_or_else(|| "A setting value is required".to_owned())
                        .and_then(|value| state.set(id, value))
                }),
            "settings.reset" => state.reset(
                params.get("id").and_then(Value::as_str),
                params.get("owner").and_then(Value::as_str),
            ),
            "settings.cancel" => {
                state.open = false;
                state.cancel()
            }
            "settings.apply" => state.apply(world),
            "settings.bind" | "settings.unbind" | "settings.resetBinding" => {
                state.bind(method, params)
            }
            _ => Err(format!("Unknown settings method `{method}`")),
        }
    };
    state.error = result.as_ref().err().cloned();
    world.insert_resource(state);
    result
}

fn apply_bindings(world: &mut World, compiled: &CompiledInputCatalogV1) {
    let maps = latticeaxiom_player::leafwing_maps_from_catalog(compiled);
    let entities = world.query_filtered::<bevy::prelude::Entity, bevy::prelude::With<latticeaxiom_player::LocalPlayerInput>>().iter(world).collect::<Vec<_>>();
    for entity in entities {
        world
            .entity_mut(entity)
            .insert(latticeaxiom_player::LocalPlayerClientInputBundle::from_compiled(&maps));
    }
    if let Some(mut surface) = world.get_resource_mut::<latticeaxiom_player::SurfaceActionFrame>() {
        surface.clear();
    }
    world.insert_resource(maps);
}

/// Returns the confirmed value; unsaved drafts never change game presentation.
pub(super) fn effective(world: &World, id: &str) -> Option<Value> {
    let id = id.parse::<StableId>().ok()?;
    world
        .get_resource::<WebSettings>()?
        .applied
        .get(&id)
        .cloned()
}

/// Confirmed key mappings for when the WebView owns keyboard focus.
pub(super) fn surface_shortcuts(world: &World) -> Vec<Value> {
    let Some(input) = world
        .get_resource::<WebSettings>()
        .and_then(|state| state.applied_input.as_ref())
    else {
        return Vec::new();
    };
    input.actions().iter().filter_map(|row| row.client_surface_action.map(|action| (row, action)))
        .flat_map(|(row, action)| row.effective_bindings.iter().filter_map(move |binding| {
            let InputBindingV1::Keyboard { usage, modifiers } = binding else { return None; };
            Some(json!({"code":usage,"modifiers":modifiers.iter().map(|modifier|format!("{modifier:?}").to_ascii_lowercase()).collect::<Vec<_>>(),"action":action}))
        })).collect()
}

fn predicate(condition: Option<&SettingPredicate>, values: &BTreeMap<StableId, Value>) -> bool {
    match condition {
        None => true,
        Some(SettingPredicate::Equals { setting, value }) => values.get(setting) == Some(value),
        Some(SettingPredicate::All { predicates }) => {
            predicates.iter().all(|p| predicate(Some(p), values))
        }
        Some(SettingPredicate::Any { predicates }) => {
            predicates.iter().any(|p| predicate(Some(p), values))
        }
        Some(SettingPredicate::Not { predicate: inner }) => !predicate(Some(inner), values),
    }
}

fn binding_label(binding: &InputBindingV1) -> String {
    match binding {
        InputBindingV1::Keyboard { usage, modifiers } => {
            let mut keys = modifiers
                .iter()
                .map(|modifier| format!("{modifier:?}"))
                .collect::<Vec<_>>();
            keys.push(
                usage
                    .strip_prefix("Key")
                    .or_else(|| usage.strip_prefix("Digit"))
                    .unwrap_or(usage)
                    .to_owned(),
            );
            keys.join(" + ")
        }
        InputBindingV1::KeyboardVirtualDPad {
            up,
            down,
            left,
            right,
            ..
        } => format!("{up} / {left} / {down} / {right}"),
        InputBindingV1::MouseButton { button } => format!("Mouse {button:?}"),
        InputBindingV1::MouseMotion { .. } => "Mouse movement".to_owned(),
        InputBindingV1::MouseWheel { axis } => format!("Mouse wheel {axis:?}"),
        InputBindingV1::MouseWheelDirection { direction } => format!("Wheel {direction:?}"),
        _ => "Package-defined input".to_owned(),
    }
}

fn label(id: &str) -> String {
    match id {
        "latticeaxiom:setting/inspect/enabled" => "Show targeted block information".to_owned(),
        "latticeaxiom:setting/inspect/anchor" => "Target information position".to_owned(),
        "latticeaxiom:setting/observability/detail" => "F3 debug detail".to_owned(),
        "latticeaxiom:setting/view-distance" => "Render distance".to_owned(),
        "latticeaxiom:setting/video/vsync" => "Vertical synchronization".to_owned(),
        _ => {
            let name = id
                .rsplit('/')
                .next()
                .unwrap_or(id)
                .split('@')
                .next()
                .unwrap_or(id)
                .replace('-', " ");
            let mut letters = name.chars();
            letters.next().map_or(name.clone(), |first| {
                first.to_uppercase().collect::<String>() + letters.as_str()
            })
        }
    }
}

fn description(id: &str) -> &'static str {
    match id {
        "latticeaxiom:setting/inspect/enabled" => {
            "Show information only while the crosshair targets a block. Empty space shows no panel."
        }
        "latticeaxiom:setting/inspect/anchor" => {
            "Choose the top-edge position of the targeted block panel."
        }
        "latticeaxiom:setting/observability/detail" => {
            "F3 opens the debug overlay. Compact shows frame and world status; detailed includes queues and rendering diagnostics."
        }
        "latticeaxiom:setting/view-distance" => {
            "Requested distance in chunks. The runtime may admit fewer chunks to remain within its memory budget."
        }
        "latticeaxiom:setting/ui-scale" => {
            "Scale menu text and controls. The layout remains scrollable in smaller windows."
        }
        _ => "Changes take effect after Apply and are saved for this device.",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use latticeaxiom_runtime_contracts::{
        SettingsCatalogFragment, SettingsCatalogPolicy, ValidatedSettingsCatalog,
    };

    const INSPECT_ENABLED: &str = "latticeaxiom:setting/inspect/enabled";
    const INSPECT_ANCHOR: &str = "latticeaxiom:setting/inspect/anchor";
    const DEBUG_DETAIL: &str = "latticeaxiom:setting/observability/detail";

    fn catalog() -> HostSettingsCatalog {
        let sources = [
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../../../latticeaxiom/settings/data/user-catalog-v1.json"
            )),
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../../../latticeaxiom/inspect/data/settings-v1.json"
            )),
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../../../latticeaxiom/observability/data/settings-v1.json"
            )),
        ];
        let fragments = sources.into_iter().map(|source| {
            SettingsCatalogFragment::from_canonical_json(source.as_bytes())
                .expect("shipped settings decode")
        });
        HostSettingsCatalog::new(
            ValidatedSettingsCatalog::compile(fragments, SettingsCatalogPolicy::default())
                .expect("shipped settings compile"),
        )
        .expect("foundation catalog remains complete")
    }

    fn state(root: &Path) -> WebSettings {
        WebSettings::new(
            root.to_owned(),
            HostUserSettings::load(root).expect("settings store opens"),
            catalog(),
            CanonicalHash::digest(b"web-settings-test"),
        )
        .expect("settings resolve")
    }

    #[test]
    fn module_settings_apply_persist_and_cancel_without_leaking_drafts() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let mut world = World::new();
        world.insert_resource(state(directory.path()));
        handle(
            &mut world,
            "settings.set",
            &json!({"id": INSPECT_ANCHOR, "value":"top-right"}),
        )
        .expect("draft changes");
        assert_eq!(effective(&world, INSPECT_ANCHOR), Some(json!("top-center")));
        handle(&mut world, "settings.apply", &json!({})).expect("settings persist");
        assert_eq!(effective(&world, INSPECT_ANCHOR), Some(json!("top-right")));
        assert_eq!(snapshot(&world)["dirty"], false);
        let reopened = state(directory.path());
        assert_eq!(
            reopened
                .applied
                .get(&INSPECT_ANCHOR.parse::<StableId>().expect("id")),
            Some(&json!("top-right"))
        );
        handle(
            &mut world,
            "settings.set",
            &json!({"id": INSPECT_ENABLED, "value":false}),
        )
        .expect("draft changes");
        handle(&mut world, "settings.cancel", &json!({})).expect("cancel succeeds");
        assert_eq!(effective(&world, INSPECT_ENABLED), Some(json!(true)));
        assert_eq!(snapshot(&world)["dirty"], false);
    }

    #[test]
    fn restart_required_locks_edits_without_trapping_the_settings_dialog() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let mut world = World::new();
        let mut settings = state(directory.path());
        settings.restart_required = true;
        settings.open = true;
        world.insert_resource(settings);
        handle(&mut world, "settings.cancel", &json!({})).expect("dialog can close");
        assert_eq!(snapshot(&world)["open"], false);
        assert!(world.resource::<WebSettings>().restart_required);
        assert!(
            handle(
                &mut world,
                "settings.set",
                &json!({"id":INSPECT_ENABLED,"value":false})
            )
            .is_err()
        );
        assert_eq!(effective(&world, INSPECT_ENABLED), Some(json!(true)));
    }

    #[test]
    fn package_reset_is_scoped_and_invalid_values_never_publish() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let mut world = World::new();
        world.insert_resource(state(directory.path()));
        handle(
            &mut world,
            "settings.set",
            &json!({"id": INSPECT_ANCHOR, "value":"bottom"}),
        )
        .expect_err("invalid enum rejected");
        handle(
            &mut world,
            "settings.set",
            &json!({"id": INSPECT_ENABLED, "value":"yes"}),
        )
        .expect_err("incorrect type rejected");
        handle(
            &mut world,
            "settings.set",
            &json!({"id": DEBUG_DETAIL, "value":"detailed"}),
        )
        .expect("debug draft");
        handle(
            &mut world,
            "settings.set",
            &json!({"id": INSPECT_ANCHOR, "value":"top-left"}),
        )
        .expect("inspect draft");
        handle(
            &mut world,
            "settings.reset",
            &json!({"owner":"@latticeaxiom/inspect"}),
        )
        .expect("package reset");
        handle(&mut world, "settings.apply", &json!({})).expect("settings persist");
        assert_eq!(effective(&world, INSPECT_ANCHOR), Some(json!("top-center")));
        assert_eq!(effective(&world, DEBUG_DETAIL), Some(json!("detailed")));
    }

    #[test]
    fn unavailable_dependent_setting_rejects_edits_and_snapshot_explains_visibility() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let mut world = World::new();
        world.insert_resource(state(directory.path()));
        handle(
            &mut world,
            "settings.set",
            &json!({"id": INSPECT_ENABLED, "value":false}),
        )
        .expect("disable draft");
        handle(
            &mut world,
            "settings.set",
            &json!({"id": INSPECT_ANCHOR, "value":"top-left"}),
        )
        .expect_err("dependent disabled");
        let snapshot = snapshot(&world);
        let row = snapshot["rows"]
            .as_array()
            .expect("rows")
            .iter()
            .find(|row| row["id"] == INSPECT_ANCHOR)
            .expect("inspect row");
        assert_eq!(row["editable"], false);
        assert_eq!(row["visible"], true);
    }

    #[test]
    fn failed_publication_restores_live_video_and_retains_the_draft() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let mut initial = state(directory.path());
        let invalid_root = directory.path().join("not-a-directory");
        std::fs::write(&invalid_root, "occupied").expect("failure fixture");
        initial.root = invalid_root;
        let before = initial.applied.clone();
        let mut world = World::new();
        world.insert_resource(crate::VideoRuntimeSettings::default());
        super::super::pause::apply_web_runtime_values(&mut world, &before, 1)
            .expect("initial video");
        let original_video = *world.resource::<crate::VideoRuntimeSettings>();
        world.insert_resource(initial);
        let vsync = latticeaxiom_runtime_contracts::video_vsync_setting_id();
        handle(
            &mut world,
            "settings.set",
            &json!({"id":vsync,"value":!original_video.vsync()}),
        )
        .expect("video draft");
        handle(&mut world, "settings.apply", &json!({})).expect_err("invalid storage path");
        assert_eq!(
            *world.resource::<crate::VideoRuntimeSettings>(),
            original_video
        );
        assert_eq!(world.resource::<WebSettings>().applied, before);
        assert_eq!(snapshot(&world)["dirty"], true);
        assert_eq!(snapshot(&world)["revision"], 0);
    }
}
