//! Logical SSA render-plan contract tests.

use std::collections::BTreeSet;

use latticeaxiom_core::StableId;
use latticeaxiom_render_contracts::{
    CompiledRenderPlan, Core3dSlotV1, ExtentPolicy, GraphCompileError, PassResourceUse,
    RenderFeatureDecl, RenderGraphCompiler, RenderPassDecl, RenderPlanInput, RenderResourceDecl,
    RenderResourceFormat, RenderResourceKind, RenderResourceLifetime, RenderResourceProducer,
    RenderResourceScope, RenderResourceUsage, RenderResourceVersion, TextureFormatV1,
};

fn id(value: &str) -> StableId {
    match value.parse() {
        Ok(value) => value,
        Err(error) => panic!("test StableId must be valid: {error}"),
    }
}

fn version(value: &str, local_version: u32) -> RenderResourceVersion {
    RenderResourceVersion {
        resource: id(value),
        version: local_version,
    }
}

fn usages(values: &[RenderResourceUsage]) -> BTreeSet<RenderResourceUsage> {
    values.iter().copied().collect()
}

fn deterministic_shuffle<T>(values: &mut [T], state: &mut u64) {
    for last in (1..values.len()).rev() {
        *state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        let bound = match u64::try_from(last + 1) {
            Ok(value) => value,
            Err(error) => panic!("test permutation bound must fit u64: {error}"),
        };
        let selected = match usize::try_from(*state % bound) {
            Ok(value) => value,
            Err(error) => panic!("test permutation index must fit usize: {error}"),
        };
        values.swap(last, selected);
    }
}

fn feature_scope(feature: &StableId) -> RenderResourceScope {
    RenderResourceScope::Feature {
        feature: feature.clone(),
    }
}

fn write_use(resource: &RenderResourceVersion, scope: RenderResourceScope) -> PassResourceUse {
    PassResourceUse {
        resource: resource.clone(),
        usage: RenderResourceUsage::StorageWrite,
        kind: RenderResourceKind::Texture,
        format: RenderResourceFormat::Texture {
            format: TextureFormatV1::R8Unorm,
        },
        sample_count: 1,
        scope,
    }
}

fn output_resource(
    resource: RenderResourceVersion,
    feature: &StableId,
    pass: &StableId,
) -> RenderResourceDecl {
    RenderResourceDecl {
        id: resource,
        kind: RenderResourceKind::Texture,
        scope: feature_scope(feature),
        lifetime: RenderResourceLifetime::Transient,
        format: RenderResourceFormat::Texture {
            format: TextureFormatV1::R8Unorm,
        },
        sample_count: 1,
        extent: ExtentPolicy::View,
        usages: usages(&[
            RenderResourceUsage::Sampled,
            RenderResourceUsage::StorageWrite,
        ]),
        producer: RenderResourceProducer::FeaturePass { pass: pass.clone() },
    }
}

fn compile(input: &RenderPlanInput) -> CompiledRenderPlan {
    match RenderGraphCompiler::compile(input) {
        Ok(plan) => plan,
        Err(error) => panic!("test render plan must compile: {error}"),
    }
}

fn one_pass_input() -> RenderPlanInput {
    let feature = id("demo:render-feature/fog@1");
    let pass = id("demo:render-pass/fog@1");
    let output = version("demo:render-resource/fog-output@1", 1);
    RenderPlanInput {
        schema_major: 1,
        data: Vec::new(),
        features: vec![RenderFeatureDecl {
            id: feature.clone(),
            presentation_optional: false,
        }],
        passes: vec![RenderPassDecl {
            id: pass.clone(),
            feature: feature.clone(),
            slot: Core3dSlotV1::BeforeTonemap,
            reads: Vec::new(),
            writes: vec![write_use(&output, feature_scope(&feature))],
            after: BTreeSet::new(),
            before: BTreeSet::new(),
        }],
        resources: vec![output_resource(output, &feature, &pass)],
    }
}

