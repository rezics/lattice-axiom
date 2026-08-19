//! Shared conformance suite for renderer implementations.
//!
//! Every backend (wgpu, headless, and any future implementation) must pass
//! this suite so that content behaves identically regardless of the
//! realization. Backends invoke it from their own test targets:
//!
//! ```ignore
//! #[test]
//! fn conforms_to_render_contract() {
//!     latticeaxiom_render::conformance::run_all(|| HeadlessRenderer::new(64, 64));
//! }
//! ```

use glam::{Mat4, Vec3};

use crate::camera::Camera;
use crate::renderer::{RenderError, Renderer};
use crate::world::{MaterialData, MeshData, MeshInstance, RenderWorld};

/// Runs every conformance check, constructing a fresh renderer per check.
///
/// # Panics
///
/// Panics when the implementation violates the renderer contract.
pub fn run_all<R, F>(mut make_renderer: F)
where
    R: Renderer,
    F: FnMut() -> R,
{
    uploads_assign_distinct_handles(&mut make_renderer());
    rejects_invalid_mesh(&mut make_renderer());
    rejects_unknown_handles(&mut make_renderer());
    rejects_non_finite_transform(&mut make_renderer());
    reports_drawn_instances(&mut make_renderer());
    resize_is_safe(&mut make_renderer());
}

/// A minimal valid mesh (one triangle) used by the checks.
#[must_use]
pub fn test_triangle() -> MeshData {
    MeshData {
        positions: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
        normals: vec![[0.0, 0.0, 1.0]; 3],
        indices: vec![0, 1, 2],
    }
}

fn test_camera() -> Camera {
    Camera::look_at(Vec3::new(3.0, -4.0, 2.0), Vec3::ZERO)
}

fn test_material() -> MaterialData {
    MaterialData {
        base_color: [0.8, 0.3, 0.2, 1.0],
    }
}

/// Uploading resources yields distinct, stable handles.
///
/// # Panics
///
/// Panics when uploads fail or handles collide.
pub fn uploads_assign_distinct_handles<R: Renderer>(renderer: &mut R) {
    let mesh_a = renderer
        .upload_mesh(&test_triangle())
        .expect("uploading a valid mesh must succeed");
    let mesh_b = renderer
        .upload_mesh(&test_triangle())
        .expect("uploading a second valid mesh must succeed");
    assert_ne!(mesh_a, mesh_b, "mesh handles must be distinct");

    let material_a = renderer
        .upload_material(&test_material())
        .expect("uploading a valid material must succeed");
    let material_b = renderer
        .upload_material(&test_material())
        .expect("uploading a second valid material must succeed");
    assert_ne!(material_a, material_b, "material handles must be distinct");
}

/// Structurally invalid meshes are rejected with [`RenderError::InvalidMesh`].
///
/// # Panics
///
/// Panics when an invalid mesh is accepted or misclassified.
pub fn rejects_invalid_mesh<R: Renderer>(renderer: &mut R) {
    let mut missing_normals = test_triangle();
    missing_normals.normals.pop();
    let mut out_of_range = test_triangle();
    out_of_range.indices = vec![0, 1, 3];
    let mut non_finite = test_triangle();
    non_finite.positions[0][2] = f32::NAN;

    for (label, mesh) in [
        ("empty", MeshData::default()),
        ("mismatched normals", missing_normals),
        ("out-of-range index", out_of_range),
        ("non-finite position", non_finite),
    ] {
        let result = renderer.upload_mesh(&mesh);
        assert!(
            matches!(result, Err(RenderError::InvalidMesh { .. })),
            "{label} mesh must be rejected as InvalidMesh, got {result:?}"
        );
    }
}

/// Submitting instances with foreign handles fails with the matching error.
///
/// # Panics
///
/// Panics when unknown handles are not rejected.
pub fn rejects_unknown_handles<R: Renderer>(renderer: &mut R) {
    let mesh = renderer
        .upload_mesh(&test_triangle())
        .expect("uploading a valid mesh must succeed");
    let material = renderer
        .upload_material(&test_material())
        .expect("uploading a valid material must succeed");

    let unknown_mesh = RenderWorld {
        camera: test_camera(),
        instances: vec![MeshInstance {
            mesh: crate::world::MeshId::from_raw(mesh.to_raw() + 1000),
            material,
            transform: Mat4::IDENTITY,
        }],
    };
    assert!(
        matches!(
            renderer.submit(&unknown_mesh),
            Err(RenderError::UnknownMesh(_))
        ),
        "unknown mesh handles must be rejected"
    );

    let unknown_material = RenderWorld {
        camera: test_camera(),
        instances: vec![MeshInstance {
            mesh,
            material: crate::world::MaterialId::from_raw(material.to_raw() + 1000),
            transform: Mat4::IDENTITY,
        }],
    };
    assert!(
        matches!(
            renderer.submit(&unknown_material),
            Err(RenderError::UnknownMaterial(_))
        ),
        "unknown material handles must be rejected"
    );
}

/// Non-finite instance transforms are rejected.
///
/// # Panics
///
/// Panics when a non-finite transform is accepted.
pub fn rejects_non_finite_transform<R: Renderer>(renderer: &mut R) {
    let mesh = renderer
        .upload_mesh(&test_triangle())
        .expect("uploading a valid mesh must succeed");
    let material = renderer
        .upload_material(&test_material())
        .expect("uploading a valid material must succeed");

    let world = RenderWorld {
        camera: test_camera(),
        instances: vec![MeshInstance {
            mesh,
            material,
            transform: Mat4::from_translation(Vec3::new(f32::INFINITY, 0.0, 0.0)),
        }],
    };
    assert!(
        matches!(
            renderer.submit(&world),
            Err(RenderError::NonFiniteTransform { index: 0 })
        ),
        "non-finite transforms must be rejected"
    );
}

/// A valid submission reports every instance as drawn.
///
/// # Panics
///
/// Panics when submission fails or the report is wrong.
pub fn reports_drawn_instances<R: Renderer>(renderer: &mut R) {
    let mesh = renderer
        .upload_mesh(&test_triangle())
        .expect("uploading a valid mesh must succeed");
    let material = renderer
        .upload_material(&test_material())
        .expect("uploading a valid material must succeed");

    let instances: Vec<MeshInstance> = (0..3u16)
        .map(|i| MeshInstance {
            mesh,
            material,
            transform: Mat4::from_translation(Vec3::new(f32::from(i), 0.0, 0.0)),
        })
        .collect();
    let world = RenderWorld {
        camera: test_camera(),
        instances,
    };

    let report = renderer
        .submit(&world)
        .expect("submitting a valid world must succeed");
    assert_eq!(
        report.instances_drawn, 3,
        "all valid instances must be reported as drawn"
    );

    let report = renderer
        .submit(&world)
        .expect("a second identical submission must also succeed");
    assert_eq!(report.instances_drawn, 3);
}

/// Resizing (including to degenerate sizes) must not panic or poison state.
///
/// # Panics
///
/// Panics when a resize breaks subsequent submissions.
pub fn resize_is_safe<R: Renderer>(renderer: &mut R) {
    let mesh = renderer
        .upload_mesh(&test_triangle())
        .expect("uploading a valid mesh must succeed");
    let material = renderer
        .upload_material(&test_material())
        .expect("uploading a valid material must succeed");

    renderer.resize(1, 1);
    renderer.resize(1920, 1080);

    let world = RenderWorld {
        camera: test_camera(),
        instances: vec![MeshInstance {
            mesh,
            material,
            transform: Mat4::IDENTITY,
        }],
    };
    renderer
        .submit(&world)
        .expect("submission after resizes must succeed");
}
