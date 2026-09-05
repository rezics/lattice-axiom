// Minimal engine fallback. Authored water optics live in the independent shader pack.
@fragment
fn fragment() -> @location(0) vec4<f32> {
    return vec4<f32>(0.05, 0.22, 0.31, 0.62);
}
