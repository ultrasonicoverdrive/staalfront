"""Render the raw unit-voice lines of Staalfront with Piper (WP11.2b). Run by hand, offline; CI never runs it.

The WAVs it writes are committed in tools/sf_voice_gen/voice_src/ and are the pinned input of sf_voice_gen
(`cargo run -p sf_voice_gen` filters them into assets/audio/voice/). Re-render only to change a line or the voice;
then run sf_voice_gen and update the pins in tools/sf_voice_gen/tests/generator.rs.

Made with: piper-tts 1.8.0 (GPL-3.0-or-later; a tool only, not part of the game or the repository),
onnxruntime 1.32.0, the voice en_US-joe-medium from https://huggingface.co/rhasspy/piper-voices at commit
e9ab671f2a0e50a701b34361136ba3e6a3a9eb0c, path en/en_US/joe/medium (dataset CC0, see voice_src/MODEL_CARD.md).
Default synthesis settings. The voice was chosen by Tijmen by ear from joe and norman (15-11-2026).

    pip install piper-tts==1.8.0
    python piper_lines.py <folder with en_US-joe-medium.onnx and .onnx.json> <output folder>
"""
import sys
import wave
from pathlib import Path

from piper import PiperVoice

# Our own words, written for this project; the file names are those of sf_voice_gen's LINES.
LINES = {
    "unit_select_ready": "Ready.",
    "unit_select_online": "Online.",
    "unit_select_standing_by": "Standing by.",
    "unit_move_moving": "Moving.",
    "unit_move_route_set": "Route set.",
    "unit_move_on_it": "On it.",
    "unit_attack_engaging": "Engaging.",
    "unit_attack_target_locked": "Target locked.",
    "unit_attack_firing": "Firing.",
}

voices, out = Path(sys.argv[1]), Path(sys.argv[2])
voice = PiperVoice.load(str(voices / "en_US-joe-medium.onnx"))
out.mkdir(parents=True, exist_ok=True)
for key, text in LINES.items():
    with wave.open(str(out / f"{key}.wav"), "wb") as wf:
        voice.synthesize_wav(text, wf)
print(len(LINES), "lines")