#[test]
fn compiled_plan_matches_contract_major_one_golden() {
    let json = match serde_json::to_string(&compile(&one_pass_input())) {
        Ok(json) => json,
        Err(error) => panic!("compiled plan must serialize: {error}"),
    };
    let expected = r#"{"schema_major":1,"data":[],"features":[{"id":"demo:render-feature/fog@1","presentation_optional":false}],"passes":[{"ordinal":0,"pass":{"id":"demo:render-pass/fog@1","feature":"demo:render-feature/fog@1","slot":"latticeaxiom:render-slot/core3d/before-tonemap@1","reads":[],"writes":[{"resource":{"resource":"demo:render-resource/fog-output@1","version":1},"usage":"storage-write","kind":"texture","format":{"kind":"texture","format":"r8unorm"},"sample_count":1,"scope":{"kind":"feature","feature":"demo:render-feature/fog@1"}}],"after":[],"before":[]}}],"resources":[{"id":{"resource":"demo:render-resource/fog-output@1","version":1},"kind":"texture","scope":{"kind":"feature","feature":"demo:render-feature/fog@1"},"lifetime":"transient","format":{"kind":"texture","format":"r8unorm"},"sample_count":1,"extent":{"kind":"view"},"usages":["sampled","storage-write"],"producer":{"kind":"feature-pass","pass":"demo:render-pass/fog@1"}}]}"#;
    assert_eq!(json, expected);
}

#[test]
fn discovery_and_access_permutations_produce_identical_plan_bytes() {
    let feature_a = id("demo:render-feature/a@1");
    let feature_b = id("demo:render-feature/b@1");
    let pass_a = id("demo:render-pass/a@1");
    let pass_b = id("demo:render-pass/b@1");
    let output_a1 = version("demo:render-resource/a@1", 1);
    let output_a2 = version("demo:render-resource/a@1", 2);
    let output_b = version("demo:render-resource/b@1", 1);
    let input = RenderPlanInput {
        schema_major: 1,
        data: Vec::new(),
        features: vec![
            RenderFeatureDecl {
                id: feature_b.clone(),
                presentation_optional: true,
            },
            RenderFeatureDecl {
                id: feature_a.clone(),
                presentation_optional: false,
            },
        ],
        passes: vec![
            RenderPassDecl {
                id: pass_b.clone(),
                feature: feature_b.clone(),
                slot: Core3dSlotV1::BeforeTonemap,
                reads: Vec::new(),
                writes: vec![write_use(&output_b, feature_scope(&feature_b))],
                after: BTreeSet::new(),
                before: BTreeSet::new(),
            },
            RenderPassDecl {
                id: pass_a.clone(),
                feature: feature_a.clone(),
                slot: Core3dSlotV1::BeforeTonemap,
                reads: Vec::new(),
                writes: vec![
                    write_use(&output_a2, feature_scope(&feature_a)),
                    write_use(&output_a1, feature_scope(&feature_a)),
                ],
                after: BTreeSet::new(),
                before: BTreeSet::new(),
            },
        ],
        resources: vec![
            output_resource(output_b, &feature_b, &pass_b),
            output_resource(output_a2, &feature_a, &pass_a),
            output_resource(output_a1, &feature_a, &pass_a),
        ],
    };
    let first = compile(&input);
    for seed in 0..128_u64 {
        let mut permutation = input.clone();
        let mut state = seed;
        deterministic_shuffle(&mut permutation.features, &mut state);
        deterministic_shuffle(&mut permutation.passes, &mut state);
        deterministic_shuffle(&mut permutation.resources, &mut state);
        for pass in &mut permutation.passes {
            deterministic_shuffle(&mut pass.reads, &mut state);
            deterministic_shuffle(&mut pass.writes, &mut state);
        }
        assert_eq!(first, compile(&permutation), "discovery permutation {seed}");
    }
    assert_eq!(first.passes[0].pass.id, pass_a);
    assert_eq!(first.passes[1].pass.id, pass_b);
}

#[test]
fn resource_and_order_edges_detect_cycles() {
    let feature = id("demo:render-feature/cycle@1");
    let pass_a = id("demo:render-pass/cycle-a@1");
    let pass_b = id("demo:render-pass/cycle-b@1");
    let resource_a = version("demo:render-resource/cycle-a@1", 1);
    let resource_b = version("demo:render-resource/cycle-b@1", 1);
    let scope = feature_scope(&feature);
    let read = |resource: &RenderResourceVersion| PassResourceUse {
        resource: resource.clone(),
        usage: RenderResourceUsage::Sampled,
        kind: RenderResourceKind::Texture,
        format: RenderResourceFormat::Texture {
            format: TextureFormatV1::R8Unorm,
        },
        sample_count: 1,
        scope: scope.clone(),
    };
    let input = RenderPlanInput {
        schema_major: 1,
        data: Vec::new(),
        features: vec![RenderFeatureDecl {
            id: feature.clone(),
            presentation_optional: false,
        }],
        passes: vec![
            RenderPassDecl {
                id: pass_a.clone(),
                feature: feature.clone(),
                slot: Core3dSlotV1::EarlyPost,
                reads: vec![read(&resource_b)],
                writes: vec![write_use(&resource_a, scope.clone())],
                after: BTreeSet::new(),
                before: BTreeSet::new(),
            },
            RenderPassDecl {
                id: pass_b.clone(),
                feature: feature.clone(),
                slot: Core3dSlotV1::EarlyPost,
                reads: vec![read(&resource_a)],
                writes: vec![write_use(&resource_b, scope)],
                after: BTreeSet::new(),
                before: BTreeSet::new(),
            },
        ],
        resources: vec![
            output_resource(resource_a, &feature, &pass_a),
            output_resource(resource_b, &feature, &pass_b),
        ],
    };
    assert!(matches!(
        RenderGraphCompiler::compile(&input),
        Err(GraphCompileError::Cycle { .. })
    ));
}

