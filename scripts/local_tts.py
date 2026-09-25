#!/usr/bin/env python3
"""Локальный TTS: текст (stdin) → WAV (stdout).

Поддерживает голоса Piper (*.onnx). Путь — `--model` (файл .onnx или каталог
с *.onnx, включая вложенные ru/ru_RU/.../voice.onnx).

Usage:
  echo "Привет" | python3 scripts/local_tts.py --model models/ru/.../voice.onnx
Exit codes: 1 — нет текста; 2 — ошибка инференса; 3 — не установлен piper.
"""
from __future__ import annotations

import argparse
import io
import os
import sys
import wave


def die(msg: str, code: int) -> None:
    print(msg, file=sys.stderr)
    sys.exit(code)


def find_onnx(path: str) -> str:
    if os.path.isfile(path) and path.endswith(".onnx"):
        return path
    if os.path.isdir(path):
        hits: list[str] = []
        for root, _dirs, files in os.walk(path):
            for name in sorted(files):
                if name.endswith(".onnx") and not name.endswith(".onnx.data"):
                    # .onnx.json — конфиг, пропускаем; веса — .onnx
                    if name.endswith(".onnx.json"):
                        continue
                    hits.append(os.path.join(root, name))
        if len(hits) == 1:
            return hits[0]
        if hits:
            # Берём первый (обычно единственный голос в выкачанном каталоге).
            return hits[0]
    die(f"TTS: не найден .onnx голос по пути {path}", 2)


def main() -> None:
    parser = argparse.ArgumentParser(description="Local TTS (Piper)")
    parser.add_argument("--model", required=True, help=".onnx voice path or dir")
    parser.add_argument("--voice", default=None, help="reserved (Piper single voice)")
    args = parser.parse_args()

    text = sys.stdin.read().strip()
    if not text:
        die("ERROR: no text on stdin", 1)

    onnx = find_onnx(args.model)
    try:
        from piper import PiperVoice
    except ImportError:
        die(
            "для Piper нужен piper-tts: pip install -r scripts/requirements-voice.txt",
            3,
        )

    try:
        voice = PiperVoice.load(onnx)
    except Exception as e:  # noqa: BLE001
        die(f"piper: load failed: {e}", 2)

    buf = io.BytesIO()
    try:
        with wave.open(buf, "wb") as wf:
            # piper >= 1.3: synthesize_wav; старые API — synthesize
            if hasattr(voice, "synthesize_wav"):
                voice.synthesize_wav(text, wf)
            else:
                for chunk in voice.synthesize(text):
                    # stream API
                    pass
                # Fallback через PCM → wave
                if hasattr(voice, "synthesize_stream_raw"):
                    import numpy as np

                    raw = b"".join(voice.synthesize_stream_raw(text))
                    wf.setnchannels(1)
                    wf.setsampwidth(2)
                    wf.setframerate(getattr(voice, "config", None).sample_rate
                                    if getattr(voice, "config", None)
                                    else 22050)
                    wf.writeframes(raw)
                else:
                    die("piper: неподдерживаемый API synthesize", 2)
    except Exception as e:  # noqa: BLE001
        die(f"piper: synthesize failed: {e}", 2)

    sys.stdout.buffer.write(buf.getvalue())


if __name__ == "__main__":
    main()
