// Terrain shader (WP1.7): one shared vertex grid, material per cell.
// The material index map is a texture (R8Uint, one texel per cell); the
// fragment shader blends the four nearest cell centres. Materials live in a
// texture array and are sampled with world-space UVs. Lighting as in
// scene.wgsl: one directional light + ambient, Lambert.

struct Globals {
    view_proj: mat4x4<f32>,
    // Unit vector pointing from the surface towards the light.
    light_dir: vec4<f32>,
    light_color: vec4<f32>,
    ambient: vec4<f32>,
};

struct TerrainParams {
    // Map size in cells.
    map_cells: vec2<i32>,
    // World size of one texture repeat in metres.
    tile_meters: f32,
    _pad: f32,
};

@group(0) @binding(0) var<uniform> globals: Globals;
@group(1) @binding(0) var materials: texture_2d_array<f32>;
@group(1) @binding(1) var material_sampler: sampler;
@group(1) @binding(2) var cell_materials: texture_2d<u32>;
@group(1) @binding(3) var<uniform> params: TerrainParams;

struct VertexIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
};

struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) world_xz: vec2<f32>,
};

@vertex
fn vs_main(in: VertexIn) -> VertexOut {
    var out: VertexOut;
    out.clip = globals.view_proj * vec4<f32>(in.position, 1.0);
    out.normal = in.normal;
    out.world_xz = in.position.xz;
    return out;
}

fn layer_at(cell: vec2<i32>) -> i32 {
    let c = clamp(cell, vec2<i32>(0, 0), params.map_cells - vec2<i32>(1, 1));
    return i32(textureLoad(cell_materials, c, 0).r);
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let uv = in.world_xz / params.tile_meters;
    // Cell centres sit at integer + 0.5; blend between the four nearest,
    // with a smooth ramp over a full cell (centre to centre).
    let p = in.world_xz - vec2<f32>(0.5, 0.5);
    let base = vec2<i32>(floor(p));
    let w = smoothstep(vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 1.0), fract(p));
    let c00 = textureSample(materials, material_sampler, uv, layer_at(base)).rgb;
    let c10 = textureSample(materials, material_sampler, uv, layer_at(base + vec2<i32>(1, 0))).rgb;
    let c01 = textureSample(materials, material_sampler, uv, layer_at(base + vec2<i32>(0, 1))).rgb;
    let c11 = textureSample(materials, material_sampler, uv, layer_at(base + vec2<i32>(1, 1))).rgb;
    let albedo = mix(mix(c00, c10, w.x), mix(c01, c11, w.x), w.y);

    let n = normalize(in.normal);
    let diffuse = max(dot(n, globals.light_dir.xyz), 0.0);
    let lit = globals.ambient.rgb + globals.light_color.rgb * diffuse;
    return vec4<f32>(albedo * lit, 1.0);
}
