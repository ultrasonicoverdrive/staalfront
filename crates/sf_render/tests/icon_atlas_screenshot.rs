//! Screenshot tests for the icon atlas (WP3.6), through the real renderer on a
//! software adapter (same adapter rules as `icons_screenshot.rs`).
//!
//! * every icon of the shipped set is drawn from the atlas, at its real size
//!   of 14 px, in the four player colours on four terrain colours: the contact
//!   sheet `docs/screenshots/wp3.6/icons_14px.png` (set `SF_SAVE_SHOTS=<dir>` to
//!   write it, with its legend);
//! * **no two icons look alike**: at 14 px every pair differs in at least
//!   [`MIN_DIFFERING_PIXELS`] pixels in every player colour (the closest pair
//!   is printed);
//! * a few icon kinds through the whole pipeline (instancer, atlas, shader) at
//!   strategic zoom;
//! * the quad is big enough for the outline at every UI scale (the WP3.3 review
//!   follow-up: a fixed 4 px margin cut off a selected triangle at scale 2);
//! * an icon the atlas does not have is the plain square, not nothing.

use std::collections::BTreeMap;
use std::path::PathBuf;

use glam::{Mat4, Vec3, Vec4};
use sf_data::map::Map;
use sf_math::render_bridge::metres_to_cells;
use sf_math::{Facing, Vec2};
use sf_render::camera::DVec2;
use sf_render::icon_atlas::IconAtlas;
use sf_render::icons::{
    Blend, ICON_HIGHLIGHT, ICON_SELECTED, IconId, IconInstance, IconParams, IconShape,
    snap_icon_center,
};
use sf_render::image::Rgba8;
use sf_render::model::EyeView;
use sf_render::pick::project_px;
use sf_render::style::Shape;
use sf_render::{
    Camera, CameraConfig, FrameView, Frustum, GpuOptions, Instancer, Motion, Renderer, Style,
    UnitId,
};

/// Every pair of icons must differ in at least this many of the 14 x 14 pixels
/// (a pixel differs when a colour channel is off by more than
/// [`PIXEL_DIFFERENCE`]), in each of the four player colours. The closest pair
/// of the shipped set is printed by the test and recorded in
/// `docs/rules/icons.md`; this is what an artist's replacement set has to
/// keep to as well.
const MIN_DIFFERING_PIXELS: usize = 16;
/// A channel difference (of 255) from which two pixels are different.
const PIXEL_DIFFERENCE: u8 = 40;
/// Every sign has a connected dark part of at least this many pixels (of the
/// 196), so it is a shape and not antialiasing specks.
const MIN_SIGN_PIXELS: usize = 6;

/// One cell of a sheet: the 14 px icon with two pixels around it.
const CELL: u32 = 18;

/// The terrain colours of the sheet (sRGB): the grass and the rock of the
/// terrain materials, sand, and the white-blue snow a snow material would
/// have. Flat, so that the sheet shows the contrast and nothing else; the
/// terrain in the game has more texture than this.
const TERRAINS: [(&str, [u8; 3]); 4] = [
    ("green", [74, 128, 52]),
    ("sand", [204, 182, 128]),
    ("grey", [122, 120, 116]),
    ("snow", [236, 240, 246]),
];

fn assets_icons() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/icons")
}

fn atlas() -> IconAtlas {
    let atlas = IconAtlas::load(&assets_icons());
    assert!(atlas.warnings().is_empty(), "{:?}", atlas.warnings());
    atlas
}

fn renderer(width: u32, height: u32) -> Renderer {
    let mut r = Renderer::headless(width, height, &GpuOptions::software())
        .or_else(|software_err| {
            if std::env::var_os("CI").is_some() {
                return Err(software_err);
            }
            Renderer::headless(
                width,
                height,
                &GpuOptions {
                    backends: sf_render::wgpu::Backends::PRIMARY,
                    ..GpuOptions::default()
                },
            )
        })
        .unwrap_or_else(|e| panic!("NO GPU ADAPTER AVAILABLE: {e}"));
    r.set_scene(&[]);
    r
}

