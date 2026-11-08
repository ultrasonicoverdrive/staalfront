# WP7.5 screenshots: the queue beside the approach

Through the real renderer on Mesa lavapipe (software Vulkan), with the roster's shapes and the cargo
bars, from `crates/sf_game/tests/harvest_screenshot.rs`
(`queued_harvesters_stand_beside_the_approach_in_the_picture`, written with `SF_SAVE_SHOTS=<dir>`). The
tick is in the file name; the test waits until three harvesters are queued at the refinery and then lets
the queue settle for 120 ticks. Seven harvesters of one player on one refinery (the red block on the
left); its dock point is the middle of the cell just east of it, and the approach is the row east of that
point.

| File | Tick | Shows |
|---|---|---|
| `queue_t01072_beside_the_approach.png` | 1072 | The shipped rules: one harvester in the bay (empty bar, at the refinery), the queued ones stand to the sides of the row that leads to the bay (measured from the simulation: 1.3 to 4.4 cells off the line of the approach), and the row in front of the bay is free. |
| `queue_t01179_in_the_approach_old_rule.png` | 1179 | The same scene with `queue_gap: 0` (the rule of WP7.2): the queued harvesters stand where they were told to wait, a cluster in the approach (one measured 0.3 cells from its line). |

The camera is a close-up of the refinery looking down and across the map, so the approach runs to the
right of the bay. The numbers in the table are read from the world by the test, not from the pixels.
