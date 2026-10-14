#!/usr/bin/env bash
# Determinism lint for sf_sim and sf_math (PLAN.md section 5, rules 2 and 3).
# Fails if a forbidden token or a float literal appears anywhere under
# crates/sf_sim/src or crates/sf_math/src, including comments and strings
# (keep the code free of them on purpose). Complements the clippy float lints
# in the crates' Cargo.toml.
#
# sf_math is shared with rendering, so it has exactly one exempt file,
# crates/sf_math/src/render_bridge.rs (f32 conversion at the rendering
# boundary). sf_sim may not mention that module at all, and may not enable the
# sf_math `render` cargo feature that compiles it (checked below). Tests that
# need floats to judge sf_math (crates/sf_math/tests) are outside the scanned
# src.
set -euo pipefail
cd "$(dirname "$0")/.."

# Lookbehind keeps ranges (0..5) and tuple fields (x.0, t.0.1) passing.
patterns=(
    '\b(f32|f64|HashMap|HashSet)\b'                          # forbidden types
    '[0-9]_?f(32|64)\b'                                      # suffixed literal: 1f32
    '(?<![\w.])[0-9][0-9_]*\.[0-9]'                          # 2.5
    '(?<![\w.])[0-9][0-9_]*\.(?![.\w])'                      # 2.
    '(?<![\w.])[0-9][0-9_]*(\.[0-9_]+)?[eE][+-]?[0-9]'       # 1e5, 2.5E-3
    '_f(32|64)\b'                                            # float helpers: to_f32, from_f64
)
# Only forbidden in sf_sim: the float bridge of sf_math.
sim_only_patterns=(
    '\brender_bridge\b'
)

fail=0

# scan DIR EXEMPT_FILE PATTERN...  (EXEMPT_FILE may be empty)
scan() {
    local dir=$1 exempt=$2 p hits
    shift 2
    for p in "$@"; do
        hits=$(grep -rnP --include='*.rs' -e "$p" "$dir" || true)
        if [ -n "$exempt" ] && [ -n "$hits" ]; then
            hits=$(printf '%s\n' "$hits" | grep -v -e "^$exempt:" || true)
        fi
        if [ -n "$hits" ]; then
            [ $fail -eq 0 ] && echo "determinism lint FAILED:" >&2
            echo "$hits" >&2
            fail=1
        fi
    done
}

scan crates/sf_sim/src "" "${patterns[@]}" "${sim_only_patterns[@]}"
scan crates/sf_math/src crates/sf_math/src/render_bridge.rs "${patterns[@]}"

# render_bridge only exists with sf_math's `render` cargo feature. sf_sim must
# never switch that feature on, or the compiler stops guarding the boundary.
hits=$(grep -nP '"render"' crates/sf_sim/Cargo.toml || true)
if [ -n "$hits" ]; then
    [ $fail -eq 0 ] && echo "determinism lint FAILED:" >&2
    echo "sf_sim must not enable the sf_math render feature (crates/sf_sim/Cargo.toml):" >&2
    echo "$hits" >&2
    fail=1
fi

if [ $fail -ne 0 ]; then
    echo "Use sf_math fixed-point types and BTreeMap/BTreeSet or sorted Vec." >&2
    exit 1
fi
echo "determinism lint ok"
