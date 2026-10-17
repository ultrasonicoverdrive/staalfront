#!/usr/bin/env python3
"""Validate Staalfront mission documents against CONVENTIONS.md and units.md.

Usage: python scripts/validate.py [missions/F1-M03.md ...]
Exit code 1 if any error is found.
"""
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
UNITS = ROOT / "units.md"
MISSIONS = ROOT / "missions"

GENERIC_PHRASES = [
    "The situation has changed. Reassess the objective markers and proceed.",
    "Objective secured. Report for the next operation.",
    "mission-specific trigger",
    "where applicable",
    "as marked",
    "mission-specific objects",
    "Opposing faction",
]

SECTION_HEADINGS = [
    "## A. Core", "## B. Story", "## C. Map", "## D. Players", "## E. Starting setup",
    "## F. Variables and timers", "## G. Teams", "## H. Triggers",
    "## I. Objectives", "## J. Flow in words", "## K. After the mission", "## Notes",
]


def load_roles():
    roles = set()
    for line in UNITS.read_text(encoding="utf-8").splitlines():
        m = re.match(r"\|\s*([a-z_]+)\s*\|", line)
        if m and m.group(1) not in {"role", "after"}:
            roles.add(m.group(1))
    return roles


def section(text, start, end):
    i = text.find(start)
    if i < 0:
        return ""
    j = text.find(end, i + len(start)) if end else -1
    return text[i:j] if j >= 0 else text[i:]


def table_rows(block):
    rows = []
    for line in block.splitlines():
        if line.startswith("|") and not re.match(r"\|\s*-", line):
            cells = [c.strip() for c in line.strip().strip("|").split("|")]
            rows.append(cells)
    return rows


def validate(path, roles):
    errors, warnings = [], []
    text = path.read_text(encoding="utf-8")
    code = path.stem

    for h in SECTION_HEADINGS:
        if h not in text:
            errors.append(f"missing section heading '{h}'")
    for p in GENERIC_PHRASES:
        if p in text:
            errors.append(f"generic template phrase still present: '{p}'")

    # Defined identifiers
    defined = {k: set() for k in ("Z", "P", "OBJ", "T", "TR", "MSG", "timer", "var")}
    sec_c = section(text, "## C. Map", "## D.")
    defined["Z"] = set(re.findall(r"\b(Z_[A-Z0-9_]+)\b", sec_c))
    defined["P"] = set(re.findall(r"\b(P_[A-Z0-9_]+)\b", sec_c))
    sec_e = section(text, "## E. Starting setup", "## F.")
    defined["OBJ"] = set(re.findall(r"`?(OBJ_[A-Z0-9_]+)`?", sec_e))
    sec_f = section(text, "## F. Variables", "## G.")
    for r in table_rows(sec_f)[1:]:
        if r:
            name = r[0].strip("`")
            (defined["timer"] if name.startswith("timer") else defined["var"]).add(name)
    sec_g = section(text, "## G. Teams", "## H.")
    defined["T"] = {r[0].strip("`") for r in table_rows(sec_g)[1:] if r and r[0].startswith("T_")}
    sec_h = section(text, "## H. Triggers", "## I.")
    defined["TR"] = {r[0].strip("`") for r in table_rows(sec_h)[1:] if r and r[0].startswith("TR_")}
    sec_b = section(text, "## B. Story", "## C.")
    defined["MSG"] = {r[0] for r in table_rows(sec_b)[1:] if r and r[0].startswith("MSG_")}

    # References in triggers and teams
    ref_text = sec_g + sec_h
    for prefix, key in (("Z_", "Z"), ("P_", "P"), ("OBJ_", "OBJ"), ("T_", "T"), ("TR_", "TR"), ("MSG_", "MSG")):
        for ref in set(re.findall(rf"\b({prefix}[A-Z0-9_]+)\b", ref_text)):
            if ref not in defined[key]:
                errors.append(f"{ref} referenced in G/H but not defined")
    for ref in set(re.findall(r"\b(timer_[a-z0-9_]+)\b", ref_text)):
        if ref not in defined["timer"]:
            errors.append(f"{ref} referenced in G/H but not defined in F")
    # Messages defined but never played
    for msg in defined["MSG"]:
        if msg not in sec_h:
            warnings.append(f"{msg} defined but never used in a trigger")
    # Objects defined but never referenced in triggers
    for obj in defined["OBJ"]:
        if obj not in ref_text:
            warnings.append(f"{obj} defined but never referenced in G/H")

    # Roles
    for blk in (sec_e, sec_g):
        for r in table_rows(blk)[1:]:
            cell = r[1] if blk is sec_e and len(r) > 1 else (r[2] if len(r) > 2 else "")
            for role in re.findall(r"\b([a-z][a-z_]+)\b(?=\s*(?:×|x|\(|\s*\|))", cell):
                if role not in roles and role not in {"none", "no", "yes"}:
                    errors.append(f"unknown role '{role}' in E/G")

    # Objectives: each required objective completed by a defined trigger
    sec_i = section(text, "## I. Objectives", "## J.")
    for r in table_rows(sec_i)[1:]:
        if len(r) >= 4 and r[2].lower().startswith("yes"):
            trs = re.findall(r"TR_\d+", r[3])
            if not trs:
                errors.append(f"required objective {r[0]} has no completing trigger")
            for tr in trs:
                if tr not in defined["TR"]:
                    errors.append(f"objective {r[0]} points to undefined {tr}")
    if "**Win**" not in sec_i or "**Lose**" not in sec_i:
        errors.append("Win/Lose statements missing in section I")
    if not re.search(r"\|\s*TR_\d+\s*\|[^\n]*\bwin\b", sec_h, re.I):
        errors.append("no trigger with a win action")
    if not re.search(r"\|\s*TR_\d+\s*\|[^\n]*\blose\b", sec_h, re.I):
        errors.append("no trigger with a lose action")

    return errors, warnings


def main(argv):
    roles = load_roles()
    paths = [Path(a) for a in argv] or sorted(MISSIONS.glob("*.md"))
    total_err = 0
    for p in paths:
        errs, warns = validate(p, roles)
        total_err += len(errs)
        status = "OK " if not errs else "ERR"
        print(f"[{status}] {p.name}: {len(errs)} errors, {len(warns)} warnings")
        for e in errs:
            print(f"    E: {e}")
        for w in warns:
            print(f"    W: {w}")
    return 1 if total_err else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
