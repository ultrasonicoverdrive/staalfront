#!/usr/bin/env python3
"""Validate the Staalfront story content (stdlib only).

Mission documents (missions/F?-M??.md) are checked against CONVENTIONS.md and units.md:
sections, references, roles, objectives, win/lose triggers, and the speakers of the
MSG table (they must exist in cast.md; OPERATIONS is the system voice).

Briefings (briefings/F?-M??.fountain) are checked against cast.md and the Fountain
conventions in README.md:
  * every dialogue cue is a character of cast.md (OPERATIONS is the system voice);
  * every speaker is in the room per the action lines, unless the cue carries (V.O.)
    or (O.S.); a speaker who is on the radio must carry one of them;
  * a transition other than "... TO:" starts with ">" (for example "> FADE OUT.").
The whole of content/ is also scanned for names that belong to other games (B5):
the role names construction-yard and naval-yard (with an underscore) and the title of the game that inspired the genre are errors.

Usage (from anywhere; run it isolated):
    python3 -I content/scripts/validate.py                  # every campaign file
    python3 -I content/scripts/validate.py FILE [FILE ...]  # selected files

Without arguments the templates (_template.*) and the worked examples (EXAMPLE-*) are
skipped, because they are deliberately incomplete or use made-up characters. Pass them
by path to check them anyway.

Exit code 1 if any error is found.
"""
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
UNITS = ROOT / "units.md"
CAST = ROOT / "cast.md"
MISSIONS = ROOT / "missions"
BRIEFINGS = ROOT / "briefings"

SYSTEM_VOICE = "OPERATIONS"

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


def is_campaign_file(path):
    name = path.name
    return not (name.startswith("_") or name.startswith("EXAMPLE"))


def load_roles():
    roles = set()
    for line in UNITS.read_text(encoding="utf-8").splitlines():
        m = re.match(r"\|\s*([a-z_]+)\s*\|", line)
        if m and m.group(1) not in {"role", "after"}:
            roles.add(m.group(1))
    return roles


def load_cast():
    """Names of the characters in cast.md: the '## NAME' headings, without '(note)'."""
    cast = set()
    for line in CAST.read_text(encoding="utf-8").splitlines():
        m = re.match(r"##\s+(.+?)\s*$", line)
        if m:
            cast.add(re.sub(r"\s*\(.*?\)\s*$", "", m.group(1)).strip())
    cast.add(SYSTEM_VOICE)
    return cast


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


# --------------------------------------------------------------------------- missions

def validate_mission(path, roles, cast):
    errors, warnings = [], []
    text = path.read_text(encoding="utf-8")

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

    # Every MSG speaker must be a character of cast.md (OPERATIONS is the system voice)
    for r in table_rows(sec_b)[1:]:
        if len(r) >= 3 and r[0].startswith("MSG_") and r[1] not in cast:
            errors.append(f"{r[0]} speaker '{r[1]}' is not in cast.md")

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

    # Every mission has a briefing of the same code
    if not (BRIEFINGS / f"{path.stem}.fountain").exists():
        errors.append(f"briefings/{path.stem}.fountain is missing")

    return errors, warnings


# --------------------------------------------------------------------------- briefings

SCENE_HEADING = re.compile(r"^(INT|EXT|EST|INT\./EXT|INT/EXT|I/E)[. ]", re.I)
# Words that start a screenplay transition; "FADE IN:" and "FADE OUT." are the usual ones.
TRANSITION_WORDS = re.compile(
    r"^(FADE|CUT|DISSOLVE|WIPE|IRIS|SMASH|MATCH|JUMP|HARD CUT|THE END|BACK TO)\b")
RADIO_WORDS = re.compile(r"\b(voice|handset|radio|telephone|phone|intercom|loudspeaker|crackles?|through it|on the line)\b", re.I)
# A speaker counts as present only if an action sentence names them together with one
# of these verbs. A name alone is not enough: "Mira holds the handset. Rusk crackles
# through it." names Rusk, but not as being in the room, so his cue needs (V.O.).
# Limitation: the check is a word list, not a parser. A presence verb that is not in the
# list is reported as an error (add it here), and a sentence that uses a listed verb for
# someone on the radio ("Rusk waits on the line") would pass.
PRESENCE_VERBS = (
    "stand", "stands", "stood", "sit", "sits", "sat", "lean", "leans", "crouch", "crouches",
    "kneel", "kneels", "wait", "waits", "watch", "watches", "study", "studies", "read", "reads",
    "hold", "holds", "bend", "bends", "pace", "paces", "enter", "enters", "walk", "walks",
    "look", "looks", "work", "works", "rest", "rests", "lie", "lies", "face", "faces",
    "point", "points", "arrives", "arrive",
)
PRESENCE_RE = re.compile(r"\b(" + "|".join(PRESENCE_VERBS) + r")\b", re.I)
RADIO_PAREN =re.compile(r"\b(over (the )?(radio|phone|handset)|on (the )?radio|radio)\b", re.I)


def fountain_blocks(text):
    """Split a Fountain file into blank-line separated blocks (title page dropped)."""
    blocks, cur = [], []
    for line in text.splitlines():
        if line.strip() == "":
            if cur:
                blocks.append(cur)
                cur = []
        else:
            cur.append(line.rstrip())
    if cur:
        blocks.append(cur)
    # The title page is the first block when it consists of 'Key: value' lines.
    if blocks and all(re.match(r"^[A-Za-z ]+:", l) or l.startswith((" ", "\t")) for l in blocks[0]):
        blocks = blocks[1:]
    return blocks


