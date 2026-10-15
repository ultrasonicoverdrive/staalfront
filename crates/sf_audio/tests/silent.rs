//! Integration: construct the real `AudioSystem` in an environment without
//! a sound device (CI, containers) and drive the whole API. Must not panic
//! whether or not a device exists.

use std::path::Path;
use std::time::Duration;

use sf_audio::{AudioSystem, Category, Listener, SpatialParams};

fn assets() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets")
}

#[test]
fn full_api_without_a_device_does_not_panic() {
    let mut audio = AudioSystem::new(assets());
    println!(
        "audio output: {}",
        if audio.is_silent() {
            "silent"
        } else {
            "device"
        }
    );

    audio.set_listener(Listener::from_eye_focus([0.0, 30.0, 30.0], [0.0, 0.0, 0.0]));
    audio.set_spatial_params(SpatialParams::default());
    audio.set_crossfade(Duration::from_millis(100));
    audio.set_category_volume(Category::Sfx, 0.7);

    audio.play("test_click");
    audio.play_at("test_click", [3.0, 0.0, -4.0]);
    for _ in 0..100 {
        audio.play_at("test_click", [1.0, 0.0, 1.0]); // voice limiting
    }
    audio.play("no_such_sound");
    audio.play_music("test_loop");
    audio.play_music("test_loop");
    audio.update();
    audio.play_music("test_click"); // crossfade to another track
    audio.stop_music();
    audio.update();
    assert!(audio.reload_bank().is_empty());

    // Zoom out and in while sounds play.
    for d in [10.0, 40.0, 200.0, 2000.0] {
        audio.set_listener(Listener::new([0.0; 3], [1.0, 0.0, 0.0], d));
        audio.play_at("test_click", [50.0, 0.0, 50.0]);
        audio.update();
    }
}

#[test]
fn missing_assets_dir_is_survivable() {
    let mut audio = AudioSystem::new("/no/such/assets");
    audio.play("test_click");
    audio.play_music("test_loop");
    audio.stop_music();
}
