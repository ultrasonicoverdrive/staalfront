// Forward shader for lit meshes: one directional light + ambient, Lambert.
// Instance transforms: rotation + translation + uniform scale (non-uniform
// scale is only correct along axes aligned with the mesh normals).
//
// Cross-fade to the strategic icons (WP1.9): the alpha of the instance color is
// the model's weight `w` in 0..1. The model keeps the pixels whose dither
// threshold is below `w` (an 8x8 Bayer pattern) and drops the rest; icon.wgsl
// draws exactly the pixels this drops. Opaque, depth-correct, no sorting.
// Weight 1 (everything that is not a fading unit) keeps every pixel.

struct Globals {
    view_proj: mat4x4<f32>,
    // Unit vector pointing from the surface towards the light.
    light_dir: vec4<f32>,
    light_color: vec4<f32>,
    ambient: vec4<f32>,
};

@group(0) @binding(0) var<uniform> globals: Globals;

struct VertexIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    // Per-instance model matrix (columns) and base color (linear RGB).
    @location(2) model0: vec4<f32>,
    @location(3) model1: vec4<f32>,
    @location(4) model2: vec4<f32>,
    @location(5) model3: vec4<f32>,
    @location(6) color: vec4<f32>,
};

struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) color: vec3<f32>,
    // The cross-fade weight, the same for every vertex of an instance (it
    // comes from the per-instance color). Flat like in icon.wgsl, so both
    // shaders compare the exact value against the threshold; an interpolated
    // value could round across a threshold and draw a pixel twice or never.
    @location(2) @interpolate(flat) weight: f32,
};

// Threshold of the pixel in the 8x8 Bayer ordering, in (0, 1). The same
// integer formula as sf_render::icons::dither_rank (keep icon.wgsl in step).
fn dither_threshold(pixel: vec2<f32>) -> f32 {
    let x = u32(pixel.x) & 7u;
    let y = u32(pixel.y) & 7u;
    let xy = x ^ y;
    let rank = ((xy & 1u) << 5u) | ((y & 1u) << 4u) | ((xy & 2u) << 2u)
        | ((y & 2u) << 1u) | ((xy & 4u) >> 1u) | ((y & 4u) >> 2u);
    return (f32(rank) + 0.5) / 64.0;
}

@vertex
fn vs_main(in: VertexIn) -> VertexOut {
    let model = mat4x4<f32>(in.model0, in.model1, in.model2, in.model3);
    var out: VertexOut;
    out.clip = globals.view_proj * model * vec4<f32>(in.position, 1.0);
    out.normal = (model * vec4<f32>(in.normal, 0.0)).xyz;
    out.color = in.color.rgb;
    out.weight = in.color.a;
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    // Weight 1 keeps every pixel (the thresholds are all below 1): skip the test.
    if (in.weight < 1.0 && dither_threshold(in.clip.xy) >= in.weight) {
        discard;
    }
    let n = normalize(in.normal);
    let diffuse = max(dot(n, globals.light_dir.xyz), 0.0);
    let lit = globals.ambient.rgb + globals.light_color.rgb * diffuse;
    return vec4<f32>(in.color * lit, 1.0);
}
