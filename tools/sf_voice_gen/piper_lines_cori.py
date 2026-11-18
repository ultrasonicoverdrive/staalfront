"""Render the raw Veyran unit-voice lines of Staalfront with Piper (WP11.2d). Run by hand, offline; CI never runs it.

The WAVs it writes are committed in tools/sf_voice_gen/voice_src/ (with the prefix `cori_`) and are the pinned input of
sf_voice_gen (`cargo run -p sf_voice_gen` filters them into assets/audio/voice/). Re-render only to change a line or
the voice; then run sf_voice_gen and update the pins in tools/sf_voice_gen/tests/generator.rs.

Made with: piper-tts 1.8.0 (GPL-3.0-or-later; a tool only, not part of the game or the repository), onnxruntime 1.32.0,
the voice en_GB-cori-high from https://huggingface.co/rhasspy/piper-voices at commit
e9ab671f2a0e50a701b34361136ba3e6a3a9eb0c, path en/en_GB/cori/high (British female, dataset: LibriVox recordings, public
domain, see voice_src/MODEL_CARD_cori.md). Default synthesis settings (`SynthesisConfig()` written out). The voice was
chosen by Tijmen by ear from cori and norman (16-11-2026: "Ja, cori").

    pip install piper-tts==1.8.0
    python piper_lines_cori.py <folder with en_GB-cori-high.onnx and .onnx.json> <output folder>

The output files are named `cori_<key>.wav`; the keys below, with the prefix, are the file names of sf_voice_gen's
VEYRAN_LINES. The text keeps its punctuation ("Standing by!", "Orders, sir?"): it shapes the intonation.
"""
import sys
import wave
from pathlib import Path

from piper import PiperVoice, SynthesisConfig

# Our own words, written for this project.
LINES = {
    "unit_select_ready_to_go": "Ready to go.",
    "unit_select_systems_online": "Systems online.",
    "unit_select_standing_by": "Standing by!",
    "unit_select_go_ahead": "Go ahead.",
    "unit_select_listening_in": "Listening in.",
    "unit_select_orders_sir": "Orders, sir?",
    "unit_select_copy_sir": "Copy, sir.",
    "unit_select_your_call_sir": "Your call, sir.",
    "unit_select_on_the_line": "On the line!",
    "unit_move_moving_out": "Moving out.",
    "unit_move_route_set": "Route set.",
    "unit_move_on_my_way": "On my way.",
    "unit_attack_opening_fire": "Opening fire.",
    "unit_attack_target_locked": "Target locked.",
    "unit_attack_engaging_now": "Engaging now.",
}

voices, out = Path(sys.argv[1]), Path(sys.argv[2])
voice = PiperVoice.load(str(voices / "en_GB-cori-high.onnx"))
cfg = SynthesisConfig()
out.mkdir(parents=True, exist_ok=True)
for key, text in LINES.items():
    with wave.open(str(out / f"cori_{key}.wav"), "wb") as wf:
        voice.synthesize_wav(text, wf, syn_config=cfg)
print(len(LINES), "lines")
