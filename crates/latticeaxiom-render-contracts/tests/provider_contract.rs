//! Exactly-one provider and realization fallback contract tests.

use std::collections::{BTreeMap, BTreeSet};

use latticeaxiom_core::{CapabilityId, StableId};
use latticeaxiom_render_contracts::{
    GpuCapabilities, GpuFeatureV1, GpuLimitV1, ProviderFallback, ProviderRealization,
    ProviderRequirement, ProviderSelectError, ProviderSelectionRequest, RenderProviderDecl,
    TextureFormatV1, select_provider,
};

fn id(value: &str) -> StableId {
    match value.parse() {
        Ok(value) => value,
        Err(error) => panic!("test StableId must be valid: {error}"),
    }
}

fn capability(value: &str) -> CapabilityId {
    match value.parse() {
        Ok(value) => value,
        Err(error) => panic!("test CapabilityId must be valid: {error}"),
    }
}

fn realization(
    value: &str,
    priority: i32,
    requirements: ProviderRequirement,
    fallback: ProviderFallback,
) -> ProviderRealization {
    ProviderRealization {
        id: id(value),
        stable_priority: priority,
        requirements,
        fallback,
    }
}

fn terrain_provider(optional: bool) -> RenderProviderDecl {
    let degraded = id("demo:render-realization/terrain-degraded@1");
    RenderProviderDecl {
        id: id("demo:render-provider/terrain@1"),
        capability: capability("demo:capability/render-terrain@1"),
        presentation_optional: optional,
        realizations: vec![
            realization(
                "demo:render-realization/terrain-degraded@1",
                10,
                ProviderRequirement::default(),
                if optional {
                    ProviderFallback::Disabled
                } else {
                    ProviderFallback::Fail
                },
            ),
            realization(
                "demo:render-realization/terrain-preferred@1",
                100,
                ProviderRequirement {
                    features: BTreeSet::from([GpuFeatureV1::ShaderF16]),
                    minimum_limits: BTreeMap::from([(GpuLimitV1::MaxTextureDimension2d, 8192)]),
                    formats: BTreeSet::from([TextureFormatV1::Rgba16Float]),
                },
                ProviderFallback::Realization {
                    realization: degraded,
                },
            ),
        ],
    }
}

fn request(provider: RenderProviderDecl, gpu: GpuCapabilities) -> ProviderSelectionRequest {
    ProviderSelectionRequest {
        capability: capability("demo:capability/render-terrain@1"),
        providers: vec![provider],
        explicit_provider: None,
        gpu,
    }
}

#[test]
fn preferred_then_degraded_selection_uses_only_standard_capabilities() {
    let supported = GpuCapabilities {
        features: BTreeSet::from([GpuFeatureV1::ShaderF16]),
        limits: BTreeMap::from([(GpuLimitV1::MaxTextureDimension2d, 8192)]),
        formats: BTreeSet::from([TextureFormatV1::Rgba16Float]),
    };
    let selected = match select_provider(&request(terrain_provider(false), supported)) {
        Ok(value) => value,
        Err(error) => panic!("preferred realization must select: {error}"),
    };
    assert_eq!(
        selected.realization,
        Some(id("demo:render-realization/terrain-preferred@1"))
    );
    assert!(selected.rejected.is_empty());

    let degraded = match select_provider(&request(
        terrain_provider(false),
        GpuCapabilities::default(),
    )) {
        Ok(value) => value,
        Err(error) => panic!("degraded realization must select: {error}"),
    };
    assert_eq!(
        degraded.realization,
        Some(id("demo:render-realization/terrain-degraded@1"))
    );
    assert_eq!(degraded.rejected.len(), 1);
}

