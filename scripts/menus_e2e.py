#!/usr/bin/env python3
"""WP11.5a: end-to-end check of the menus on a real window, with real X events (XTEST).

Needs Linux, Xvfb and the software Vulkan driver (Mesa lavapipe), like the screenshot runs of the CI;
no window manager. Build the game first (`cargo build -p sf_game`), then:

    python3 scripts/menus_e2e.py [--display :77] [--bin target/debug/sf_game]

It plays the match in a window it starts itself and says PASS or FAIL for each thing it saw in the
game's own log and in the settings file (kept in a temporary directory through STAALFRONT_SETTINGS,
never the real one):

* scenario 1, "playthrough": Esc opens the pause menu and the sim stops; keys, clicks and the sidebar
  under the menu do nothing (with controls that show the same keys and clicks do reach the game while
  no menu is open); Resume and Esc go on; Restart builds the match again with a young clock; Main
  menu holds the sim; Quit ends the program.
* scenario 2, "settings": a key is rebound in the settings screen (the new key works, the old one
  does not, the settings file has it), graphics and accessibility choices are written, and a second
  start with that file reads it back without a warning and the rebound key still works.

The coordinates are those of the menus at UI scale 1.0 in a 1280 x 720 window (`menu_look.ron`).
Exit code 0 when everything passed.
"""
import argparse
import ctypes
import os
import re
import subprocess
import sys
import tempfile
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
RESULTS = []

x11 = ctypes.CDLL("libX11.so.6")
xtst = ctypes.CDLL("libXtst.so.6")
x11.XOpenDisplay.restype = ctypes.c_void_p
x11.XOpenDisplay.argtypes = [ctypes.c_char_p]
x11.XStringToKeysym.restype = ctypes.c_ulong
x11.XKeysymToKeycode.restype = ctypes.c_ubyte
x11.XKeysymToKeycode.argtypes = [ctypes.c_void_p, ctypes.c_ulong]
x11.XFlush.argtypes = [ctypes.c_void_p]
x11.XDefaultRootWindow.restype = ctypes.c_ulong
x11.XDefaultRootWindow.argtypes = [ctypes.c_void_p]
x11.XQueryTree.argtypes = [
    ctypes.c_void_p,
    ctypes.c_ulong,
    ctypes.POINTER(ctypes.c_ulong),
    ctypes.POINTER(ctypes.c_ulong),
    ctypes.POINTER(ctypes.POINTER(ctypes.c_ulong)),
    ctypes.POINTER(ctypes.c_uint),
]
x11.XSetInputFocus.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_int, ctypes.c_ulong]
xtst.XTestFakeKeyEvent.argtypes = [ctypes.c_void_p, ctypes.c_uint, ctypes.c_int, ctypes.c_ulong]
xtst.XTestFakeButtonEvent.argtypes = [ctypes.c_void_p, ctypes.c_uint, ctypes.c_int, ctypes.c_ulong]
xtst.XTestFakeMotionEvent.argtypes = [
    ctypes.c_void_p,
    ctypes.c_int,
    ctypes.c_int,
    ctypes.c_int,
    ctypes.c_ulong,
]


def check(name, ok, detail=""):
    RESULTS.append((name, ok))
    print(("PASS " if ok else "FAIL ") + name + (f"  [{detail}]" if detail else ""), flush=True)


