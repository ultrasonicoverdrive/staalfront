// UI shader (sf_ui, WP1.12): textured, tinted quads in window pixels.
// One pipeline for everything: solid rectangles sample a white texel, glyphs
// and skin sample the atlas, the minimap samples its own texture.

struct Screen {
    // Target size in pixels.
    size: vec2<f32>,
    // 1.0 when the target is an sRGB format: vertex colors are authored in
    // sRGB and must be converted to linear light before the hardware
    // converts back on write.
    srgb_target: f32,
    _pad: f32,
};

@group(0) @binding(0) var<uniform> screen: Screen;
@group(1) @binding(0) var tex: texture_2d<f32>;
@group(1) @binding(1) var samp: sampler;

struct VertexIn {
    @location(0) pos: vec2<f32>,
    // In texels of the bound texture.
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
};

struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
};

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((c + vec3<f32>(0.055)) / 1.055, vec3<f32>(2.4));
    return select(hi, lo, c <= vec3<f32>(0.04045));
}

@vertex
fn vs_main(in: VertexIn) -> VertexOut {
    var out: VertexOut;
    let ndc = vec2<f32>(
        in.pos.x / screen.size.x * 2.0 - 1.0,
        1.0 - in.pos.y / screen.size.y * 2.0,
    );
    out.clip = vec4<f32>(ndc, 0.0, 1.0);
    out.uv = in.uv / vec2<f32>(textureDimensions(tex));
    var rgb = in.color.rgb;
    if (screen.srgb_target > 0.5) {
        rgb = srgb_to_linear(rgb);
    }
    out.color = vec4<f32>(rgb, in.color.a);
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    return in.color * textureSample(tex, samp, in.uv);
}
