//! F1 sound browser (WP11.7): lists the MIX archives and AUD entries found
//! under the configured original Red Alert audio path, with a play button
//! per sound and the decode error per broken entry, so the mapping in
//! `assets/audio/ra_mapping.ron` can be filled by ear. Only shown while
//! original Red Alert audio is on.

use egui::{Align2, CollapsingHeader, Color32, RichText};
use sf_audio::RaStatus;
use sf_audio::ra::{EntryKind, aud::Compression};

use crate::debug_ui::{PanelData, PanelState};

/// The window title (tests look for it).
pub const SOUND_BROWSER_TITLE: &str = "Sound browser (Red Alert)";

const ROW_HEIGHT: f32 = 18.0;

pub fn show(ctx: &egui::Context, data: &PanelData, state: &mut PanelState) {
    let Some(status) = &data.ra_status else {
        return;
    };
    if *status == RaStatus::Off {
        return;
    }
    egui::Window::new(SOUND_BROWSER_TITLE)
        .anchor(Align2::LEFT_BOTTOM, [8.0, -8.0])
        .default_open(true)
        .resizable(false)
        .show(ctx, |ui| {
            match status {
                RaStatus::Loading => {
                    ui.label("Scanning the Red Alert files...");
                }
                RaStatus::Ready { mapped } => {
                    ui.label(format!("{mapped} events use an original sound"));
                }
                RaStatus::Off => {}
            }
            if let Some(e) = &data.ra_play_error {
                ui.colored_label(Color32::LIGHT_RED, format!("play: {e}"));
            }
            let Some(cat) = &data.ra_catalog else {
                return;
            };
            ui.label(format!(
                "{}: {} archives, {} sounds",
                cat.root.display(),
                cat.archives.len(),
                cat.sound_count()
            ));
            for p in &cat.problems {
                ui.colored_label(Color32::LIGHT_RED, p);
            }
            for (ai, archive) in cat.archives.iter().enumerate() {
                let title = format!(
                    "{}{} ({} entries)",
                    archive.label,
                    if archive.encrypted {
                        " [encrypted]"
                    } else {
                        ""
                    },
                    archive.entries.len()
                );
                CollapsingHeader::new(title)
                    .id_salt(("ra_archive", ai))
                    .default_open(false)
                    .show(ui, |ui| {
                        egui::ScrollArea::vertical()
                            .id_salt(("ra_scroll", ai))
                            .max_height(260.0)
                            .show_rows(ui, ROW_HEIGHT, archive.entries.len(), |ui, rows| {
                                for ei in rows {
                                    let e = &archive.entries[ei];
                                    ui.horizontal(|ui| match &e.kind {
                                        EntryKind::Aud {
                                            sample_rate,
                                            channels,
                                            millis,
                                            compression,
                                        } => {
                                            if ui.small_button("play").clicked() {
                                                state.ra_play = Some((ai, ei));
                                            }
                                            ui.monospace(e.label());
                                            let codec = match compression {
                                                Compression::Ima => "IMA",
                                                Compression::Westwood => "WS",
                                            };
                                            ui.weak(format!(
                                                "{codec} {sample_rate} Hz {} {millis} ms",
                                                if *channels == 2 { "stereo" } else { "mono" }
                                            ));
                                        }
                                        EntryKind::Broken(err) => {
                                            ui.monospace(e.label());
                                            ui.label(RichText::new(err).color(Color32::LIGHT_RED));
                                        }
                                        EntryKind::Mix => {
                                            ui.monospace(e.label());
                                            ui.weak("archive (listed below)");
                                        }
                                        EntryKind::Other => {
                                            ui.monospace(e.label());
                                            ui.weak(format!("{} bytes, not audio", e.size));
                                        }
                                    });
                                }
                            });
                    });
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use sf_audio::ra::{ArchiveInfo, Catalog, EntryInfo};
    use sf_render::CameraConfig;
    use std::sync::Arc;

    fn texts(ctx: &egui::Context, data: &PanelData) -> Vec<String> {
        fn walk(shape: &egui::epaint::Shape, out: &mut Vec<String>) {
            match shape {
                egui::epaint::Shape::Text(t) => out.push(t.galley.text().to_owned()),
                egui::epaint::Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
                _ => {}
            }
        }
        let mut state = PanelState::new(CameraConfig::default());
        let mut out = Vec::new();
        for _ in 0..2 {
            let o = ctx.run_ui(egui::RawInput::default(), |ui| {
                show(ui.ctx(), data, &mut state)
            });
            out.clear();
            o.shapes.iter().for_each(|c| walk(&c.shape, &mut out));
            let mut d = o.textures_delta;
            d.clear();
        }
        out
    }

    #[test]
    fn hidden_unless_original_audio_is_on() {
        let ctx = egui::Context::default();
        assert!(texts(&ctx, &PanelData::default()).is_empty());
        let off = PanelData {
            ra_status: Some(RaStatus::Off),
            ..PanelData::default()
        };
        assert!(texts(&ctx, &off).is_empty());
    }

    #[test]
    fn lists_archives_problems_and_play_errors() {
        let ctx = egui::Context::default();
        let entry = |name: &str, kind| EntryInfo {
            id: 1,
            name: Some(name.into()),
            path: "x".into(),
            offset: 0,
            size: 10,
            kind,
        };
        let catalog = Catalog {
            root: "ra".into(),
            archives: vec![ArchiveInfo {
                label: "MAIN.MIX".into(),
                encrypted: true,
                entries: vec![
                    entry(
                        "A.AUD",
                        EntryKind::Aud {
                            sample_rate: 22050,
                            channels: 1,
                            millis: 100,
                            compression: Compression::Ima,
                        },
                    ),
                    entry("B.AUD", EntryKind::Broken("AUD data is truncated".into())),
                ],
            }],
            problems: vec!["JUNK.MIX: file is truncated".into()],
        };
        let data = PanelData {
            ra_status: Some(RaStatus::Ready { mapped: 2 }),
            ra_catalog: Some(Arc::new(catalog)),
            ra_play_error: Some("boom".into()),
            ..PanelData::default()
        };
        let all = texts(&ctx, &data);
        let has = |n: &str| all.iter().any(|t| t.contains(n));
        for needle in [
            SOUND_BROWSER_TITLE,
            "2 events use an original sound",
            "1 archives, 1 sounds",
            "JUNK.MIX: file is truncated",
            "MAIN.MIX [encrypted] (2 entries)",
            "play: boom",
        ] {
            assert!(has(needle), "missing {needle:?} in {all:?}");
        }
        let loading = PanelData {
            ra_status: Some(RaStatus::Loading),
            ..PanelData::default()
        };
        assert!(texts(&ctx, &loading).iter().any(|t| t.contains("Scanning")));
    }
}