/// A view that maps world `(x, 0, z)` to the pixel `(x, z)` of a `w` x `h`
/// target, so that an icon can be put on an exact pixel.
fn pixel_view(w: u32, h: u32) -> Mat4 {
    Mat4::from_cols(
        Vec4::new(2.0 / w as f32, 0.0, 0.0, 0.0),
        Vec4::ZERO,
        Vec4::new(0.0, -2.0 / h as f32, 0.0, 0.0),
        Vec4::new(-1.0, 1.0, 0.5, 1.0),
    )
}

/// An icon to draw: its picture, its colour (linear), where its centre is in
/// pixels, and the flags (hover, selection).
#[derive(Clone, Copy)]
struct Placed {
    icon: IconId,
    rgb: [f32; 3],
    at: (f32, f32),
    flags: u32,
}

/// Draw icons at exact pixels on a flat background, nothing else.
fn draw_icons(
    r: &mut Renderer,
    (w, h): (u32, u32),
    background: [u8; 3],
    placed: &[Placed],
    params: IconParams,
) -> Rgba8 {
    r.set_background(background);
    r.set_view_proj(pixel_view(w, h));
    let instances: Vec<IconInstance> = placed
        .iter()
        .map(|p| {
            let mut i = IconInstance::new([p.at.0, 0.0, p.at.1], p.icon, p.rgb, Blend::ICONS);
            i.flags |= p.flags;
            i
        })
        .collect();
    r.set_icons(&instances, params);
    let img = r.render_to_image().expect("render");
    assert_eq!(r.draw_stats().icon_instances as usize, placed.len());
    assert_eq!(
        r.draw_stats().icon_draw_calls,
        u32::from(!placed.is_empty()),
        "all icons are one draw"
    );
    img
}

/// The pictures of the atlas, one per layer, with every name that has it
/// (`defence_tower = decoy_emplacement`), in layer order: the plain shapes
/// first, then alphabetical.
fn pictures(atlas: &IconAtlas) -> Vec<(String, IconId)> {
    let mut by_layer: BTreeMap<IconId, Vec<&str>> = BTreeMap::new();
    for (name, id) in atlas.names() {
        by_layer.entry(id).or_default().push(name);
    }
    by_layer
        .into_iter()
        .map(|(id, names)| (names.join(" = "), id))
        .collect()
}

/// Every picture in the four player colours (rows) at 14 px, on one terrain
/// colour: a `pictures.len() * CELL` by `4 * CELL` image.
fn sheet(
    r: &mut Renderer,
    style: &Style,
    pictures: &[(String, IconId)],
    background: [u8; 3],
) -> Rgba8 {
    let (w, h) = (pictures.len() as u32 * CELL, 4 * CELL);
    let mut placed = Vec::new();
    for player in 0..4u8 {
        for (i, (_, id)) in pictures.iter().enumerate() {
            placed.push(Placed {
                icon: *id,
                rgb: style.linear(player),
                at: (
                    (i as u32 * CELL + CELL / 2) as f32,
                    (u32::from(player) * CELL + CELL / 2) as f32,
                ),
                flags: 0,
            });
        }
    }
    // The size of the real game at UI scale 1.
    assert_eq!(style.icons().icon_px(1.0), 14.0);
    draw_icons(r, (w, h), background, &placed, style.icons().params(1.0))
}

fn crop(img: &Rgba8, x0: u32, y0: u32, w: u32, h: u32) -> Vec<[u8; 4]> {
    (y0..y0 + h)
        .flat_map(|y| (x0..x0 + w).map(move |x| img.pixel(x, y)))
        .collect()
}

fn differing(a: &[[u8; 4]], b: &[[u8; 4]]) -> usize {
    a.iter()
        .zip(b)
        .filter(|(p, q)| (0..3).any(|k| p[k].abs_diff(q[k]) > PIXEL_DIFFERENCE))
        .count()
}