#[test]
fn malformed_resource_fault_corpus_is_rejected_before_activation() {
    let valid = one_pass_input();

    let mut multiple_writer = valid.clone();
    multiple_writer.resources.push(valid.resources[0].clone());
    assert!(matches!(
        RenderGraphCompiler::compile(&multiple_writer),
        Err(GraphCompileError::MultipleWriters { .. })
    ));

    let mut format_mismatch = valid.clone();
    format_mismatch.passes[0].writes[0].format = RenderResourceFormat::Texture {
        format: TextureFormatV1::Rgba16Float,
    };
    assert!(matches!(
        RenderGraphCompiler::compile(&format_mismatch),
        Err(GraphCompileError::ResourceMismatch {
            field: "format",
            ..
        })
    ));

    let mut sample_mismatch = valid.clone();
    sample_mismatch.passes[0].writes[0].sample_count = 4;
    assert!(matches!(
        RenderGraphCompiler::compile(&sample_mismatch),
        Err(GraphCompileError::ResourceMismatch {
            field: "sample-count",
            ..
        })
    ));

    let mut scope_mismatch = valid.clone();
    scope_mismatch.passes[0].writes[0].scope = RenderResourceScope::View;
    assert!(matches!(
        RenderGraphCompiler::compile(&scope_mismatch),
        Err(GraphCompileError::ResourceMismatch { field: "scope", .. })
    ));

    let mut undeclared_usage = valid.clone();
    undeclared_usage.resources[0].usages = usages(&[RenderResourceUsage::Sampled]);
    assert!(matches!(
        RenderGraphCompiler::compile(&undeclared_usage),
        Err(GraphCompileError::UsageNotDeclared { .. })
    ));

    let mut missing_producer = valid;
    missing_producer.passes[0].reads.push(PassResourceUse {
        resource: version("demo:render-resource/missing@1", 0),
        usage: RenderResourceUsage::Sampled,
        kind: RenderResourceKind::Texture,
        format: RenderResourceFormat::Texture {
            format: TextureFormatV1::R8Unorm,
        },
        sample_count: 1,
        scope: RenderResourceScope::View,
    });
    assert!(matches!(
        RenderGraphCompiler::compile(&missing_producer),
        Err(GraphCompileError::MissingProducer { .. })
    ));
}

#[test]
fn host_resource_cannot_be_read_before_its_semantic_slot() {
    let mut input = one_pass_input();
    let imported = version("demo:render-resource/late-host@1", 0);
    input.resources.push(RenderResourceDecl {
        id: imported.clone(),
        kind: RenderResourceKind::Texture,
        scope: RenderResourceScope::View,
        lifetime: RenderResourceLifetime::Imported,
        format: RenderResourceFormat::Texture {
            format: TextureFormatV1::Rgba16Float,
        },
        sample_count: 1,
        extent: ExtentPolicy::View,
        usages: usages(&[RenderResourceUsage::Sampled]),
        producer: RenderResourceProducer::HostSlot {
            slot: Core3dSlotV1::AfterTonemap,
        },
    });
    input.passes[0].reads.push(PassResourceUse {
        resource: imported,
        usage: RenderResourceUsage::Sampled,
        kind: RenderResourceKind::Texture,
        format: RenderResourceFormat::Texture {
            format: TextureFormatV1::Rgba16Float,
        },
        sample_count: 1,
        scope: RenderResourceScope::View,
    });
    assert!(matches!(
        RenderGraphCompiler::compile(&input),
        Err(GraphCompileError::ReadBeforeProduce { .. })
    ));
}

#[test]
fn serde_rejects_unknown_slot_and_unknown_fields() {
    let unknown_slot = r#""demo:render-slot/private@1""#;
    assert!(serde_json::from_str::<Core3dSlotV1>(unknown_slot).is_err());

    let unknown_field = r#"{
        "id": "demo:render-feature/fog@1",
        "presentation_optional": false,
        "load_order": 7
    }"#;
    assert!(serde_json::from_str::<RenderFeatureDecl>(unknown_field).is_err());
}