#[test]
fn logical_provider_conflict_requires_explicit_profile_selection() {
    let mut first = terrain_provider(false);
    first.id = id("demo:render-provider/a@1");
    let mut second = terrain_provider(false);
    second.id = id("demo:render-provider/b@1");
    let mut request = ProviderSelectionRequest {
        capability: capability("demo:capability/render-terrain@1"),
        providers: vec![second.clone(), first.clone()],
        explicit_provider: None,
        gpu: GpuCapabilities::default(),
    };
    let error = match select_provider(&request) {
        Ok(value) => panic!("conflicting providers must fail, selected {value:?}"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        ProviderSelectError::ProviderConflict { ref candidates, .. }
            if candidates == &vec![first.id.clone(), second.id.clone()]
    ));

    request.explicit_provider = Some(second.id.clone());
    let selected = match select_provider(&request) {
        Ok(value) => value,
        Err(error) => panic!("explicit provider must resolve conflict: {error}"),
    };
    assert_eq!(selected.provider, second.id);
}

#[test]
fn provider_discovery_order_does_not_change_conflict_diagnostic() {
    let mut a = terrain_provider(false);
    a.id = id("demo:render-provider/a@1");
    let mut b = terrain_provider(false);
    b.id = id("demo:render-provider/b@1");
    let first = ProviderSelectionRequest {
        capability: capability("demo:capability/render-terrain@1"),
        providers: vec![a.clone(), b.clone()],
        explicit_provider: None,
        gpu: GpuCapabilities::default(),
    };
    let second = ProviderSelectionRequest {
        providers: vec![b, a],
        ..first.clone()
    };
    assert_eq!(select_provider(&first), select_provider(&second));

    let provider = terrain_provider(false);
    let original = request(provider.clone(), GpuCapabilities::default());
    let mut reversed_provider = provider;
    reversed_provider.realizations.reverse();
    let reversed = request(reversed_provider, GpuCapabilities::default());
    assert_eq!(select_provider(&original), select_provider(&reversed));
}

#[test]
fn fallback_cycles_and_illegal_disabled_terminals_fail_before_activation() {
    let first = id("demo:render-realization/a@1");
    let second = id("demo:render-realization/b@1");
    let cyclic = RenderProviderDecl {
        id: id("demo:render-provider/cyclic@1"),
        capability: capability("demo:capability/render-terrain@1"),
        presentation_optional: false,
        realizations: vec![
            realization(
                first.as_str(),
                2,
                ProviderRequirement::default(),
                ProviderFallback::Realization {
                    realization: second.clone(),
                },
            ),
            realization(
                second.as_str(),
                1,
                ProviderRequirement::default(),
                ProviderFallback::Realization { realization: first },
            ),
        ],
    };
    assert!(matches!(
        select_provider(&request(cyclic, GpuCapabilities::default())),
        Err(ProviderSelectError::FallbackCycle { .. })
    ));

    let mut required_can_disable = terrain_provider(false);
    required_can_disable.realizations[0].fallback = ProviderFallback::Disabled;
    assert!(matches!(
        select_provider(&request(required_can_disable, GpuCapabilities::default())),
        Err(ProviderSelectError::RequiredProviderCanDisable { .. })
    ));
}

#[test]
fn optional_provider_can_end_in_disabled_without_switching_logical_provider() {
    let mut provider = terrain_provider(true);
    provider.realizations[0].requirements.features = BTreeSet::from([GpuFeatureV1::ComputeShaders]);
    let selected = match select_provider(&request(provider.clone(), GpuCapabilities::default())) {
        Ok(value) => value,
        Err(error) => panic!("optional provider must disable: {error}"),
    };
    assert!(selected.disabled);
    assert_eq!(selected.provider, provider.id);
    assert_eq!(selected.realization, None);
}

#[test]
fn provider_schema_rejects_unknown_fields() {
    let json = r#"{
        "id":"demo:render-provider/terrain@1",
        "capability":"demo:capability/render-terrain@1",
        "presentation_optional":false,
        "realizations":[],
        "vendor_priority":7
    }"#;
    assert!(serde_json::from_str::<RenderProviderDecl>(json).is_err());
}