class Game:
    """The game in its own Xvfb, and the hands to play it with."""

    def __init__(self, display, binary, settings, args):
        self.display = display
        self.log_path = Path(tempfile.mkdtemp(prefix="menus_e2e_")) / "game.log"
        self.xvfb = subprocess.Popen(
            ["Xvfb", display, "-screen", "0", "1280x720x24"],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        time.sleep(2)
        env = dict(os.environ, DISPLAY=display, STAALFRONT_SETTINGS=str(settings))
        self.logf = open(self.log_path, "w")
        self.proc = subprocess.Popen(
            [str(binary)] + args, env=env, stdout=self.logf, stderr=subprocess.STDOUT, cwd=ROOT
        )
        self.d = None

    def text(self):
        return self.log_path.read_text(errors="replace")

    def mark(self):
        return len(self.text())

    def count(self, pattern, since=0):
        return len(re.findall(pattern, self.text()[since:]))

    def total_ticks(self):
        found = re.findall(r"\(total (\d+)\)", self.text())
        return int(found[-1]) if found else -1

    def totals(self):
        return [int(n) for n in re.findall(r"\(total (\d+)\)", self.text())]

    def wait_settled(self, timeout=30):
        """Waits until the sim has stopped: two reports that came after this call show the same
        total. Returns that total (the last one seen on a timeout, so the check after it fails
        instead of this call)."""
        seen = len(self.totals())
        end = time.time() + timeout
        while time.time() < end:
            t = self.totals()
            if len(t) >= seen + 2 and t[-1] == t[-2]:
                return t[-1]
            time.sleep(0.25)
        return self.total_ticks()

    def wait_ticks_above(self, ticks, timeout=30):
        """Waits until the sim has gone past `ticks`; says whether it did."""
        end = time.time() + timeout
        while time.time() < end:
            if self.total_ticks() > ticks:
                return True
            time.sleep(0.25)
        return False

    def wait_for(self, pattern, timeout):
        end = time.time() + timeout
        while time.time() < end:
            if re.search(pattern, self.text()):
                return True
            time.sleep(0.5)
        return False

    def barrier(self, timeout=20):
        """Waits until the main thread has handled every key pressed before this call. Ctrl+9
        (assign group 9) logs `group 9: ` from the frame that handles it; the first press can share
        a frame with earlier keys, so a second one is pressed only once the first line is there: its
        frame is then strictly later than theirs. Says whether both lines came."""
        for _ in range(2):
            m = self.mark()
            self.chord("Control_L", "9")
            end = time.time() + timeout
            while not re.search(r"group 9: ", self.text()[m:]):
                if time.time() > end:
                    return False
                time.sleep(0.1)
        return True

    def old_and_new_key(self, timeout=20):
        """Presses b (the old key of Choose structure), waits for a barrier that comes after b's
        frame, then presses n (the new one). Returns the number of `build: ` lines from b and from n,
        or (-1, -1) when the barrier did not come (so the check fails). n is given until its line
        comes and one second more, so a second line would be counted too."""
        m = self.mark()
        self.key("b")
        if not self.barrier():
            return -1, -1
        m2 = self.mark()
        self.key("n")
        end = time.time() + timeout
        while time.time() < end and not re.search(r"build: ", self.text()[m2:]):
            time.sleep(0.25)
        time.sleep(1.0)
        text = self.text()
        return len(re.findall(r"build: ", text[m:m2])), len(re.findall(r"build: ", text[m2:]))

    def hands(self):
        self.d = x11.XOpenDisplay(self.display.encode())
        assert self.d, "cannot open the display"
        # No window manager: the keyboard focus goes to the game's window by hand.
        root = x11.XDefaultRootWindow(self.d)
        r, p = ctypes.c_ulong(), ctypes.c_ulong()
        kids = ctypes.POINTER(ctypes.c_ulong)()
        n = ctypes.c_uint()
        x11.XQueryTree(self.d, root, ctypes.byref(r), ctypes.byref(p), ctypes.byref(kids), ctypes.byref(n))
        assert n.value > 0, "the game has no window"
        x11.XSetInputFocus(self.d, kids[n.value - 1], 2, 0)
        x11.XFlush(self.d)
        time.sleep(0.5)

    def _key(self, name, down):
        code = x11.XKeysymToKeycode(self.d, x11.XStringToKeysym(name.encode()))
        xtst.XTestFakeKeyEvent(self.d, code, 1 if down else 0, 0)
        x11.XFlush(self.d)

    def key(self, name):
        self._key(name, True)
        time.sleep(0.08)
        self._key(name, False)
        time.sleep(0.25)

    def chord(self, modifier, name):
        self._key(modifier, True)
        time.sleep(0.1)
        self.key(name)
        self._key(modifier, False)
        time.sleep(0.25)

    def move(self, x, y):
        xtst.XTestFakeMotionEvent(self.d, 0, x, y, 0)
        x11.XFlush(self.d)
        time.sleep(0.3)

    def click(self, x, y, button=1):
        self.move(x, y)
        xtst.XTestFakeButtonEvent(self.d, button, 1, 0)
        x11.XFlush(self.d)
        time.sleep(0.1)
        xtst.XTestFakeButtonEvent(self.d, button, 0, 0)
        x11.XFlush(self.d)
        time.sleep(0.4)

    def stop(self):
        if self.proc.poll() is None:
            self.proc.terminate()
            try:
                self.proc.wait(timeout=10)
            except subprocess.TimeoutExpired:
                self.proc.kill()
        self.xvfb.terminate()
        self.logf.close()


def playthrough(display, binary, settings):
    game = Game(display, binary, settings, ["--match"])
    try:
        check("the game started and runs", game.wait_for(r"\(total \d+\)", 120))
        game.hands()
        game.move(300, 300)
        time.sleep(3)

        # With no menu open a key and a click reach the game (the controls).
        m = game.mark()
        game.key("b")
        check("B reaches the game with no menu open", game.count(r"build: ", m) == 1)
        for _ in range(4):
            game.key("b")  # round the list of structures to none
        m = game.mark()
        game.click(1125, 163, 1)  # the first tile, under the Repair and Sell buttons of WP6.6 (24 px + 4 px gap)
        check("a click on the sidebar reaches it with no menu open", game.count(r"sidebar: build", m) >= 1)
        game.click(1125, 168, 3)  # the right button puts the structure away

        # Esc opens the pause menu and holds the sim.
        game.key("Escape")
        t1 = game.wait_settled()
        time.sleep(3.0)
        t2 = game.total_ticks()
        check("Esc holds the sim: no tick for 3 s", t1 == t2, f"total {t1} -> {t2}")

        # Under the menu nothing reaches the game.
        m = game.mark()
        game.key("b")
        game.key("c")
        game.key("x")
        game.chord("Control_L", "1")
        game.click(200, 200, 1)
        game.click(250, 250, 3)
        game.click(1125, 163, 1)  # the first tile of the sidebar
        game.click(1125, 168, 3)
        time.sleep(1.0)
        bad = re.findall(r"^(?:build:|group \d|order for tick|sidebar:).*$", game.text()[m:], re.M)
        check("keys, clicks and the sidebar under the menu do nothing", not bad, str(bad[:3]))
        check("and still no tick", game.total_ticks() == t2)

        # Resume is the first button of the pause menu.
        game.click(640, 290, 1)
        check("Resume lets the sim go on", game.wait_ticks_above(t2))
        m = game.mark()
        game.key("b")
        check("B reaches the game again after Resume", game.count(r"build: ", m) == 1)
        for _ in range(4):
            game.key("b")

        # Esc twice: open, close.
        game.key("Escape")
        held = game.wait_settled()
        time.sleep(2.0)
        check("held again", game.total_ticks() == held)
        game.key("Escape")
        check("Esc in the pause menu resumes", game.wait_ticks_above(held))

        # Restart is the second button.
        matches = game.count(r"match m1: ")
        before = game.total_ticks()
        game.key("Escape")
        game.wait_settled()  # the menu is open when the sim is held
        game.click(640, 336, 1)
        game.wait_for(r"(?s)(match m1: .*){%d}" % (matches + 1), 30)
        # The first report of the new game: its total is below the old one.
        end = time.time() + 30
        while time.time() < end and not 0 <= game.total_ticks() < before:
            time.sleep(0.25)
        after = game.total_ticks()
        check("Restart builds the match again", game.count(r"match m1: ") == matches + 1)
        check("and the new game's clock is young", 0 <= after < before, f"{before} -> {after}")

        # Main menu is the fourth button; the start menu's Quit the fourth.
        game.key("Escape")
        game.wait_settled()
        game.click(640, 428, 1)
        held = game.wait_settled()
        time.sleep(2.5)
        check("the main menu holds the sim", game.total_ticks() == held)
        game.click(640, 451, 1)
        ended = False
        for _ in range(20):
            if game.proc.poll() is not None:
                ended = True
                break
            time.sleep(0.5)
        check("Quit ends the program", ended and game.proc.returncode == 0, f"exit {game.proc.poll()}")
    finally:
        game.stop()


def settings_scenario(display, binary, settings):
    if settings.exists():
        settings.unlink()
    game = Game(display, binary, settings, ["--match"])
    try:
        check("the game started (no settings file yet)", game.wait_for(r"\(total \d+\)", 120))
        check("a missing settings file is no warning", "settings" not in game.text().split("window ")[0])
        game.hands()
        game.move(300, 300)
        time.sleep(3)

        # Esc, Settings (third button), the key list shows: rebind "Choose structure" (B) to N.
        game.key("Escape")
        game.click(640, 383, 1)
        game.click(872, 177, 1)  # the key button of Choose structure
        game.key("n")
        check("the settings file is written on the change", settings.exists())
        text = settings.read_text() if settings.exists() else ""
        check("it holds the new key and only that action", "BuildCycle" in text and "KeyN" in text and "OrderStop" not in text, text.replace("\n", " ")[:120])
        # A refused key: W is pan up. The wait goes on and nothing is written for it.
        game.click(872, 211, 1)  # Queue unit
        game.key("w")
        game.key("Escape")  # ends the wait
        text2 = settings.read_text()
        check("a key that another action has is refused", "ProduceQueue" not in text2)

        # Graphics: vertical sync off, frame cap 60; Accessibility: 150 %, colour-blind palette.
        # (The tabs stay at y 106 on every page; the first row of a page is at y 143, then 177, 211.)
        game.click(548, 106, 1)
        game.click(866, 211, 1)  # vertical sync: Off
        game.click(726, 177, 1)  # frame cap: 60 fps
        game.click(731, 106, 1)
        game.click(866, 177, 1)  # player colours: colour-blind safe
        game.click(835, 143, 1)  # UI scale: 150 % (the menu itself grows at once: last)
        text = settings.read_text()
        check("graphics and accessibility choices are written", all(s in text for s in ["vsync: false", "frame_cap: Some(60)", "ui_scale: 1.5", "palette: ColourBlind"]), text.replace("\n", " ")[:200])
        check("vertical sync off is applied to the surface", game.count(r"present: ") >= 1, str(re.findall(r"present: .*", game.text())[-1:]))

        # Back to the game; the new key works, the old one does not.
        before = game.total_ticks()
        game.key("Escape")
        game.key("Escape")
        check("the sim runs again after the menu", game.wait_ticks_above(before + 1))
        old, new = game.old_and_new_key()
        check("the new key works and the old one does not", old == 0 and new == 1, f"b: {old}, n: {new}")
    finally:
        game.stop()

    # A second start reads the file back.
    saved = settings.read_text()
    game = Game(display, binary, settings, ["--match"])
    try:
        check("the second start runs", game.wait_for(r"\(total \d+\)", 120))
        head = game.text().split("window ")[0]
        check("and reads the file without a warning", "settings.ron" not in head and "settings" not in head, head[-200:])
        game.hands()
        game.move(300, 300)
        check("the sim runs at the second start", game.wait_ticks_above(game.total_ticks() + 1))
        old, new = game.old_and_new_key()
        check("the rebound key is still there after a restart of the program", old == 0 and new == 1, f"b: {old}, n: {new}")
        check("the file is unchanged by a start", settings.read_text() == saved)
    finally:
        game.stop()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--display", default=":77")
    ap.add_argument(
        "--bin",
        default=str(Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "target")) / "debug" / "sf_game"),
    )
    ap.add_argument("--only", choices=["playthrough", "settings"])
    args = ap.parse_args()
    binary = Path(args.bin)
    scratch = Path(tempfile.mkdtemp(prefix="menus_e2e_settings_"))
    if args.only in (None, "playthrough"):
        playthrough(args.display, binary, scratch / "playthrough.ron")
    if args.only in (None, "settings"):
        settings_scenario(args.display, binary, scratch / "settings.ron")
    failed = [name for name, ok in RESULTS if not ok]
    print(f"{len(RESULTS) - len(failed)} of {len(RESULTS)} checks passed")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
