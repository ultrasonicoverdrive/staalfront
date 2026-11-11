//! The minimap's GPU texture (WP1.16a): the fog changes the picture about 20 times a second in play, and
//! the upload used to create a texture, a view and a bind group every time (about 8,700 in the 7-minute
//! play of follow-up 35). It now keeps them while the size stays and only writes the pixels.

use sf_data::map::generate::generate_cells;
use sf_render::{GpuOptions, Renderer};
use sf_ui::{FOG, Minimap, MinimapFog, UiRenderer, VISIBLE};

const LOOK: MinimapFog = MinimapFog {
    shroud: [4, 8, 12],
    fog_dim: 0.5,
};

fn renderer() -> Renderer {
    Renderer::headless(64, 64, &GpuOptions::software())
        .or_else(|e| {
            if std::env::var_os("CI").is_some() {
                return Err(e);
            }
            Renderer::headless(64, 64, &GpuOptions::default())
        })
        .unwrap_or_else(|e| panic!("NO GPU ADAPTER AVAILABLE: {e}"))
}

#[test]
fn many_fog_updates_create_one_texture_and_a_new_size_creates_one_more() {
    let r = renderer();
    let gpu = r.gpu();
    let mut ui = UiRenderer::new(&gpu.device, r.color_format());
    let map = generate_cells(16, 16, 1);
    let mut minimap = Minimap::bake(&map, 128);
    ui.set_minimap(&gpu.device, &gpu.queue, minimap.image());
    assert_eq!(ui.minimap_textures_created(), 1);
    let started = std::time::Instant::now();
    let mut upload = std::time::Duration::ZERO;
    for step in 0..200u32 {
        let states: Vec<u8> = (0..16 * 16)
            .map(|i| if (i + step) % 7 < 3 { FOG } else { VISIBLE })
            .collect();
        assert!(minimap.set_fog(&map, Some((&states, LOOK))));
        let t = std::time::Instant::now();
        ui.set_minimap(&gpu.device, &gpu.queue, minimap.image());
        upload += t.elapsed();
    }
    println!(
        "200 fog updates: {} textures created, set_minimap {:.3} ms each, with the fog {:.3} ms each",
        ui.minimap_textures_created(),
        upload.as_secs_f64() * 1e3 / 200.0,
        started.elapsed().as_secs_f64() * 1e3 / 200.0
    );
    assert_eq!(
        ui.minimap_textures_created(),
        1,
        "200 fog updates of the same size must reuse the texture"
    );
    // Another map, another size of picture: a new texture.
    let other = Minimap::bake(&generate_cells(16, 16, 2), 64);
    ui.set_minimap(&gpu.device, &gpu.queue, other.image());
    assert_eq!(ui.minimap_textures_created(), 2);
}