/// The atlas has every picture and the plain shapes are what they are called.
#[test]
fn the_atlas_is_uploaded_once_and_every_layer_is_drawn() {
    let atlas = atlas();
    let style = Style::embedded();
    let mut r = renderer(64, 64);
    assert_eq!(r.icon_layers(), 4, "before an atlas: the plain shapes");
    r.set_icon_atlas(&atlas);
    assert_eq!(r.icon_layers() as usize, atlas.len());
    // Every layer, in the player's colour, shows its colour (and so was
    // uploaded and sampled), and at least the pictures with a sign show the
    // dark sign too.
    let red = style.srgb(0);
    let pics = pictures(&atlas);
    let mut with_signs = 0;
    for (name, id) in &pics {
        let img = draw_icons(
            &mut r,
            (64, 64),
            [120, 120, 116],
            &[Placed {
                icon: *id,
                rgb: style.linear(0),
                at: (32.0, 32.0),
                flags: 0,
            }],
            style.icons().params(2.0),
        );
        let fill = (0..64 * 64)
            .filter(|&t| {
                let p = img.pixel(t % 64, t / 64);
                (0..3).all(|k| p[k].abs_diff(red[k]) <= 6)
            })
            .count();
        assert!(fill > 40, "{name}: only {fill} pixels in the player colour");
        // Dark pixels not on the rim: at least 3 px from the nearest edge of
        // the coloured area, which is at most 17 px from the centre at scale 2.
        let dark = (0..64 * 64)
            .filter(|&t| {
                let p = img.pixel(t % 64, t / 64);
                p[0] < 80 && p[1] < 80 && p[2] < 80
            })
            .count();
        if !name.starts_with("base_") && name != "civilian" {
            with_signs += 1;
            assert!(dark > 40, "{name}: {dark} dark pixels (rim and sign)");
        }
    }
    assert!(with_signs >= 36, "{with_signs} icons with a sign");
    // Nothing is allocated per frame because of the atlas: drawing again
    // changes no buffer.
    let before = r.alloc_stats();
    let _ = draw_icons(
        &mut r,
        (64, 64),
        [120, 120, 116],
        &[Placed {
            icon: IconId(5),
            rgb: style.linear(1),
            at: (32.0, 32.0),
            flags: 0,
        }],
        style.icons().params(1.0),
    );
    assert_eq!(r.alloc_stats(), before);
}

/// An icon id the atlas does not have is drawn as the plain square, the same
/// as layer 0, never as nothing.
#[test]
fn an_icon_beyond_the_atlas_is_the_plain_square() {
    let style = Style::embedded();
    let mut r = renderer(64, 64);
    let draw = |r: &mut Renderer, id: u16| {
        draw_icons(
            r,
            (64, 64),
            [120, 120, 116],
            &[Placed {
                icon: IconId(id),
                rgb: style.linear(2),
                at: (32.0, 32.0),
                flags: 0,
            }],
            style.icons().params(1.0),
        )
    };
    let square = draw(&mut r, 0);
    let missing = draw(&mut r, 200);
    assert_eq!(square, missing);
    assert_ne!(square, draw(&mut r, 1), "layer 1 is the circle");
    let yellow = style.srgb(2);
    let found = (0..64 * 64)
        .filter(|&t| {
            let p = square.pixel(t % 64, t / 64);
            (0..3).all(|k| p[k].abs_diff(yellow[k]) <= 6)
        })
        .count();
    assert!(found > 80, "{found}");
}

