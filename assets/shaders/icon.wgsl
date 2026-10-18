// Strategic icons (WP1.9, pictures from the icon atlas in WP3.6): one instanced
// quad per unit, screen-fixed.
//
// The vertex shader projects the unit's world anchor and lays a square around
// it in *pixels*: the art box (`box_px`, 1.2 times the icon size `size_px`)
// plus a margin for the outline of the icon's state (`ceil(reach) + 1`, so a
// plain icon has one pixel and a selected one its outline width and one), so
// the icon is the same size at every zoom and a plain icon, the common case,
// is the smallest quad. The centre is snapped to the pixel grid so that
// the edges of the shape fall on pixel boundaries. Every length arrives in
// physical pixels, already multiplied by the UI scale (icons.rs,
// IconConfig::params), the outline widths included; the margin follows them.
//
// The picture is a layer of the icon atlas (`icon_atlas.rs`): two signed
// distance fields made from the 64 x 64 PNG, R for the silhouette and G for the
// inner sign, `0.5` at the edge, 32 texels over the 0..1 range. The fragment
// shader turns them into a distance to the edge in screen pixels and does what
// the old procedural shapes did with theirs: the player colour inside, the sign
// in the dark colour, a dark rim just inside the edge, one pixel of
// antialiasing. Hover and selection (WP3.3) add a light outline *outside* the
// shape, so the player colour keeps the same area in every state.
//
// Cross-fade to the models: the alpha of the instance color is the model's
// weight `w`. The model (scene.wgsl) keeps the pixels whose 8x8 Bayer dither
// threshold is below `w`; the icon draws exactly the others. No pixel is both.
//
// Not depth tested: icons are drawn over terrain and models (see icons.rs).

struct Globals {
    view_proj: mat4x4<f32>,
    light_dir: vec4<f32>,
    light_color: vec4<f32>,
    ambient: vec4<f32>,
};

// Keep the order and the types in step with `IconParams::uniform` (icons.rs);
// a test reads this declaration.
struct IconParams {
    // Render target size in pixels.
    viewport: vec2<f32>,
    // Side of the icon's click quad, the nominal size.
    size_px: f32,
    // Width of the dark rim inside the shape's edge.
    outline_px: f32,
    // Side of the art box the picture is drawn in.
    box_px: f32,
    // Width of the outline outside the shape: hovered, selected.
    hover_px: f32,
    selected_px: f32,
};

@group(0) @binding(0) var<uniform> globals: Globals;
@group(1) @binding(0) var<uniform> icon: IconParams;
@group(1) @binding(1) var atlas: texture_2d_array<f32>;
@group(1) @binding(2) var atlas_sampler: sampler;

// Side of a picture in texels, and how many texels the distance fields reach on
// each side of the edge (icon_atlas.rs ATLAS SIDE and SDF_SPREAD; a test keeps
// them equal).
const ATLAS_SIDE: f32 = 64.0;
const SDF_SPREAD: f32 = 16.0;
const HIGHLIGHT_BIT: u32 = 0x100u;
const SELECTED_BIT: u32 = 0x200u;
// The atlas layer is in the top half of the flags.
const LAYER_SHIFT: u32 = 16u;
// The rim and the sign.
const DARK: vec3<f32> = vec3<f32>(0.02, 0.025, 0.035);

// How far the outline of an icon with these flags reaches outside its shape
// (0: none). Hovered: thin and white. Selected (WP3.3): clearly thicker, white.
// Selected and hovered: the selected width in yellow, so the four states all
// look different and the fill never shrinks. IconParams::reach in icons.rs.
fn outline_reach(flags: u32) -> f32 {
    var reach = select(0.0, icon.hover_px, (flags & HIGHLIGHT_BIT) != 0u);
    reach = select(reach, icon.selected_px, (flags & SELECTED_BIT) != 0u);
    return reach;
}

struct VertexIn {
    @builtin(vertex_index) vertex: u32,
    // Per instance: world anchor, atlas layer and flags, fill color (linear)
    // with the model weight in alpha.
    @location(0) position: vec3<f32>,
    @location(1) flags: u32,
    @location(2) color: vec4<f32>,
};

struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    // Offset from the icon centre in pixels, y up.
    @location(0) local: vec2<f32>,
    @location(1) @interpolate(flat) flags: u32,
    @location(2) @interpolate(flat) color: vec3<f32>,
    @location(3) @interpolate(flat) weight: f32,
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
    let clip = globals.view_proj * vec4<f32>(in.position, 1.0);
    var out: VertexOut;
    out.flags = in.flags;
    out.color = in.color.rgb;
    out.weight = in.color.a;
    if (clip.w <= 0.0001) {
        // Behind the camera: put the quad outside the clip volume.
        out.clip = vec4<f32>(2.0, 2.0, 2.0, 1.0);
        out.local = vec2<f32>(0.0, 0.0);
        return out;
    }
    let ndc = clip.xy / clip.w;
    let centre = vec2<f32>(
        (ndc.x * 0.5 + 0.5) * icon.viewport.x,
        (0.5 - ndc.y * 0.5) * icon.viewport.y,
    );
    // sf_render::icons::snap_icon_center: even sides centre on a pixel corner,
    // odd sides on a pixel centre.
    let even = (i32(floor(icon.size_px + 0.5)) & 1) == 0;
    let snapped = select(floor(centre) + vec2<f32>(0.5), floor(centre + vec2<f32>(0.5)), even);
    // IconParams::quad_half: the art box, the outline rounded up, one pixel.
    let half = icon.box_px * 0.5 + ceil(outline_reach(in.flags)) + 1.0;
    let local = corner * half;
    let px = snapped + vec2<f32>(local.x, -local.y);
    out.clip = vec4<f32>(
        px.x / icon.viewport.x * 2.0 - 1.0,
        1.0 - px.y / icon.viewport.y * 2.0,
        0.5,
        1.0,
    );
    out.local = local;
    return out;
}

// Threshold of the pixel in the 8x8 Bayer ordering, in (0, 1). The same
// integer formula as sf_render::icons::dither_rank (keep scene.wgsl in step).
fn dither_threshold(pixel: vec2<f32>) -> f32 {
    let x = u32(pixel.x) & 7u;
    let y = u32(pixel.y) & 7u;
    let xy = x ^ y;
    let rank = ((xy & 1u) << 5u) | ((y & 1u) << 4u) | ((xy & 2u) << 2u)
        | ((y & 2u) << 1u) | ((xy & 4u) >> 1u) | ((y & 4u) >> 2u);
    return (f32(rank) + 0.5) / 64.0;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    // The icon is drawn where the model is not.
    if (dither_threshold(in.clip.xy) < in.weight) {
        discard;
    }
    let highlighted = (in.flags & HIGHLIGHT_BIT) != 0u;
    let selected = (in.flags & SELECTED_BIT) != 0u;
    let reach = outline_reach(in.flags);

    // Beyond the art box the picture is not there, only the outline can be: past
    // its reach (and the antialiased half pixel) there is nothing to draw, and
    // the atlas is not read. For a plain icon that is the whole margin.
    let beyond = max(abs(in.local) - vec2<f32>(icon.box_px * 0.5), vec2<f32>(0.0));
    let outside = length(beyond);
    if (outside > reach + 0.5) {
        discard;
    }

    // The picture: distances to the edge of the silhouette and of the sign, in
    // texels, negative inside, then in screen pixels. Beyond the art box the
    // fields are the box's edge value plus the distance to the box (the
    // picture lies inside it).
    // A layer the atlas does not have (an icon id from a bigger atlas than the
    // one uploaded) is the plain square, not an invisible icon.
    let wanted = in.flags >> LAYER_SHIFT;
    let layer = select(wanted, 0u, wanted >= textureNumLayers(atlas));
    let uv = vec2<f32>(in.local.x / icon.box_px + 0.5, 0.5 - in.local.y / icon.box_px);
    let field = textureSampleLevel(atlas, atlas_sampler, uv, layer, 0.0).rg;
    let px_per_texel = icon.box_px / ATLAS_SIDE;
    let to_px = 2.0 * SDF_SPREAD * px_per_texel;
    let d = (0.5 - field.r) * to_px + outside;
    let d_sign = (0.5 - field.g) * to_px + outside;

    // The outer outline (reach above) covers up to `reach` beyond the edge.
    let coverage = clamp(0.5 - (d - reach), 0.0, 1.0);
    if (coverage <= 0.0) {
        discard;
    }
    // Inside the shape: the player colour, the sign in the dark colour, and the
    // dark rim just inside the edge, the same in every state.
    let rim = clamp(d + icon.outline_px + 0.5, 0.0, 1.0)
        * select(0.0, 1.0, icon.outline_px > 0.01);
    let sign = clamp(0.5 - d_sign, 0.0, 1.0);
    let inside = mix(mix(in.color, DARK, sign), DARK, rim);
    if (reach <= 0.0) {
        return vec4<f32>(inside, coverage);
    }
    // Outside the shape: the light outline, yellow when both hovered and
    // selected. The shape's own antialiased edge blends into it.
    var outline = vec3<f32>(1.0);
    outline = select(outline, vec3<f32>(1.0, 0.85, 0.2), highlighted && selected);
    let shape_cover = clamp(0.5 - d, 0.0, 1.0);
    return vec4<f32>(mix(outline, inside, shape_cover), coverage);
}
