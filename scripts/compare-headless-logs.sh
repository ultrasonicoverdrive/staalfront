#!/usr/bin/env bash
# Compares the headless logs of two machines (PLAN.md WP1.3, milestone M0):
# the same scenarios must give the same checksums on Linux and on Windows.
#
#   scripts/compare-headless-logs.sh DIR_A DIR_B
#
# Both directories hold the JSON logs written by `sf_headless --out` (one per
# scenario, same file names). For every log, everything except the `host`
# block (machine, build and wall time, the only parts that may differ) must be
# identical: scenario, seed, map, tick counts, command counts, entity count,
# initial and final checksum, and the whole list of checksums. The checksum
# lists are also compared on their own, one "ticks checksum" line each, so a
# failure shows the first tick at which the two machines disagree.
#
# Exit status: 0 all equal, 1 a difference, a missing or empty log, 2 usage
# or missing tool. Needs bash, jq (preinstalled on GitHub's Ubuntu runners),
# diff and cmp.
set -euo pipefail

if [ $# -ne 2 ]; then
    echo "usage: $0 DIR_A DIR_B" >&2
    exit 2
fi
a=$1
b=$2
command -v jq >/dev/null || { echo "compare-headless-logs: jq is required" >&2; exit 2; }
for d in "$a" "$b"; do
    [ -d "$d" ] || { echo "compare-headless-logs: $d is not a directory (artifact missing?)" >&2; exit 1; }
done

names() {
    (cd "$1" && ls -1 -- *.json 2>/dev/null | sort) || true
}
names_a=$(names "$a")
names_b=$(names "$b")
if [ -z "$names_a" ]; then
    echo "compare-headless-logs: no logs in $a" >&2
    exit 1
fi
if [ "$names_a" != "$names_b" ]; then
    echo "compare-headless-logs: the two directories hold different logs:" >&2
    diff -u --label "$a" --label "$b" <(echo "$names_a") <(echo "$names_b") >&2 || true
    exit 1
fi

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
fail=0

for name in $names_a; do
    # A log without checksums, or whose last entry is not the final checksum,
    # would make an equality check worthless.
    for d in "$a" "$b"; do
        if ! jq -e '(.checksums | length) > 0 and (.final_checksum == (.checksums | last | .checksum))' \
            "$d/$name" >/dev/null; then
            echo "FAIL $name: $d/$name has no checksums or a final checksum that is not its last entry" >&2
            fail=1
            continue 2
        fi
    done

    jq 'del(.host)' "$a/$name" >"$tmp/a.json"
    jq 'del(.host)' "$b/$name" >"$tmp/b.json"
    jq -r '.checksums[] | "\(.ticks) \(.checksum)"' "$a/$name" >"$tmp/a.list"
    jq -r '.checksums[] | "\(.ticks) \(.checksum)"' "$b/$name" >"$tmp/b.list"

    if cmp -s "$tmp/a.list" "$tmp/b.list" && cmp -s "$tmp/a.json" "$tmp/b.json"; then
        count=$(wc -l <"$tmp/a.list" | tr -d ' ')
        final=$(jq -r '.final_checksum' "$a/$name")
        scenario=$(jq -r '"\(.scenario): \(.ticks) ticks, \(.entities) entities"' "$a/$name")
        echo "ok   $name: $scenario; $count checksums identical, final $final"
        for d in "$a" "$b"; do
            jq -r --arg d "$d" '"       \($d): \(.host.os)/\(.host.arch), \(.host.ticks_per_second) ticks/s"' "$d/$name"
        done
    else
        fail=1
        echo "FAIL $name: $a and $b disagree" >&2
        echo "--- checksum lists (first differences) ---" >&2
        diff -u --label "$a/$name" --label "$b/$name" "$tmp/a.list" "$tmp/b.list" | head -n 40 >&2 || true
        echo "--- everything except host (first differences) ---" >&2
        diff -u --label "$a/$name" --label "$b/$name" "$tmp/a.json" "$tmp/b.json" | head -n 40 >&2 || true
    fi
done

if [ $fail -ne 0 ]; then
    echo "headless logs differ: the simulation is not bit-identical between the two machines" >&2
    exit 1
fi
echo "headless logs identical"