/// The contact sheet and the pairwise check, on one render per terrain.
#[test]
fn every_icon_is_distinguishable_at_14_px_and_the_contact_sheet() {
    let atlas = atlas();
    let style = Style::embedded();
    let pics = pictures(&atlas);
    assert_eq!(pics.len(), atlas.len(), "one picture per layer");
    let mut r = renderer(pics.len() as u32 * CELL, 4 * CELL);
    r.set_icon_atlas(&atlas);

    let sheets: Vec<(&str, Rgba8)> = TERRAINS
        .iter()
        .map(|(name, rgb)| (*name, sheet(&mut r, &style, &pics, *rgb)))
        .collect();
    // Before the checks, so that a failing run still leaves the pictures.
    save_sheet(&sheets, &pics);

    // The pairs: in each terrain and player colour, the pixels of the two
    // 14 px tiles (the cell without its two-pixel margin) that differ. A pair's
    // score is the smallest over the four colours and the four terrains.
    let mut closest: Vec<(usize, usize, usize, usize)> = Vec::new(); // (score, a, b, player)
    for a in 0..pics.len() {
        for b in a + 1..pics.len() {
            let mut worst = (usize::MAX, 0);
            for (_, img) in &sheets {
                for player in 0..4u32 {
                    let tile = |i: usize| crop(img, i as u32 * CELL + 2, player * CELL + 2, 14, 14);
                    let score = differing(&tile(a), &tile(b));
                    if score < worst.0 {
                        worst = (score, player as usize);
                    }
                }
            }
            closest.push((worst.0, a, b, worst.1));
        }
    }
    closest.sort_unstable();
    println!("closest pairs at 14 px (differing pixels of 196, worst colour and terrain):");
    for (score, a, b, player) in closest.iter().take(12) {
        println!(
            "  {score:3}  {} / {}  ({})",
            pics[*a].0,
            pics[*b].0,
            style.color_name(*player as u8)
        );
    }
    let (score, a, b, _) = closest[0];
    println!(
        "closest pair: {} and {} differ in {score} pixels (the minimum asked is {MIN_DIFFERING_PIXELS})",
        pics[a].0, pics[b].0
    );
    for (score, a, b, player) in &closest {
        assert!(
            *score >= MIN_DIFFERING_PIXELS,
            "{} and {} differ in only {score} of 196 pixels in {}",
            pics[*a].0,
            pics[*b].0,
            style.color_name(*player as u8)
        );
    }

    // The sheet itself: the terrain colour is there and so are all four
    // player colours.
    for ((name, bg), (_, img)) in TERRAINS.iter().zip(&sheets) {
        let bg_pixels = img
            .pixels
            .chunks_exact(4)
            .filter(|p| (0..3).all(|k| p[k].abs_diff(bg[k]) <= 3))
            .count();
        assert!(
            bg_pixels > 2000,
            "{name}: only {bg_pixels} background pixels"
        );
    }
}

/// A sign is a solid shape at 14 px, not specks of antialiasing: drawn without
/// the engine's rim (so that only the sign is dark), the largest connected part
/// of dark pixels of every picture that has a sign is at least
/// [`MIN_SIGN_PIXELS`] pixels, in each player colour. The smallest is printed.
#[test]
fn every_sign_is_a_solid_blob_at_14_px() {
    let atlas = atlas();
    let style = Style::embedded();
    let pics = pictures(&atlas);
    let size = (pics.len() as u32 * CELL, 4 * CELL);
    let mut r = renderer(size.0, size.1);
    r.set_icon_atlas(&atlas);
    // No rim: the sign is the only dark.
    let params = IconParams {
        outline_px: 0.0,
        ..style.icons().params(1.0)
    };
    let mut placed = Vec::new();
    for player in 0..4u8 {
        for (i, (_, id)) in pics.iter().enumerate() {
            placed.push(Placed {
                icon: *id,
                rgb: style.linear(player),
                at: (
                    (i as u32 * CELL + CELL / 2) as f32,
                    (u32::from(player) * CELL + CELL / 2) as f32,
                ),
                flags: 0,
            });
        }
    }
    let img = draw_icons(&mut r, size, [122, 120, 116], &placed, params);
    let mut smallest = (usize::MAX, String::new(), 0);
    let mut per_icon: Vec<(usize, String)> = Vec::new();
    for (i, (name, _)) in pics.iter().enumerate() {
        let mut icon_min = usize::MAX;
        let plain = name.starts_with("base_") || name.contains("civilian");
        for player in 0..4u32 {
            let tile = crop(&img, i as u32 * CELL + 2, player * CELL + 2, 14, 14);
            // Dark: well below the player's colour in some channel.
            let fill = style.srgb(player as u8);
            let is_dark =
                |p: &[u8; 4]| (0..3).any(|k| p[k] < fill[k].saturating_sub(100) && p[k] < 110);
            let on: Vec<bool> = tile.iter().map(|p| p[3] == 255 && is_dark(p)).collect();
            let blob = largest_blob(&on, 14);
            if plain {
                assert_eq!(blob, 0, "{name} is a plain shape but has dark pixels");
            } else {
                icon_min = icon_min.min(blob);
                if blob < smallest.0 {
                    smallest = (blob, name.clone(), player as usize);
                }
            }
        }
        if icon_min != usize::MAX {
            per_icon.push((icon_min, name.clone()));
        }
    }
    per_icon.sort();
    println!(
        "smallest signs (connected dark pixels): {:?}",
        &per_icon[..per_icon.len().min(10)]
    );
    println!(
        "smallest sign: {} with {} connected dark pixels in {} (the minimum asked is {MIN_SIGN_PIXELS})",
        smallest.1,
        smallest.0,
        style.color_name(smallest.2 as u8)
    );
    assert!(
        smallest.0 >= MIN_SIGN_PIXELS,
        "the sign of {} is only {} connected pixels at 14 px",
        smallest.1,
        smallest.0
    );
}

