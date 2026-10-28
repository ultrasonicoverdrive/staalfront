//! Pure positional-sound maths: volume and stereo pan from a listener and a
//! source. No device, no kira types; fully unit-tested.
//!
//! World convention (as in `sf_render`): `+Y` is up, the ground is the XZ
//! plane. Only the horizontal offset counts; height is ignored.
//!
//! The model:
//! - **Distance**: full volume inside a reference radius, then a squared
//!   fade to silence at the maximum radius. Both radii are fixed in world
//!   metres.
//! - **Zoom** ("quieter when zoomed out"): a global gain that falls off as
//!   `near / camera_distance` beyond a "near" camera distance, down to a
//!   floor.
//! - **Monotonic by construction**: total gain = distance gain (independent
//!   of the camera distance) x zoom gain (non-increasing in it), so for any
//!   fixed world position, zooming out never makes a sound louder.
//! - **Data**: every number is in `assets/config/audio.ron` ([`crate::config`]).
//! - **Pan**: the source offset projected on the camera's right vector,
//!   relative to a width that grows with camera distance (about the
//!   half-width of the picture), clamped to `±max_pan`. Pan does not affect
//!   gain.

/// What the player "hears from": derived from the camera every frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Listener {
    /// World point the camera looks at (the ground point under the screen centre).
    pub focus: [f32; 3],
    /// Camera right vector in world space (need not be normalised or horizontal).
    pub right: [f32; 3],
    /// Distance from the camera to the focus point (the zoom level), in metres.
    pub distance: f32,
}

impl Listener {
    pub fn new(focus: [f32; 3], right: [f32; 3], distance: f32) -> Self {
        Self {
            focus,
            right,
            distance,
        }
    }

    /// Derive the listener from the camera eye and focus point (world up is
    /// `+Y`). Looking straight down has no defined right vector; it falls
    /// back to `+X`.
    pub fn from_eye_focus(eye: [f32; 3], focus: [f32; 3]) -> Self {
        let f = [focus[0] - eye[0], focus[1] - eye[1], focus[2] - eye[2]];
        let distance = (f[0] * f[0] + f[1] * f[1] + f[2] * f[2]).sqrt();
        // right = forward x up, up = +Y.
        let right = [-f[2], 0.0, f[0]];
        let right = if right[0] * right[0] + right[2] * right[2] < 1e-12 {
            [1.0, 0.0, 0.0]
        } else {
            right
        };
        Self::new(focus, right, distance)
    }
}

impl Default for Listener {
    /// At the origin, looking north (`-Z`), at the default play height.
    fn default() -> Self {
        Self::new(
            [0.0; 3],
            [1.0, 0.0, 0.0],
            SpatialParams::default().near_distance,
        )
    }
}

/// Tuning of the model. All distances in metres.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpatialParams {
    /// Full volume within this horizontal distance from the focus.
    pub ref_radius: f32,
    /// Silent beyond this horizontal distance from the focus.
    pub max_radius: f32,
    /// Camera distance up to which no zoom attenuation applies.
    pub near_distance: f32,
    /// Lowest zoom gain (reached when zoomed far out).
    pub min_zoom_gain: f32,
    /// Half-width of the stereo image at zero camera distance.
    pub pan_width: f32,
    /// Growth of that half-width per metre of camera distance.
    pub pan_growth: f32,
    /// Largest `|pan|` (keeps a sound from being hard-left/right).
    pub max_pan: f32,
    /// Gain below which a sound is not worth a voice.
    pub audible_gain: f32,
}

impl Default for SpatialParams {
    /// The numbers of the shipped `assets/config/audio.ron` (compiled in);
    /// there are none in Rust.
    fn default() -> Self {
        crate::config::AudioConfig::embedded().spatial.params()
    }
}

/// Result of [`mix`]: linear amplitude and stereo pan.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mix {
    /// Linear gain, `0.0..=1.0`.
    pub gain: f32,
    /// `-1.0` (left) ..= `1.0` (right).
    pub pan: f32,
}

impl Mix {
    /// Whether the sound is loud enough to be worth a voice.
    pub fn is_audible(&self, p: &SpatialParams) -> bool {
        self.gain >= p.audible_gain
    }
}

/// Gain from the camera distance alone (the zoom level), `min_zoom_gain..=1`.
pub fn zoom_gain(camera_distance: f32, p: &SpatialParams) -> f32 {
    let d = sane(camera_distance);
    if d <= p.near_distance || p.near_distance <= 0.0 {
        return 1.0;
    }
    (p.near_distance / d).clamp(p.min_zoom_gain.clamp(0.0, 1.0), 1.0)
}

/// Gain from the horizontal distance (metres) to the focus.
pub fn distance_gain(horizontal: f32, p: &SpatialParams) -> f32 {
    let reference = p.ref_radius.max(0.0);
    let max = p.max_radius.max(reference + 1e-3);
    let t = ((sane(horizontal) - reference) / (max - reference)).clamp(0.0, 1.0);
    let inv = 1.0 - t;
    inv * inv
}

