#!/usr/bin/env python3
"""
TTS script using Piper (RHVoice-compatible quality).
Reads text from stdin, writes WAV audio to stdout.

Usage: echo "Текст" | python3 tts.py
"""
import sys
import wave
import io
import os

MODEL_PATH = os.environ.get(
    "TTS_MODEL_PATH",
    os.path.join(os.path.dirname(__file__), "..", "models", "ru", "ru_RU", "irina", "medium", "ru_RU-irina-medium.onnx"),
)


def tts_init():
    """Initialize Piper voice model (lazy loading)."""
    from piper import PiperVoice
    return PiperVoice.load(MODEL_PATH)


def tts_synthesize(voice, text: str) -> bytes:
    """Synthesize text to WAV bytes."""
    buf = io.BytesIO()
    with wave.open(buf, "wb") as wf:
        voice.synthesize_wav(text, wf)
    return buf.getvalue()


def main():
    text = sys.stdin.read().strip()
    if not text:
        print("ERROR: No text provided", file=sys.stderr)
        sys.exit(1)

    voice = tts_init()
    wav_bytes = tts_synthesize(voice, text)

    # Write WAV to stdout (binary)
    sys.stdout.buffer.write(wav_bytes)


if __name__ == "__main__":
    main()