/// The size of the largest 4-connected group of set cells in a `side` wide grid.
fn largest_blob(on: &[bool], side: usize) -> usize {
    let mut seen = vec![false; on.len()];
    let mut best = 0;
    for start in 0..on.len() {
        if !on[start] || seen[start] {
            continue;
        }
        let (mut stack, mut n) = (vec![start], 0);
        seen[start] = true;
        while let Some(i) = stack.pop() {
            n += 1;
            let (x, y) = (i % side, i / side);
            let mut next = Vec::new();
            if x > 0 {
                next.push(i - 1);
            }
            if x + 1 < side {
                next.push(i + 1);
            }
            if y > 0 {
                next.push(i - side);
            }
            if i + side < on.len() {
                next.push(i + side);
            }
            for j in next {
                if on[j] && !seen[j] {
                    seen[j] = true;
                    stack.push(j);
                }
            }
        }
        best = best.max(n);
    }
    best
}

/// With `SF_SAVE_SHOTS=<dir>`: `icons_14px.png` (real size, the four terrains
/// one above the other, four player colours per terrain, one picture per
/// column), `icons_14px_x4.png` (four times as big, nearest neighbour) and
/// `icons_14px.txt` (the legend: the pictures in column order).
fn save_sheet(sheets: &[(&str, Rgba8)], pics: &[(String, IconId)]) {
    let Some(dir) = std::env::var_os("SF_SAVE_SHOTS") else {
        return;
    };
    let dir = PathBuf::from(dir);
    let (w, h) = (sheets[0].1.width, sheets[0].1.height);
    let gap = 4;
    let total_h = sheets.len() as u32 * h + (sheets.len() as u32 - 1) * gap;
    let mut all = Rgba8::new(w, total_h, [24, 24, 28, 255].repeat((w * total_h) as usize));
    for (k, (_, img)) in sheets.iter().enumerate() {
        let y0 = k as u32 * (h + gap);
        for y in 0..h {
            for x in 0..w {
                let at = ((y0 + y) * w + x) as usize * 4;
                all.pixels[at..at + 4].copy_from_slice(&img.pixel(x, y));
            }
        }
    }
    sf_render::image::write_png(&dir.join("icons_14px.png"), &all).unwrap();
    let scale = 4;
    let mut big = Rgba8::new(
        w * scale,
        total_h * scale,
        vec![0; (w * scale * total_h * scale * 4) as usize],
    );
    for y in 0..total_h * scale {
        for x in 0..w * scale {
            let at = (y * w * scale + x) as usize * 4;
            big.pixels[at..at + 4].copy_from_slice(&all.pixel(x / scale, y / scale));
        }
    }
    sf_render::image::write_png(&dir.join("icons_14px_x4.png"), &big).unwrap();
    let mut legend = String::from(
        "icons_14px.png: columns are the pictures below, left to right; rows are the four\n\
         player colours (red, blue, yellow, magenta) on green, sand, grey and snow\n\
         (top to bottom). Real size: 14 px icons in 18 px cells.\n\n",
    );
    for (i, (name, _)) in pics.iter().enumerate() {
        legend.push_str(&format!("{:2}  {name}\n", i + 1));
    }
    std::fs::write(dir.join("icons_14px.txt"), legend).unwrap();
}

// ---- the quad holds the outline at every UI scale ---------------------------

