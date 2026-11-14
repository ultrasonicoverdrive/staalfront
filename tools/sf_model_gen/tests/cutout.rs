//! The restyled cutscene parts (WP11.6a): the committed PNGs under `assets/scenes/restyle_v5` are
//! what a fresh run writes, every part of both rigs of the M1 briefing has one with the size of
//! the shape it replaces (so pivots and offsets stay right), and the two-shot of the briefing with
//! the parts swapped in renders through the real cutscene renderer. The shipped scene itself is
//! not changed. `SF_SAVE_SHOTS=<dir>` writes the frame before and after.

use std::path::{Path, PathBuf};

use sf_cutscene::render::CutsceneRenderer;
use sf_cutscene::{Art, Scene, load_file};
use sf_model_gen::cutout::{self, Shape};
use sf_model_gen::{CUTOUT_DIR, CUTOUT_SCENE, cutout_files};
use sf_render::image::{self, Rgba8};
use sf_render::{GpuOptions, Renderer};

fn assets() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets")
}

fn scene() -> Scene {
    load_file(&assets().join(CUTOUT_SCENE), Some(&assets()))
        .expect("the briefing loads")
        .scene
}

#[test]
fn the_committed_parts_are_what_a_fresh_run_writes() {
    let files = cutout_files();
    assert_eq!(files, cutout_files(), "two runs differ");
    for (rel, bytes) in &files {
        let on_disk = std::fs::read(assets().join(rel))
            .unwrap_or_else(|e| panic!("{rel} is not committed: {e}"));
        assert!(
            &on_disk == bytes,
            "{rel} is out of date: run `cargo run -p sf_model_gen`"
        );
    }
    // Nothing else in the folder.
    let mut on_disk = 0;
    for ch in std::fs::read_dir(assets().join(CUTOUT_DIR)).unwrap() {
        on_disk += std::fs::read_dir(ch.unwrap().path()).unwrap().count();
    }
    assert_eq!(on_disk, files.len());
}

#[test]
fn every_part_of_both_rigs_has_a_picture_of_its_shape_size() {
    let s = scene();
    assert_eq!(s.characters.len(), 2);
    let files = cutout_files();
    for ch in &s.characters {
        for part in &ch.parts {
            let rel = format!("{CUTOUT_DIR}/{}/{}.png", ch.id, part.name);
            let (_, bytes) = files
                .iter()
                .find(|(p, _)| *p == rel)
                .unwrap_or_else(|| panic!("no picture for {rel}"));
            let img = image::decode_png(bytes).expect("an 8-bit RGBA PNG");
            let (w, h) = match part.art {
                Art::Rect { w, h, .. } | Art::RoundRect { w, h, .. } => (w, h),
                Art::Circle { radius, .. } => (radius * 2.0, radius * 2.0),
                Art::Image(_) => continue,
            };
            let px = |v: f32| (v.ceil() as u32).max(3);
            assert_eq!((img.width, img.height), (px(w), px(h)), "{rel}");
            assert_eq!(!part.mouth_shapes.is_empty(), part.name == "mouth", "{rel}");
        }
    }
}

