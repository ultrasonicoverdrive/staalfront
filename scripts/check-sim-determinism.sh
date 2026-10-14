#!/usr/bin/env bash
# Determinism lint for sf_sim (PLAN.md section 5, rules 2 and 3).
# Fails if a forbidden token or a float literal appears anywhere under
# crates/sf_sim/src, including comments and strings (keep the code free of
# them on purpose). Complements the clippy float lints in Cargo.toml.
set -euo pipefail
cd "$(dirname "$0")/.."

dir=crates/sf_sim/src
# Lookbehind keeps ranges (0..5) and tuple fields (x.0, t.0.1) passing.
patterns=(
    '\b(f32|f64|HashMap|HashSet)\b'                          # forbidden types
    '[0-9]_?f(32|64)\b'                                      # suffixed literal: 1f32
    '(?<![\w.])[0-9][0-9_]*\.[0-9]'                          # 2.5
    '(?<![\w.])[0-9][0-9_]*\.(?![.\w])'                      # 2.
    '(?<![\w.])[0-9][0-9_]*(\.[0-9_]+)?[eE][+-]?[0-9]'       # 1e5, 2.5E-3
)

fail=0
for p in "${patterns[@]}"; do
    if hits=$(grep -rnP --include='*.rs' -e "$p" "$dir"); then
        [ $fail -eq 0 ] && echo "determinism lint FAILED in $dir:" >&2
        echo "$hits" >&2
        fail=1
    fi
done
if [ $fail -ne 0 ]; then
    echo "Use sf_math fixed-point types and BTreeMap/BTreeSet or sorted Vec." >&2
    exit 1
fi
echo "determinism lint ok"
