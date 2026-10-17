// Cartoon cutscene shader (sf_cutscene, WP12.2): one pipeline for everything a
// scene draws, in window pixels.
//
//   kind 0  a shape: a rounded box (a circle is a box with radius = half size),
//           anti-aliased from its signed distance in the shape's own units,
//           so rotated and scaled cutout parts keep smooth edges
//   kind 1  a textured quad (a PNG part or layer, or a glyph of the subtitle
//           font), tinted by the vertex colour
//   kind 2  a flat colour (the fade, bars)
//
// Vertex colours are authored in sRGB; on an sRGB target they are converted to
// linear light first (the hardware converts back on write), like ui.wgsl.

struct Screen {
    // Target size in pixels.
    size: vec2<f32>,
    // 1.0 when the target is an sRGB format.
    srgb_target: f32,
    _pad: f32,
};

@group(0) @binding(0) var<uniform> screen: Screen;
@group(1) @binding(0) var tex: texture_2d<f32>;
@group(1) @binding(1) var samp: sampler;

struct VertexIn {
    @location(0) pos: vec2<f32>,
    // In texels of the bound texture (kind 1).
    @location(1) uv: vec2<f32>,
    // Position inside the shape, centre at 0, in the shape's own units (kind 0).
    @location(2) local: vec2<f32>,
    // Half width, half height, corner radius (own units), pixels per own unit.
    @location(3) params: vec4<f32>,
    @location(4) kind: f32,
    @location(5) color: vec4<f32>,
};

struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) local: vec2<f32>,
    @location(2) @interpolate(flat) params: vec4<f32>,
    @location(3) @interpolate(flat) kind: f32,
    @location(4) color: vec4<f32>,
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
    out.local = in.local;
    out.params = in.params;
    out.kind = in.kind;
    var rgb = in.color.rgb;
    if (screen.srgb_target > 0.5) {
        rgb = srgb_to_linear(rgb);
    }
    out.color = vec4<f32>(rgb, in.color.a);
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    // Sampled for every fragment so that control flow stays uniform; shapes
    // and flat colours bind a 1x1 white texture.
    let texel = textureSampleLevel(tex, samp, in.uv, 0.0);
    if (in.kind < 0.5) {
        let half_size = in.params.xy;
        let radius = in.params.z;
        let q = abs(in.local) - (half_size - vec2<f32>(radius));
        let dist = length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - radius;
        let cover = clamp(0.5 - dist * in.params.w, 0.0, 1.0);
        return vec4<f32>(in.color.rgb, in.color.a * cover);
    }
    if (in.kind < 1.5) {
        return in.color * texel;
    }
    return in.color;
}