/// The pixels of an icon's outline are all inside its quad: nothing is drawn on
/// the border ring of the quad or beyond it, for every base shape, every state
/// and every UI scale. At scale 2 the old fixed 4 px margin failed this for the
/// triangle (its corners reach past the click quad, and the outline of a
/// selected icon is 2.5 px or, scaled, 5 px beyond them).
#[test]
fn the_outline_of_every_shape_fits_the_quad_at_every_ui_scale() {
    let atlas = atlas();
    let style = Style::embedded();
    let (w, h) = (160, 160);
    let mut r = renderer(w, h);
    r.set_icon_atlas(&atlas);
    let bg = [120, 120, 116];
    let empty = draw_icons(&mut r, (w, h), bg, &[], style.icons().params(1.0));
    let flags = [
        ("plain", 0),
        ("hovered", ICON_HIGHLIGHT),
        ("selected", ICON_SELECTED),
        ("both", ICON_HIGHLIGHT | ICON_SELECTED),
    ];
    for scale in [1.0_f32, 1.25, 1.5, 2.0, 3.0] {
        let params = style.icons().params(scale);
        let centre = (80.0_f32, 80.0_f32);
        let snapped = (
            snap_icon_center(centre.0, params.size_px),
            snap_icon_center(centre.1, params.size_px),
        );
        for shape in IconShape::ALL {
            for (state, flag) in flags {
                // The quad of this state: its own outline and a pixel.
                let half = params.quad_half(flag);
                let img = draw_icons(
                    &mut r,
                    (w, h),
                    bg,
                    &[Placed {
                        icon: shape.into(),
                        rgb: style.linear(1),
                        at: centre,
                        flags: flag,
                    }],
                    params,
                );
                let mut outline_extent = 0.0_f32;
                for y in 0..h {
                    for x in 0..w {
                        let (p, q) = (img.pixel(x, y), empty.pixel(x, y));
                        if (0..3).all(|k| p[k].abs_diff(q[k]) <= 2) {
                            continue;
                        }
                        // The distance of the pixel's centre from the quad's
                        // centre, in the chessboard metric: the quad's edge is
                        // at `half`.
                        let d = (x as f32 + 0.5 - snapped.0)
                            .abs()
                            .max((y as f32 + 0.5 - snapped.1).abs());
                        assert!(
                            d < half - 0.5,
                            "{shape:?} {state} at scale {scale}: a pixel at {x},{y} is drawn \
                             {d} px from the centre, the quad ends at {half}"
                        );
                        outline_extent = outline_extent.max(d);
                    }
                }
                // The outline really reaches out (it is not just absent): a
                // selected icon draws further than a plain one.
                if flag == ICON_SELECTED {
                    assert!(
                        outline_extent > params.size_px * 0.5 + params.selected_px - 1.5,
                        "{shape:?} selected at scale {scale}: reaches {outline_extent}"
                    );
                }
            }
        }
    }
}

// ---- a few kinds at strategic zoom ------------------------------------------

fn test_map() -> Map {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/maps/test64");
    Map::load(&dir).expect("committed test map loads")
}

fn fx2(x: f32, z: f32) -> Vec2 {
    Vec2::new(metres_to_cells(x), metres_to_cells(z))
}