def is_upper_line(s):
    return s == s.upper() and re.search(r"[A-Z]", s) is not None


def named_as_present(name, scene_action):
    """True if an action sentence names the speaker with a presence verb and does not
    describe a radio, handset or other channel."""
    surname = name.split()[-1]
    for sentence in re.split(r"(?<=[.!?])\s+", scene_action):
        if re.search(rf"\b({re.escape(name)}|{re.escape(surname)})\b", sentence, re.I) \
                and PRESENCE_RE.search(sentence) and not RADIO_WORDS.search(sentence):
            return True
    return False


def validate_briefing(path, cast):
    errors, warnings = [], []
    text = path.read_text(encoding="utf-8")
    blocks = fountain_blocks(text)

    scene_action = ""           # action text of the current scene so far
    first_cue_seen = set()
    for block in blocks:
        first = block[0].strip()
        # notes, forced transitions and centered text
        if first.startswith("[["):
            continue
        if first.startswith(">"):
            continue
        if SCENE_HEADING.match(first) or first.startswith("."):
            scene_action = ""
            continue
        single = len(block) == 1
        # Transition rule: "... TO:" is recognised by Fountain itself; every other
        # transition needs a leading ">".
        if single and is_upper_line(first):
            if first.endswith("TO:"):
                continue
            if TRANSITION_WORDS.match(first):
                errors.append(f"transition '{first}' needs a leading '>' (write '> {first}')")
                continue
        # Dialogue block: an upper-case cue followed by (parenthetical and) text.
        if not single and is_upper_line(first) and not first.startswith("("):
            m = re.match(r"^([^()^]+?)\s*((?:\([^)]*\)\s*)*)\^?$", first)
            name = (m.group(1) if m else first).strip()
            ext = (m.group(2) if m else "").strip()
            extension = re.findall(r"\(([^)]*)\)", ext)
            on_radio = any(e.strip().upper() in {"V.O.", "O.S."} for e in extension)
            if name not in cast:
                errors.append(f"cue '{name}' is not in cast.md")
                continue
            if name == SYSTEM_VOICE:
                continue
            parens = " ".join(l for l in block[1:] if l.strip().startswith("("))
            if not on_radio and RADIO_PAREN.search(parens):
                errors.append(f"{name} speaks over the radio ({parens.strip()}) but the cue lacks (V.O.) or (O.S.)")
            if not on_radio and not named_as_present(name, scene_action):
                errors.append(
                    f"{name} speaks without (V.O.)/(O.S.) but the action lines of the scene do not say that "
                    f"{name.split()[-1]} is in the room (a sentence naming them with a verb such as stand, sit, "
                    f"lean, crouch, wait, watch; or write the cue as '{name} (V.O.)' if on the radio)")
            first_cue_seen.add(name)
            continue
        # Everything else is action.
        scene_action += " " + " ".join(l.strip() for l in block)

    if not first_cue_seen:
        errors.append("no dialogue found")
    return errors, warnings


# --------------------------------------------------------------------------- forbidden terms

# Names that belong to another game (B5, clean-room). The patterns are written so that
# this file does not match itself.
FORBIDDEN = [
    (re.compile(r"construction[_ ]yard", re.I), "use construction_center / construction center"),
    (re.compile(r"naval[_ ]yard", re.I), "use shipyard"),
    (re.compile(r"red\s+alert", re.I), "do not name the other game; say \"existing games\""),
]
TEXT_SUFFIXES = {".md", ".fountain", ".csv", ".py", ".txt", ".json", ".ron"}


def check_forbidden_terms():
    errors = []
    for p in sorted(ROOT.rglob("*")):
        if not p.is_file() or p.suffix not in TEXT_SUFFIXES or "__pycache__" in p.parts:
            continue
        text = p.read_text(encoding="utf-8")
        for pattern, advice in FORBIDDEN:
            for m in pattern.finditer(text):
                line = text.count("\n", 0, m.start()) + 1
                errors.append(f"{p.relative_to(ROOT)}:{line}: forbidden term '{m.group(0)}' ({advice})")
    return errors


# --------------------------------------------------------------------------- main

def main(argv):
    roles = load_roles()
    cast = load_cast()
    if argv:
        paths = [Path(a) for a in argv]
    else:
        paths = [p for p in sorted(MISSIONS.glob("*.md")) if is_campaign_file(p)]
        paths += [p for p in sorted(BRIEFINGS.glob("*.fountain")) if is_campaign_file(p)]
    total_err = 0
    for p in paths:
        if p.suffix == ".fountain":
            errs, warns = validate_briefing(p, cast)
        else:
            errs, warns = validate_mission(p, roles, cast)
        total_err += len(errs)
        status = "OK " if not errs else "ERR"
        print(f"[{status}] {p.resolve().parent.name}/{p.name}: {len(errs)} errors, {len(warns)} warnings")
        for e in errs:
            print(f"    E: {e}")
        for w in warns:
            print(f"    W: {w}")
    term_errors = check_forbidden_terms()
    total_err += len(term_errors)
    print(f"[{'OK ' if not term_errors else 'ERR'}] content/: forbidden terms, {len(term_errors)} errors")
    for e in term_errors:
        print(f"    E: {e}")
    print(f"{len(paths)} files checked, {total_err} errors")
    return 1 if total_err else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
