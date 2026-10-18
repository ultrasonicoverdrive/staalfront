// Strategic icons (WP1.9): one instanced quad per unit, screen-fixed.
//
// The vertex shader projects the unit's world anchor and lays a square of
// `size_px` (plus a small margin for the antialiased edge) around it in
// *pixels*, so the icon is the same size at every zoom. The centre is snapped
// to the pixel grid so that the edges of the shape fall on pixel boundaries.
// The fragment shader draws the shape (square, circle or triangle) as a signed
// distance function: player colour inside, a dark outline along the edge
// (white when highlighted), one pixel of antialiasing.
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

struct IconParams {
    // Render target size in pixels.
    viewport: vec2<f32>,
    // Side of the icon quad and width of the outline, in pixels.
    size_px: f32,
    outline_px: f32,
};

@group(0) @binding(0) var<uniform> globals: Globals;
@group(1) @binding(0) var<uniform> icon: IconParams;

// Extra room around the quad so the antialiased edge and the highlight ring
// are not cut off.
const MARGIN_PX: f32 = 2.5;
// The triangle is wider than the quad's side (its base reaches a pixel or two
// outside the click quad, inside the margin) so that it weighs about as much
// as the square and the circle.
const TRIANGLE_SCALE: f32 = 1.2;
const HIGHLIGHT_BIT: u32 = 0x100u;
const SELECTED_BIT: u32 = 0x200u;
// Extra outline width of a selected icon, on top of the normal outline.
const SELECTED_EXTRA_PX: f32 = 1.2;

struct VertexIn {
    @builtin(vertex_index) vertex: u32,
    // Per instance: world anchor, shape (low byte) and flags, fill color
    // (linear) with the model weight in alpha.
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
    let half = icon.size_px * 0.5 + MARGIN_PX;
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

// Distance from `p` to the segment `a`-`b`.
fn dist_segment(p: vec2<f32>, a: vec2<f32>, b: vec2<f32>) -> f32 {
    let ab = b - a;
    let t = clamp(dot(p - a, ab) / dot(ab, ab), 0.0, 1.0);
    return length(p - a - ab * t);
}

fn cross2(a: vec2<f32>, b: vec2<f32>) -> f32 {
    return a.x * b.y - a.y * b.x;
}

// Signed distance (negative inside) to an upward-pointing equilateral triangle
// with half-width `r`, its bounding box centred on the origin: the distance to
// the nearest edge, negative when `p` is on the inner side of all three edges
// (the corners a, b, c run counter-clockwise).
fn sd_triangle(p: vec2<f32>, r: f32) -> f32 {
    let h = r * 1.7320508;
    let a = vec2<f32>(0.0, 0.5 * h);
    let b = vec2<f32>(-r, -0.5 * h);
    let c = vec2<f32>(r, -0.5 * h);
    let d = min(dist_segment(p, a, b), min(dist_segment(p, b, c), dist_segment(p, c, a)));
    let inside = cross2(b - a, p - a) >= 0.0
        && cross2(c - b, p - b) >= 0.0
        && cross2(a - c, p - c) >= 0.0;
    return select(d, -d, inside);
}

// Distance from the edge in pixels, negative inside, for shape 0 = square,
// 1 = circle, 2 = triangle.
fn sd_shape(shape: u32, p: vec2<f32>, r: f32) -> f32 {
    if (shape == 1u) {
        return length(p) - r;
    }
    if (shape == 2u) {
        return sd_triangle(p, r * TRIANGLE_SCALE);
    }
    return max(abs(p.x), abs(p.y)) - r;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    // The icon is drawn where the model is not.
    if (dither_threshold(in.clip.xy) < in.weight) {
        discard;
    }
    let highlighted = (in.flags & HIGHLIGHT_BIT) != 0u;
    let selected = (in.flags & SELECTED_BIT) != 0u;
    let d = sd_shape(in.flags & 0xffu, in.local, icon.size_px * 0.5);
    let coverage = clamp(0.5 - d, 0.0, 1.0);
    if (coverage <= 0.0) {
        discard;
    }
    // The outline is a ring just inside the edge; a highlighted icon gets a
    // thicker white one, a selected icon (WP3.3) a thicker one still.
    var ring = select(icon.outline_px, icon.outline_px + 1.5, highlighted);
    ring = select(ring, icon.outline_px + SELECTED_EXTRA_PX, selected);
    let edge = clamp(d + ring + 0.5, 0.0, 1.0) * select(0.0, 1.0, ring > 0.01);
    let outline = select(
        vec3<f32>(0.02, 0.025, 0.035),
        vec3<f32>(1.0),
        highlighted || selected,
    );
    return vec4<f32>(mix(in.color, outline, edge), coverage);
}
