#!/usr/bin/env python3
"""Export all in-mission messages (MSG_ tables) to voiceover.csv for recording."""
import csv
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
out = ROOT / "voiceover.csv"
rows = []
for p in sorted((ROOT / "missions").glob("*.md")):
    text = p.read_text(encoding="utf-8")
    i, j = text.find("## B. Story"), text.find("## C. Map")
    for line in text[i:j].splitlines():
        m = re.match(r"\|\s*(MSG_\d+)\s*\|\s*([^|]+?)\s*\|\s*\"?(.+?)\"?\s*\|\s*$", line)
        if m:
            rows.append([p.stem, m.group(1), m.group(2), m.group(3)])
with out.open("w", newline="", encoding="utf-8") as f:
    w = csv.writer(f)
    w.writerow(["mission", "code", "speaker", "line"])
    w.writerows(rows)
print(f"{len(rows)} lines -> {out.name}")
