// Cell grid overlay (WP2.4): drawn over the terrain, with the terrain's own
// vertex buffer and level of detail draws, when the cell grid is on.
// One texel per 4 m gameplay cell (Rgba8Uint): rgb = sRGB colour, a = state
// (sf_render::cell_overlay::CellState: 0 open, 1 impassable, 2 units,
// 3 structure). The colours and states are computed on the CPU and tested
// there; this shader only adds the pattern per state.

struct Globals {
    view_proj: mat4x4<f32>,
    light_dir: vec4<f32>,
    light_color: vec4<f32>,
    ambient: vec4<f32>,
};

struct OverlayParams {
    // Map size in gameplay cells.
    map_cells: vec2<i32>,
    // Metres in one gameplay cell (B12).
    cell_meters: f32,
    // Opacity of the type tint.
    fill: f32,
};

@group(0) @binding(0) var<uniform> globals: Globals;
@group(1) @binding(0) var cells: texture_2d<u32>;
@group(1) @binding(1) var<uniform> params: OverlayParams;

struct VertexIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) lod: u32,
};

struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world_xz: vec2<f32>,
};

// Lifted a few centimetres off the terrain so that the depth test passes
// over the surface it is drawn on, whatever the rounding of either shader.
const LIFT_METERS: f32 = 0.05;

@vertex
fn vs_main(in: VertexIn) -> VertexOut {
    var out: VertexOut;
    out.clip = globals.view_proj * vec4<f32>(in.position + vec3<f32>(0.0, LIFT_METERS, 0.0), 1.0);
    out.world_xz = in.position.xz;
    return out;
}

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((c + vec3<f32>(0.055)) / 1.055, vec3<f32>(2.4));
    return select(hi, lo, c <= vec3<f32>(0.04045));
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let q = in.world_xz / params.cell_meters;
    let cell = clamp(vec2<i32>(floor(q)), vec2<i32>(0, 0), params.map_cells - vec2<i32>(1, 1));
    let t = textureLoad(cells, cell, 0);
    let colour = srgb_to_linear(vec3<f32>(t.rgb) / 255.0);
    // Position inside the cell, 0..1, and the distance to its nearest edge.
    let f = fract(q);
    let edge = min(min(f.x, 1.0 - f.x), min(f.y, 1.0 - f.y));
    // Pixels per cell: the patterns fade out below ~8 px a cell, so the far
    // zoom shows flat colours instead of moire. Computed before the switch
    // (uniform control flow for the derivative).
    let w = fwidth(q);
    let pattern = clamp((1.0 / max(max(w.x, w.y), 1e-6) - 8.0) / 8.0, 0.0, 1.0);
    switch t.a {
        // Impassable: the type colour with dark diagonal hatching.
        case 1u: {
            let stripe = select(0.0, pattern, fract((f.x + f.y) * 3.0) < 0.35);
            // Far away: the type colour darkened a little instead.
            let base = colour * mix(0.6, 1.0, pattern);
            return mix(vec4<f32>(base, params.fill), vec4<f32>(0.02, 0.02, 0.02, 0.6), stripe);
        }
        // Units: a disc in the cell.
        case 2u: {
            let d = length(f - vec2<f32>(0.5, 0.5));
            if (d < 0.3) {
                return vec4<f32>(colour, 0.85);
            }
            return vec4<f32>(colour, 0.5 * params.fill);
        }
        // Structure: filled and outlined.
        case 3u: {
            if (edge < 0.08) {
                return vec4<f32>(colour, 0.95);
            }
            return vec4<f32>(colour, 0.65);
        }
        default: {
            return vec4<f32>(colour, params.fill);
        }
    }
}