/// Volume and pan of a source at `source` as heard by `listener`.
pub fn mix(listener: &Listener, source: [f32; 3], p: &SpatialParams) -> Mix {
    let ox = source[0] - listener.focus[0];
    let oz = source[2] - listener.focus[2];
    let cam = sane(listener.distance);
    let horizontal = (ox * ox + oz * oz).sqrt();
    let gain = distance_gain(horizontal, p) * zoom_gain(cam, p);

    let (rx, rz) = (listener.right[0], listener.right[2]);
    let rlen = (rx * rx + rz * rz).sqrt();
    let pan = if rlen > 1e-6 && ox.is_finite() && oz.is_finite() {
        let across = (ox * rx + oz * rz) / rlen;
        let width = (p.pan_width + p.pan_growth * cam).max(1e-3);
        (across / width).clamp(-1.0, 1.0) * p.max_pan.clamp(0.0, 1.0)
    } else {
        0.0
    };
    Mix {
        gain: if gain.is_finite() { gain } else { 0.0 },
        pan,
    }
}

/// Non-finite or negative inputs count as 0 so a bad camera never poisons the mix.
fn sane(v: f32) -> f32 {
    if v.is_finite() { v.max(0.0) } else { 0.0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    const P: SpatialParams = SpatialParams {
        ref_radius: 25.0,
        max_radius: 125.0,
        near_distance: 40.0,
        min_zoom_gain: 0.2,
        pan_width: 10.0,
        pan_growth: 1.0,
        max_pan: 0.9,
        audible_gain: 0.003,
    };

    fn at(distance: f32) -> Listener {
        Listener::new([100.0, 0.0, 100.0], [1.0, 0.0, 0.0], distance)
    }

    fn near(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    #[test]
    fn source_on_the_focus_is_full_volume_and_centred() {
        let m = mix(&at(30.0), [100.0, 0.0, 100.0], &P);
        assert!(near(m.gain, 1.0), "{m:?}");
        assert!(near(m.pan, 0.0));
    }

    #[test]
    fn full_volume_inside_the_reference_radius() {
        assert!(near(mix(&at(30.0), [120.0, 0.0, 100.0], &P).gain, 1.0));
        assert!(near(mix(&at(30.0), [100.0, 0.0, 125.0], &P).gain, 1.0));
    }

    #[test]
    fn gain_falls_monotonically_to_silence() {
        let l = at(30.0);
        let mut prev = 2.0;
        for step in 0..60 {
            let g = mix(&l, [100.0 + step as f32 * 3.0, 0.0, 100.0], &P).gain;
            assert!(g <= prev + 1e-6, "step {step}: {g} > {prev}");
            prev = g;
        }
        assert!(near(mix(&l, [100.0 + 125.0, 0.0, 100.0], &P).gain, 0.0));
        assert!(near(mix(&l, [100.0 + 500.0, 0.0, 100.0], &P).gain, 0.0));
        assert!(!mix(&l, [100.0 + 500.0, 0.0, 100.0], &P).is_audible(&P));
    }

    #[test]
    fn distance_gain_is_squared_fade_midway() {
        // reference 25, max 125; at 75 m t = 0.5 -> gain 0.25
        assert!(near(distance_gain(75.0, &P), 0.25));
        assert!(near(distance_gain(25.0, &P), 1.0));
        assert!(near(distance_gain(125.0, &P), 0.0));
    }

    #[test]
    fn height_is_ignored() {
        let l = at(30.0);
        let a = mix(&l, [150.0, 0.0, 100.0], &P);
        let b = mix(&l, [150.0, 40.0, 100.0], &P);
        assert_eq!(a, b);
    }

    #[test]
    fn zooming_out_makes_the_same_source_quieter() {
        // A source on the focus: only the zoom gain differs.
        let src = [100.0, 0.0, 100.0];
        let g_near = mix(&at(20.0), src, &P).gain;
        let g_mid = mix(&at(80.0), src, &P).gain;
        let g_far = mix(&at(400.0), src, &P).gain;
        assert!(near(g_near, 1.0));
        assert!(near(g_mid, 0.5));
        assert!(g_far < g_mid);
        assert!(near(g_far, 0.2), "floor: {g_far}");
        // never below the floor, however far
        assert!(near(mix(&at(1.0e6), src, &P).gain, 0.2));
    }

    #[test]
    fn zoom_gain_is_one_up_to_the_near_distance() {
        assert!(near(zoom_gain(0.0, &P), 1.0));
        assert!(near(zoom_gain(40.0, &P), 1.0));
        assert!(zoom_gain(41.0, &P) < 1.0);
    }

    #[test]
    fn zooming_out_never_makes_a_fixed_source_louder() {
        // Grid of source offsets (incl. 60 m off-centre, the case that used
        // to get louder) x increasing camera distances.
        let cams = [
            1.0, 5.0, 10.0, 20.0, 30.0, 40.0, 41.0, 60.0, 100.0, 250.0, 1000.0,
        ];
        for dx in (-200..=200).step_by(10) {
            for dz in (-200..=200).step_by(10) {
                let src = [100.0 + dx as f32, 0.0, 100.0 + dz as f32];
                for w in cams.windows(2) {
                    let g1 = mix(&at(w[0]), src, &P).gain;
                    let g2 = mix(&at(w[1]), src, &P).gain;
                    assert!(
                        g2 <= g1 + 1e-6,
                        "offset ({dx},{dz}): cam {} -> {}: {g1} -> {g2}",
                        w[0],
                        w[1]
                    );
                }
            }
        }
        // The regression case: 60 m off-centre.
        let src = [160.0, 0.0, 100.0];
        assert!(mix(&at(40.0), src, &P).gain <= mix(&at(20.0), src, &P).gain);
    }

    #[test]
    fn audible_range_does_not_widen_when_zooming_out() {
        // 140 m away is beyond the max radius: silent at every zoom level.
        let src = [240.0, 0.0, 100.0];
        for cam in [10.0, 100.0, 1000.0] {
            assert!(!mix(&at(cam), src, &P).is_audible(&P), "cam {cam}");
        }
    }

    #[test]
    fn pan_follows_the_right_vector() {
        let l = at(30.0); // pan width 40
        let right = mix(&l, [120.0, 0.0, 100.0], &P).pan;
        let left = mix(&l, [80.0, 0.0, 100.0], &P).pan;
        assert!(near(right, 0.5 * 0.9), "{right}");
        assert!(near(left, -0.5 * 0.9), "{left}");
        // straight ahead / behind (along forward) stays centred
        assert!(near(mix(&l, [100.0, 0.0, 60.0], &P).pan, 0.0));
        assert!(near(mix(&l, [100.0, 0.0, 140.0], &P).pan, 0.0));
    }

    #[test]
    fn pan_is_clamped_to_max_pan() {
        let l = at(30.0);
        assert!(near(mix(&l, [1000.0, 0.0, 100.0], &P).pan, 0.9));
        assert!(near(mix(&l, [-1000.0, 0.0, 100.0], &P).pan, -0.9));
    }

    #[test]
    fn rotated_camera_rotates_the_stereo_image() {
        // Camera turned 90 degrees: right now points along +Z (world).
        let l = Listener::new([0.0; 3], [0.0, 0.0, 1.0], 30.0);
        assert!(mix(&l, [0.0, 0.0, 20.0], &P).pan > 0.3);
        assert!(near(mix(&l, [20.0, 0.0, 0.0], &P).pan, 0.0));
        // The right vector's length and vertical part do not matter.
        let l2 = Listener::new([0.0; 3], [0.0, 5.0, 3.0], 30.0);
        assert!(near(
            mix(&l2, [0.0, 0.0, 20.0], &P).pan,
            mix(&l, [0.0, 0.0, 20.0], &P).pan
        ));
    }

    #[test]
    fn pan_image_widens_when_zooming_out() {
        let src = [120.0, 0.0, 100.0];
        assert!(mix(&at(30.0), src, &P).pan > mix(&at(200.0), src, &P).pan);
    }

    #[test]
    fn degenerate_inputs_do_not_poison_the_mix() {
        let bad = Listener::new([f32::NAN; 3], [f32::NAN; 3], f32::INFINITY);
        let m = mix(&bad, [1.0, 2.0, 3.0], &P);
        assert!(m.gain.is_finite() && m.pan.is_finite(), "{m:?}");
        let zero_right = Listener::new([0.0; 3], [0.0; 3], 30.0);
        assert_eq!(mix(&zero_right, [5.0, 0.0, 5.0], &P).pan, 0.0);
        let nan_src = mix(&at(30.0), [f32::NAN, 0.0, f32::NAN], &P);
        assert!(nan_src.gain.is_finite() && nan_src.pan.is_finite());
    }

    #[test]
    fn listener_from_eye_and_focus() {
        // Camera south of the focus looking north (-Z), tilted down.
        let l = Listener::from_eye_focus([10.0, 30.0, 40.0], [10.0, 0.0, 0.0]);
        assert!(near(l.distance, 50.0));
        assert_eq!(l.focus, [10.0, 0.0, 0.0]);
        // Looking north, right is +X.
        assert!(l.right[0] > 0.0 && near(l.right[2], 0.0));
        // Straight down: falls back to +X.
        let d = Listener::from_eye_focus([0.0, 50.0, 0.0], [0.0, 0.0, 0.0]);
        assert_eq!(d.right, [1.0, 0.0, 0.0]);
    }
}