/// Role icons through the instancer, at strategic zoom on the test map: each
/// shows its colour and its sign, the signs differ from each other and from
/// the plain base shape, and the plain base shapes have no sign.
#[test]
fn role_icons_at_strategic_zoom_show_their_signs() {
    let map = test_map();
    let atlas = atlas();
    let (w, h) = (640, 360);
    let mut r = renderer(w, h);
    r.set_terrain(&map);
    r.set_icon_atlas(&atlas);

    // Shapes of the style: for each, the picture and a plain cube (the model
    // is not drawn at this zoom).
    let kinds = [
        ("rifleman", IconShape::Circle),
        ("light_tank", IconShape::Triangle),
        ("heavy_tank", IconShape::Triangle),
        ("gunboat", IconShape::HalfCircle),
        ("power_plant", IconShape::Square),
        ("radar", IconShape::Square),
        // No picture of its own: the base shape of its category.
        ("no_such_role", IconShape::Triangle),
    ];
    let base = Style::embedded();
    let shapes: Vec<Shape> = kinds
        .iter()
        .map(|(name, fallback)| Shape {
            mesh: sf_render::scene::MeshKind::Cube,
            size: [1.0, 1.0, 1.0],
            icon: atlas.resolve(Some(name), *fallback),
        })
        .collect();
    // The unknown role got the plain triangle.
    assert_eq!(shapes[6].icon, IconId::from(IconShape::Triangle));
    let style = base.with_shapes(shapes).unwrap();
    let mut inst = Instancer::new(style.clone(), (map.width() as f32, map.height() as f32));
    inst.set_height_range(-20.0, 40.0);

    let motions: Vec<Motion> = (0..kinds.len())
        .map(|i| {
            let at = (32.0 + (i as f32 - 3.0) * 6.0, 32.0);
            Motion::from_fixed(
                i as u8,
                0,
                fx2(at.0, at.1),
                fx2(at.0, at.1),
                Facing::new(0),
                Facing::new(0),
            )
            .with_id(UnitId::from_parts(i as u32, 0))
        })
        .collect();
    inst.update_tick(motions);

    let cfg = CameraConfig {
        map_size: DVec2::new(f64::from(map.width()), f64::from(map.height())),
        ..CameraConfig::default()
    };
    let mut cam = Camera::new(cfg, w, h);
    cam.set_zoom(1.0);
    cam.set_focus(DVec2::new(32.0, 32.0));
    let vp = cam.view_proj();
    r.set_view_proj(vp);
    let frustum = Frustum::from_view_proj(vp);
    let eye = EyeView::from_view_proj(vp, h as f32);
    inst.build_frame(
        &FrameView::new(&frustum, 0.0, eye, Blend::ICONS, 1.0),
        |x, z| map.height_at(x, z),
    );
    r.upload_units(&inst);
    let img = r.render_to_image().expect("render");
    assert_eq!(r.draw_stats().icon_instances, kinds.len() as u32);
    assert_eq!(r.draw_stats().icon_draw_calls, 1);
    if let Some(dir) = std::env::var_os("SF_SAVE_SHOTS") {
        sf_render::image::write_png(&PathBuf::from(dir).join("role_icons_strategic.png"), &img)
            .unwrap();
    }

    // Each icon's 18 x 18 pixel tile, where the shader draws it.
    let size = inst.icon_params().size_px;
    let tiles: Vec<Vec<[u8; 4]>> = (0..kinds.len())
        .map(|i| {
            let x = 32.0 + (i as f32 - 3.0) * 6.0;
            let ground = map.height_at(x, 32.0);
            let (px, _) = project_px(
                &vp,
                Vec3::new(x, ground + inst.icon_config().lift, 32.0),
                [w as f32, h as f32],
            )
            .unwrap();
            let (cx, cy) = (
                snap_icon_center(px[0], size) as u32,
                snap_icon_center(px[1], size) as u32,
            );
            crop(&img, cx - 9, cy - 9, 18, 18)
        })
        .collect();
    let red = style.srgb(0);
    let is_fill = |p: &[u8; 4]| (0..3).all(|k| p[k].abs_diff(red[k]) <= 6);
    let is_dark = |p: &[u8; 4]| p[0] < 80 && p[1] < 80 && p[2] < 80;
    for (i, tile) in tiles.iter().enumerate() {
        let fill = tile.iter().filter(|p| is_fill(p)).count();
        assert!(
            fill > 8,
            "{}: {fill} pixels of the player's colour",
            kinds[i].0
        );
    }
    // The signs: more dark pixels than the plain triangle, which has only its rim.
    let dark = |i: usize| tiles[i].iter().filter(|p| is_dark(p)).count();
    let plain_triangle = dark(6);
    for (i, (name, shape)) in kinds.iter().enumerate().take(6) {
        if *shape == IconShape::Triangle {
            assert!(
                dark(i) > plain_triangle + 3,
                "{name} shows no sign: {} dark pixels against {plain_triangle} for the plain triangle",
                dark(i)
            );
        }
    }
    // The two tanks are both triangles with a sign, and different signs.
    assert!(differing(&tiles[1], &tiles[2]) >= MIN_DIFFERING_PIXELS);
    // And all of them differ from each other.
    for a in 0..kinds.len() {
        for b in a + 1..kinds.len() {
            assert!(
                differing(&tiles[a], &tiles[b]) >= MIN_DIFFERING_PIXELS,
                "{} and {} look alike",
                kinds[a].0,
                kinds[b].0
            );
        }
    }
}
