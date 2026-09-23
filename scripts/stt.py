#!/usr/bin/env python3
"""
STT script using faster-whisper.
Reads audio bytes (WAV/WebM) from stdin, writes recognized text to stdout.

Usage: cat audio.webm | python3 stt.py
"""
import sys
import os
import tempfile

MODEL_SIZE = os.environ.get("STT_MODEL_SIZE", "tiny")


def stt_init():
    """Initialize Whisper model (lazy loading)."""
    from faster_whisper import WhisperModel
    return WhisperModel(MODEL_SIZE, device="cpu", compute_type="int8")


def stt_transcribe(model, audio_path: str) -> str:
    """Transcribe audio file to text."""
    segments, info = model.transcribe(audio_path, language="ru")
    texts = [seg.text.strip() for seg in segments]
    return " ".join(texts)


def main():
    audio_bytes = sys.stdin.buffer.read()
    if not audio_bytes:
        print("ERROR: No audio provided", file=sys.stderr)
        sys.exit(1)

    # Determine format from magic bytes
    if audio_bytes[:4] == b"RIFF":
        ext = ".wav"
    elif audio_bytes[:4] == b"fLaC":
        ext = ".flac"
    elif audio_bytes[:4] == b"\x1a\x45\xdf\xa3":
        ext = ".webm"
    else:
        ext = ".wav"

    # Write to temp file (Whisper needs a file path)
    with tempfile.NamedTemporaryFile(suffix=ext, delete=False) as tmp:
        tmp.write(audio_bytes)
        tmp_path = tmp.name

    try:
        model = stt_init()
        text = stt_transcribe(model, tmp_path)
        sys.stdout.write(text)
    finally:
        os.unlink(tmp_path)


if __name__ == "__main__":
    main()
