#!/usr/bin/env bash
# Determinism lint for sf_sim (PLAN.md section 5, rules 2 and 3).
# Fails if a forbidden token appears anywhere under crates/sf_sim/src,
# including comments and strings (keep the code free of them on purpose).
set -euo pipefail
cd "$(dirname "$0")/.."

dir=crates/sf_sim/src
pattern='\b(f32|f64|HashMap|HashSet)\b'

if hits=$(grep -rnE --include='*.rs' "$pattern" "$dir"); then
    echo "determinism lint FAILED: forbidden tokens in $dir" >&2
    echo "$hits" >&2
    echo "Use sf_math fixed-point types and BTreeMap/BTreeSet or sorted Vec." >&2
    exit 1
fi
echo "determinism lint ok"
