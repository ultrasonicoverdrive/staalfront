// Particles and projectile tracers (WP5.6a): one instanced screen-facing quad each, a soft disc made here.
//
// An instance is a world position, the side of its quad in physical pixels (already clamped between the
// limits of effects.ron on the CPU: sf_render::effects::size_px) and a premultiplied linear colour. The
// vertex shader projects the centre and lays a square round it in pixels, as the icons do, so a particle is
// the same size on the screen whatever its distance once it is clamped. The fragment shader draws a disc
// that is full in the middle and falls to nothing at the edge.
//
// Blending is premultiplied (One, OneMinusSrcAlpha): a glowing particle arrives with blend alpha 0, which
// makes it additive, smoke arrives with its alpha. So flashes, fire, smoke and tracers are one pipeline and
// one draw call. Depth is tested against the scene (a particle behind a hill or a model is hidden) and not
// written. The depth of the whole quad is that of its centre.

struct Globals {
    view_proj: mat4x4<f32>,
    light_dir: vec4<f32>,
    light_color: vec4<f32>,
    ambient: vec4<f32>,
};

// Keep in step with `Renderer::prepare_frame` (renderer.rs): the viewport in pixels.
struct EffectParams {
    viewport: vec2<f32>,
    pad: vec2<f32>,
};

@group(0) @binding(0) var<uniform> globals: Globals;
@group(1) @binding(0) var<uniform> fx: EffectParams;

struct VertexIn {
    @builtin(vertex_index) vertex: u32,
    // Per instance: world position and the side in pixels; premultiplied colour and blend alpha.
    @location(0) position_size: vec4<f32>,
    @location(1) color: vec4<f32>,
};

struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    // Offset from the centre in pixels, and the radius of the disc.
    @location(0) local: vec2<f32>,
    @location(1) @interpolate(flat) radius: f32,
    @location(2) @interpolate(flat) color: vec4<f32>,
};

@vertex
fn vs_main(in: VertexIn) -> VertexOut {
    var corners = array<vec2<f32>, 4>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(1.0, -1.0),
        vec2<f32>(1.0, 1.0),
        vec2<f32>(-1.0, 1.0),
    );
    let corner = corners[in.vertex];
    let clip = globals.view_proj * vec4<f32>(in.position_size.xyz, 1.0);
    var out: VertexOut;
    out.color = in.color;
    out.radius = in.position_size.w * 0.5;
    if (clip.w <= 0.0001) {
        // Behind the camera: outside the clip volume.
        out.clip = vec4<f32>(2.0, 2.0, 2.0, 1.0);
        out.local = vec2<f32>(0.0, 0.0);
        return out;
    }
    let ndc = clip.xy / clip.w;
    // One pixel of margin for the soft edge.
    let half = out.radius + 1.0;
    let local = corner * half;
    out.clip = vec4<f32>(
        ndc.x + local.x / fx.viewport.x * 2.0,
        ndc.y + local.y / fx.viewport.y * 2.0,
        clip.z / clip.w,
        1.0,
    );
    out.local = local;
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    // 1 in the middle, 0 at the radius, smooth in between; a pixel of antialiasing at the edge.
    let d = length(in.local);
    let t = clamp(1.0 - d / max(in.radius, 0.5), 0.0, 1.0);
    let shape = t * t * (3.0 - 2.0 * t);
    return in.color * shape;
}
