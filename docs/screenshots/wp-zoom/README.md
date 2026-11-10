# Zoom work package: models at every zoom, the icons on top, a farther farthest zoom

Made with the real renderer (release build, software Vulkan llvmpipe under Xvfb, 1280 x 720, no audio device),
about 6 s after the start of the run, a screen grab of the window: the top bar of every picture shows the fps and the
sim tick (about 100 to 130 in all of them; the fps is the loaded container's, not a measurement). `before_main_*` come from
a build of `origin/main` (ac1fccf), the others from this branch. `--match` is the first map M1 (96 x 96 cells, 384 m;
the player's base is in the south-west and everything else is shroud), `--crowd --units N` is the crowd scene (cubes,
cylinders, cones in four colours), `--sandbox` is the sandbox with 59 units of three sides. The zoom is `--zoom z`
(0 = closest, 1 = the farthest).

| File | What it shows |
|---|---|
| `before_main_m1_zoom0.53.jpg` | M1 base on main at zoom 0.53: the building is half dissolved into its icon, dithered and see-through (the picture Tijmen's complaint was about) |
| `after_m1_zoom0.53.jpg` | The same view on this branch: the solid model, no icon yet (the icons start at zoom 0.85) |
| `m1_zoom0.10_close.jpg`, `m1_zoom0.35_start.jpg`, `m1_zoom0.70.jpg` | M1 base at close, the starting zoom and 0.70: models, the building is drawn 1.0 to 1.16 times its size |
| `m1_zoom0.93_icon_fading_in.jpg` | Zoom 0.93: most of the map is in view and the icon is a translucent overlay on the model (ramp about 0.9, drawn about 33 % opaque with the 60 % cap of fix round 1; the model shows through) |
| `m1_zoom1.00_farthest.jpg` | The farthest zoom: the whole 384 m map plus a quarter map of margin all round (the map is 420 px of the 1280), the icon is 14 px and drawn at its cap of 60 % opacity (fix round 1), so the model, drawn 1.6 times larger, shows through it |
| `before_main_m1_zoom1.00.jpg` | Main's farthest zoom: just the map, icons only |
| `crowd1000_zoom1.00_farthest.jpg`, `before_main_crowd1000_zoom1.00.jpg` | 1,000 units at the farthest zoom, this branch (1,000 models at the coarse levels plus 1,000 icons at the 60 % cap) against main (icons only) |
| `crowd1000_zoom0.90.jpg`, `crowd300_zoom0.93.jpg` | 1,000 and 300 units in the icon fade band: models under the translucent icons |
| `sandbox59_zoom1.00.jpg` | The sandbox's two sides in the middle of the map at the farthest zoom |

What the pictures say, honestly: the model of a structure stays readable to the end of the zoom range, but at the farthest
zoom a soldier is 1 to 2 px as a model (the map is 1.1 px per metre at 1280 x 720), so there the icon is what reads, and
the icon (14 px) is as big as a structure's model; since fix round 1 it is drawn at most 60 % opaque, so the model shows through. The model scale cap (1.6) is deliberately modest: more would
make neighbouring structures overlap.

`m1_zoom0.93_icon_fading_in.jpg`, `m1_zoom1.00_farthest.jpg` and `crowd1000_zoom1.00_farthest.jpg` were made again in fix round 1
(10-11-2026) with the icon opacity cap of 60 % (`zoom.icons_max_alpha`), same commands and window.

## Fix round 2 (10-11-2026): much farther, icons much later

Same build kind, window and machine; `--shots dir --shots-zooms ...` of the real renderer (the game's own picture, no
grab), at tick 100 to 110. The farthest zoom now shows the map plus a full map size of margin on every side
(`far_margin` 1.0), the icons come only from zoom 0.95 and at most 40 % opaque, the models grow up to 2.5 times.
"Icons off" is `--unit-display models`, the same picture as the K toggle off (`Blend::MODELS`).

| File | What it shows |
|---|---|
| `r2_m1_zoom1.00_farthest_icons_on.jpg` | M1 at the new farthest zoom: the 384 m map is about 225 px of the 1280 (a third of the view left of the sidebar), shroud black, the margin clear colour; the construction centre is drawn 2.5 times its size with its icon over it at 40 % |
| `r2_m1_zoom1.00_farthest_icons_off.jpg` | The same with the icons off: the model alone |
| `r2_m1_zoom0.90_no_icons_yet.jpg` | Zoom 0.90: no icon at all yet (the band starts at 0.95), the model at about 2.3 times |
| `r2_crowd1000_zoom1.00_farthest.jpg` | 1,000 crowd units at the new farthest zoom: the models with the 40 % icons over them, 55 fps in the top bar |
| `r2_sandbox59_zoom1.00_farthest.jpg` | The sandbox's 59 units at the new farthest zoom: the two sides read as coloured clusters; single soldiers are dots |

Honest reading: on M1 the farthest view is mostly margin, as accepted in the decision (bigger maps need it). A
structure is about 18 px drawn at the cap; vehicles a few pixels; infantry is a dot either way (no separate scale,
left for Tijmen to judge on his laptop).

## Fix round 3 (10-11-2026): structures at their own scale

Same commands as round 2. Structures are drawn at most 1.6 times (`structure_scale_max`), units 2.5 times.

| File | What it shows |
|---|---|
| `r3_m1_zoom1.00_farthest_icons_on.jpg` | M1 at the farthest zoom: the construction centre at 1.6 with its 40 % icon (round 2's `r2_m1_*` show it at 2.5) |
| `r3_m1_zoom1.00_farthest_icons_off.jpg` | The same with the icons off |
| `r3_sandbox59_zoom1.00_farthest.jpg` | The sandbox: the units of the two sides still at 2.5 |
