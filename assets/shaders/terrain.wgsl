// Terrain shader (WP1.7, WP1.8, WP2.2): one vertex grid per chunk and level
// of detail, material per cell.
// The cell index is a texture (Rg8Uint, one texel per gameplay cell of 4 m, or
// per 1 m sample on a map without terrain types): red = layer of the material
// texture array, green = style bits (1 = flat, 2 = liquid). The fragment shader
// blends the four nearest cell centres with a narrow ramp around the cell
// border (so a cell stays pure in its middle), the border wobbles a little so a
// staircase of cells does not look like a chessboard, and where all four cells
// agree it takes a single texture sample. Materials live in a texture array and
// are sampled with world-space UVs. Lighting as in scene.wgsl: one directional
// light + ambient, Lambert; a flat style (water, a bridge deck) is lit as
// level ground, and a liquid gets a light line along its shore.

struct Globals {
    view_proj: mat4x4<f32>,
    // Unit vector pointing from the surface towards the light.
    light_dir: vec4<f32>,
    light_color: vec4<f32>,
    ambient: vec4<f32>,
};

struct TerrainParams {
    // Size of the cell index texture in texels.
    index_size: vec2<i32>,
    // World size of one texture repeat in metres.
    tile_meters: f32,
    // Debug flags: bit 0 = tint chunks by their level of detail.
    flags: u32,
    // Debug cell grid opacity, 0 = off.
    grid_alpha: f32,
    // Metres in one gameplay cell: the spacing of the debug grid (B12).
    cell_meters: f32,
    // Metres per index texel.
    index_meters: f32,
    // Half the width of the blend ramp in texels (0.5 = centre to centre).
    ramp: f32,
    // How far borders wander, in texels; 0 = straight.
    wobble: f32,
    // Strength of the shore line, 0 = off.
    shore_amount: f32,
    shore_r: f32,
    shore_g: f32,
    shore_b: f32,
    pad: f32,
};

@group(0) @binding(0) var<uniform> globals: Globals;
@group(1) @binding(0) var materials: texture_2d_array<f32>;
@group(1) @binding(1) var material_sampler: sampler;
@group(1) @binding(2) var cell_index: texture_2d<u32>;
@group(1) @binding(3) var<uniform> params: TerrainParams;

struct VertexIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    // Level of detail of the mesh this vertex belongs to (0 = full detail).
    @location(2) lod: u32,
};

struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) world_xz: vec2<f32>,
    @location(2) @interpolate(flat) lod: u32,
};

// Debug tint per level (linear light): red, orange, yellow, green, blue, purple.
// Mirrored by sf_render::lod::LOD_TINT for the tests.
fn lod_tint(level: u32) -> vec3<f32> {
    switch level {
        case 0u: { return vec3<f32>(0.80, 0.05, 0.05); }
        case 1u: { return vec3<f32>(1.00, 0.40, 0.02); }
        case 2u: { return vec3<f32>(0.90, 0.85, 0.05); }
        case 3u: { return vec3<f32>(0.05, 0.60, 0.10); }
        case 4u: { return vec3<f32>(0.05, 0.25, 1.00); }
        default: { return vec3<f32>(0.55, 0.10, 0.90); }
    }
}

@vertex
fn vs_main(in: VertexIn) -> VertexOut {
    var out: VertexOut;
    out.clip = globals.view_proj * vec4<f32>(in.position, 1.0);
    out.normal = in.normal;
    out.world_xz = in.position.xz;
    out.lod = in.lod;
    return out;
}

// Layer (x) and style bits (y) of an index texel; the map edge repeats.
fn cell_at(cell: vec2<i32>) -> vec2<u32> {
    let c = clamp(cell, vec2<i32>(0, 0), params.index_size - vec2<i32>(1, 1));
    return textureLoad(cell_index, c, 0).rg;
}

fn hash_cell(p: vec2<i32>) -> f32 {
    var h = u32(p.x) * 0x85ebca6bu ^ u32(p.y) * 0xc2b2ae35u;
    h = h ^ (h >> 16u);
    h = h * 0x7feb352du;
    h = h ^ (h >> 15u);
    h = h * 0x846ca68bu;
    h = h ^ (h >> 16u);
    return f32(h >> 8u) / 16777216.0;
}

// Smooth value noise in 0..1, one lattice point per `1 / freq` metres.
fn value_noise(p: vec2<f32>) -> f32 {
    let i = vec2<i32>(floor(p));
    let f = fract(p);
    let s = f * f * (vec2<f32>(3.0, 3.0) - 2.0 * f);
    let a = hash_cell(i);
    let b = hash_cell(i + vec2<i32>(1, 0));
    let c = hash_cell(i + vec2<i32>(0, 1));
    let d = hash_cell(i + vec2<i32>(1, 1));
    return mix(mix(a, b, s.x), mix(c, d, s.x), s.y);
}

// Chunk size in metres (32 terrain cells of 1 m), for the debug grid's chunk
// borders.
const CHUNK_METERS: f32 = 32.0;

