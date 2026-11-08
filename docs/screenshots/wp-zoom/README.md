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
| `m1_zoom0.93_icon_fading_in.jpg` | Zoom 0.93: most of the map is in view and the icon is a translucent overlay on the model (about 55 % opaque; the model shows through) |
| `m1_zoom1.00_farthest.jpg` | The farthest zoom: the whole 384 m map plus a quarter map of margin all round (the map is 420 px of the 1280), the icon is opaque and 14 px, the model under it is drawn 1.6 times larger |
| `before_main_m1_zoom1.00.jpg` | Main's farthest zoom: just the map, icons only |
| `crowd1000_zoom1.00_farthest.jpg`, `before_main_crowd1000_zoom1.00.jpg` | 1,000 units at the farthest zoom, this branch (1,000 models at the coarse levels plus 1,000 icons) against main (icons only) |
| `crowd1000_zoom0.90.jpg`, `crowd300_zoom0.93.jpg` | 1,000 and 300 units in the icon fade band: models under the translucent icons |
| `sandbox59_zoom1.00.jpg` | The sandbox's two sides in the middle of the map at the farthest zoom |

What the pictures say, honestly: the model of a structure stays readable to the end of the zoom range, but at the farthest
zoom a soldier is 1 to 2 px as a model (the map is 1.1 px per metre at 1280 x 720), so there the icon is what reads, and
the icon (14 px) is as big as a structure's model and covers it. The model scale cap (1.6) is deliberately modest: more would
make neighbouring structures overlap.