#[test]
fn a_part_has_two_tones_a_soft_shade_and_a_dark_outline() {
    let colour = [200, 160, 80];
    let (w, h, px) = cutout::draw(
        Shape::RoundRect {
            w: 60.0,
            h: 100.0,
            radius: 10.0,
        },
        colour,
    );
    let at = |x: u32, y: u32| -> [u8; 4] {
        let i = ((y * w + x) * 4) as usize;
        [px[i], px[i + 1], px[i + 2], px[i + 3]]
    };
    let tone = |k: i64| -> [u8; 4] {
        let c = colour.map(|v| (i64::from(v) * k / 1000).min(255) as u8);
        [c[0], c[1], c[2], 255]
    };
    assert_eq!(at(0, 0)[3], 0, "the rounded corner is transparent");
    assert_eq!(
        at(w / 2, 0),
        tone(cutout::OUTLINE),
        "outline at the top edge"
    );
    assert_eq!(
        at(10, 10),
        tone(cutout::LIGHT),
        "light towards the top left"
    );
    assert_eq!(at(w / 2, h / 2), tone(1000), "the base tone in the middle");
    assert_eq!(
        at(w - 6, h - 6),
        tone(cutout::SHADE),
        "shade towards the bottom right"
    );
    let soft = (0..h).flat_map(|y| (0..w).map(move |x| (x, y)));
    assert!(
        soft.filter(|&(x, y)| at(x, y) == tone(cutout::SOFT))
            .count()
            > 50,
        "a soft band"
    );
    // Snapping: an olive close to the palette takes the palette colour; skin keeps its own.
    let olive = [122, 122, 64];
    assert_eq!(cutout::snap([102, 112, 70], &[olive]), olive);
    assert_eq!(cutout::snap([224, 180, 143], &[olive]), [224, 180, 143]);
}

fn renderer(w: u32, h: u32) -> Renderer {
    Renderer::headless(w, h, &GpuOptions::software())
        .or_else(|software_err| {
            if std::env::var_os("CI").is_some() {
                return Err(software_err);
            }
            Renderer::headless(
                w,
                h,
                &GpuOptions {
                    backends: sf_render::wgpu::Backends::PRIMARY,
                    ..GpuOptions::default()
                },
            )
        })
        .unwrap_or_else(|e| panic!("NO GPU ADAPTER AVAILABLE: {e}"))
}

/// Dark outline pixels: near black, but not the black of the bars.
fn outline_pixels(img: &Rgba8) -> usize {
    img.pixels
        .chunks_exact(4)
        .filter(|p| {
            let m = p[0].max(p[1]).max(p[2]);
            (15..75).contains(&m)
        })
        .count()
}

#[test]
fn the_two_shot_renders_with_the_restyled_parts() {
    let original = scene();
    let mut restyled = original.clone();
    for ch in &mut restyled.characters {
        for part in &mut ch.parts {
            part.art = Art::Image(format!("{CUTOUT_DIR}/{}/{}.png", ch.id, part.name));
            for (shape, art) in &mut part.mouth_shapes {
                *art = Art::Image(format!(
                    "{CUTOUT_DIR}/{}/{}_{}.png",
                    ch.id,
                    part.name,
                    shape.letter()
                ));
            }
        }
    }
    // The wide two-shot: the camera has panned back to the middle (38.4 s + 0.8 s).
    let t = 39_400;
    let (w, h) = (1280, 720);
    let mut r = renderer(w, h);
    let gpu = r.gpu();
    let mut cs = CutsceneRenderer::new(&gpu.device, &gpu.queue, r.color_format(), assets());
    let mut shoot = |scene: &Scene| -> Rgba8 {
        let state = scene.sample(t);
        r.render_to_image_with(|target| cs.render(target, scene, &state, None))
            .expect("render")
    };
    let before = shoot(&original);
    let after = shoot(&restyled);
    if let Some(dir) = std::env::var_os("SF_SAVE_SHOTS") {
        image::write_png(&Path::new(&dir).join("two_shot_before.png"), &before).unwrap();
        image::write_png(&Path::new(&dir).join("two_shot_restyled.png"), &after).unwrap();
    }
    // No magenta placeholder (every picture was found) and the outlines show.
    let magenta = after
        .pixels
        .chunks_exact(4)
        .filter(|p| p[0] > 230 && p[1] < 30 && p[2] > 230)
        .count();
    assert_eq!(magenta, 0, "a part picture is missing");
    let (ob, oa) = (outline_pixels(&before), outline_pixels(&after));
    println!("outline pixels: before {ob}, restyled {oa}");
    assert!(oa > ob + 1500, "outlines: before {ob}, restyled {oa}");
}