// Anti-aliased line coverage `0..1` for lines every `period` metres, about
// `px` pixels wide. Fades out once the lines are closer than ~2 px, so a
// grazing view or a far zoom shows no moire.
fn grid_lines(p: vec2<f32>, period: f32, px: f32) -> f32 {
    let q = p / period;
    let w = max(fwidth(q), vec2<f32>(1e-6, 1e-6));
    let d = abs(fract(q - vec2<f32>(0.5, 0.5)) - vec2<f32>(0.5, 0.5)) / w;
    let nearest = min(d.x, d.y);
    let coverage = 1.0 - clamp(nearest - 0.5 * (px - 1.0), 0.0, 1.0);
    let pixels_per_line = 1.0 / max(w.x, w.y);
    let fade = clamp((pixels_per_line - 2.0) / 4.0, 0.0, 1.0);
    return coverage * fade;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let uv = in.world_xz / params.tile_meters;
    // Derivatives before any branch: the texture samples below sit in
    // non-uniform control flow and so take explicit gradients.
    let ddx = dpdx(uv);
    let ddy = dpdy(uv);
    // Index texel centres sit at integer + 0.5; the four nearest are blended.
    let q = in.world_xz / params.index_meters - vec2<f32>(0.5, 0.5);
    let footprint = max(fwidth(q.x), fwidth(q.y));
    let base = vec2<i32>(floor(q));
    let c00 = cell_at(base);
    let c10 = cell_at(base + vec2<i32>(1, 0));
    let c01 = cell_at(base + vec2<i32>(0, 1));
    let c11 = cell_at(base + vec2<i32>(1, 1));

    var albedo: vec3<f32>;
    var flat_weight: f32;
    var liquid_weight: f32;
    if (all(c00 == c10) && all(c00 == c01) && all(c00 == c11)) {
        // Inside a uniform stretch: one sample, no weights.
        albedo = textureSampleGrad(materials, material_sampler, uv, i32(c00.x), ddx, ddy).rgb;
        flat_weight = f32(c00.y & 1u);
        liquid_weight = f32((c00.y >> 1u) & 1u);
    } else {
        var f = fract(q);
        // The border (f = 0.5) wanders by up to `wobble` texels; far away,
        // where a pixel covers a good part of a cell, it stays put.
        if (params.wobble > 0.0) {
            let near = 1.0 - smoothstep(0.1, 0.4, footprint);
            let n = vec2<f32>(
                value_noise(in.world_xz * 0.9),
                value_noise(in.world_xz * 0.9 + vec2<f32>(31.7, 17.3)));
            // The shift fades to nothing at the texel centres (f = 0 and 1), so
            // the picture stays continuous where `base` changes.
            let window = 4.0 * f * (vec2<f32>(1.0, 1.0) - f);
            f = clamp(
                f + (n - vec2<f32>(0.5, 0.5)) * 2.0 * params.wobble * near * window,
                vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 1.0));
        }
        // Ramp of `ramp` texels either side of the border, at least as wide as
        // a pixel so the border does not alias when zoomed out.
        let r = clamp(max(params.ramp, 0.5 * footprint), 0.001, 0.5);
        let w = smoothstep(vec2<f32>(0.5 - r, 0.5 - r), vec2<f32>(0.5 + r, 0.5 + r), f);
        let t00 = textureSampleGrad(materials, material_sampler, uv, i32(c00.x), ddx, ddy).rgb;
        let t10 = textureSampleGrad(materials, material_sampler, uv, i32(c10.x), ddx, ddy).rgb;
        let t01 = textureSampleGrad(materials, material_sampler, uv, i32(c01.x), ddx, ddy).rgb;
        let t11 = textureSampleGrad(materials, material_sampler, uv, i32(c11.x), ddx, ddy).rgb;
        albedo = mix(mix(t00, t10, w.x), mix(t01, t11, w.x), w.y);
        let flat4 = vec4<f32>(f32(c00.y & 1u), f32(c10.y & 1u), f32(c01.y & 1u), f32(c11.y & 1u));
        let liquid4 = vec4<f32>(
            f32((c00.y >> 1u) & 1u), f32((c10.y >> 1u) & 1u),
            f32((c01.y >> 1u) & 1u), f32((c11.y >> 1u) & 1u));
        let w4 = vec4<f32>(
            (1.0 - w.x) * (1.0 - w.y), w.x * (1.0 - w.y),
            (1.0 - w.x) * w.y, w.x * w.y);
        flat_weight = dot(w4, flat4);
        liquid_weight = dot(w4, liquid4);
    }
    // A light line where a liquid meets something else: strongest where the
    // liquid and the rest weigh the same.
    let shore = 4.0 * liquid_weight * (1.0 - liquid_weight);
    albedo = mix(albedo, vec3<f32>(params.shore_r, params.shore_g, params.shore_b), params.shore_amount * shore);
    if ((params.flags & 1u) != 0u) {
        albedo = mix(albedo, lod_tint(in.lod), 0.6);
    }
    // Debug cell grid: a line every gameplay cell (4 m), chunk borders heavier
    // and warm. Skipped
    // entirely when off. `grid_alpha` is a uniform, so the branch is uniform
    // control flow and `fwidth` inside it is allowed.
    if (params.grid_alpha > 0.0) {
        let cell_lines = grid_lines(in.world_xz, params.cell_meters, 1.0);
        let chunk_lines = grid_lines(in.world_xz, CHUNK_METERS, 2.0);
        albedo = mix(albedo, vec3<f32>(0.0, 0.0, 0.0), 0.55 * cell_lines * params.grid_alpha);
        albedo = mix(albedo, vec3<f32>(1.0, 0.75, 0.1), 0.9 * chunk_lines * params.grid_alpha);
    }

    // Flat styles are lit as level ground.
    let n = normalize(mix(normalize(in.normal), vec3<f32>(0.0, 1.0, 0.0), flat_weight));
    let diffuse = max(dot(n, globals.light_dir.xyz), 0.0);
    let lit = globals.ambient.rgb + globals.light_color.rgb * diffuse;
    return vec4<f32>(albedo * lit, 1.0);
}
