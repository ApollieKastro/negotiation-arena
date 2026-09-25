#!/usr/bin/env python3
"""Локальный STT: аудио (stdin) → текст (stdout).

Выбор бэкенда по `--model` (файл/каталог модели в MODELS_DIR):
  1. transformers (каталог с config.json / nemotron / asr / parakeet)
  2. nemo-speech CLI (.nemo / nemotron .gguf, если есть в PATH)
  3. faster-whisper (whisper / ggml / ct2)

Usage:
  cat audio.wav | python3 scripts/local_stt.py --model models/... [--language ru-RU]
Exit codes: 1 — нет аудио; 2 — неизвестный формат; 3 — не установлены deps.
"""
from __future__ import annotations

import argparse
import os
import subprocess
import sys
import tempfile


def die(msg: str, code: int) -> None:
    print(msg, file=sys.stderr)
    sys.exit(code)


def write_temp_audio(audio: bytes) -> str:
    if audio[:4] == b"RIFF":
        ext = ".wav"
    elif audio[:4] == b"fLaC":
        ext = ".flac"
    elif audio[:4] == b"\x1a\x45\xdf\xa3":
        ext = ".webm"
    elif audio[:4] == b"OggS":
        ext = ".ogg"
    elif audio[:4] == b"FORM":
        ext = ".aiff"
    else:
        ext = ".wav"
    fd, path = tempfile.mkstemp(suffix=ext)
    with os.fdopen(fd, "wb") as f:
        f.write(audio)
    return path


def has_config(dir_path: str) -> bool:
    return os.path.isfile(os.path.join(dir_path, "config.json"))


def looks_like_transformers_dir(path: str) -> bool:
    if os.path.isdir(path) and has_config(path):
        return True
    # Каталог рядом с весом (config.json на уровень выше).
    parent = os.path.dirname(path)
    return bool(parent) and has_config(parent)


def stt_transformers(model_path: str, audio_path: str, language: str | None) -> str:
    try:
        import torch  # noqa: F401
        from transformers import AutoProcessor, pipeline
    except ImportError as e:
        die(
            "для этой модели нужны torch+transformers: "
            "pip install -r scripts/requirements-voice.txt",
            3,
        )

    # Каталог модели или родитель файла веса.
    model_dir = model_path if os.path.isdir(model_path) else os.path.dirname(model_path)
    if not model_dir or not has_config(model_dir):
        # Fallback: HF repo id из имени.
        base = os.path.basename(model_path.rstrip("/"))
        if "/" in model_path and os.path.isdir(model_path) is False:
            model_dir = model_path

    device = 0 if _cuda_available() else -1
    try:
        processor = AutoProcessor.from_pretrained(model_dir)
        # AutoModelForCTC / RNNT / seq2seq — pipeline сам выберет архитектуру.
        pipe = pipeline(
            "automatic-speech-recognition",
            model=model_dir,
            tokenizer=processor.tokenizer,
            feature_extractor=processor.feature_extractor,
            device=device,
        )
    except Exception as e:  # noqa: BLE001 — любая ошибка загрузки → понятный текст
        die(f"transformers: не удалось загрузить модель {model_dir}: {e}", 2)

    # Nemotron streaming ASR принимает task/prompt с языком.
    generate_kwargs = {}
    if language:
        lang = language.replace("_", "-")
        # multilingual nemotron: "Transcribe the following audio to <lang>..."
        generate_kwargs["task"] = "transcribe"
        if "nemotron" in model_dir.lower() or "asr" in model_dir.lower():
            # processor для nemotron может понимать target_lang
            generate_kwargs["target_lang"] = lang

    try:
        with open(audio_path, "rb") as f:
            audio_bytes = f.read()
        # Передаём path — pipeline сам декодирует через soundfile/ffmpeg.
        result = pipe(
            {"raw": audio_bytes, "sampling_rate": 16000}
            if False
            else audio_path,
            generate_kwargs=generate_kwargs or None,
            chunk_length_s=30,
            stride_length_s=5,
        )
    except Exception as e:  # noqa: BLE001
        # Повтор без generate_kwargs (некоторые модели их не принимают).
        try:
            result = pipe(audio_path, chunk_length_s=30)
        except Exception as e2:  # noqa: BLE001
            die(f"transformers: ошибка инференса: {e2} (первая попытка: {e})", 2)

    text = (result or {}).get("text") or ""
    return str(text).strip()


def stt_nemo_speech(model_path: str, audio_path: str, language: str | None) -> str:
    if not _which("nemo-speech"):
        die(
            "нужен nemo-speech CLI для .nemo/.gguf Nemotron: "
            "https://github.com/NVIDIA/NeMo-Skills  (или положите HF-каталог с config.json)",
            3,
        )
    cmd = ["nemo-speech", "transcribe", audio_path, "--model", model_path]
    if language:
        cmd += ["--language", language]
    try:
        out = subprocess.run(
            cmd, capture_output=True, text=True, timeout=170, check=False
        )
    except (OSError, subprocess.TimeoutExpired) as e:
        die(f"nemo-speech: {e}", 2)
    if out.returncode != 0:
        err = (out.stderr or out.stdout or "").strip()
        die(f"nemo-speech failed: {err[:500]}", 2)
    # Первые непустые строки — текст.
    lines = [ln.strip() for ln in (out.stdout or "").splitlines() if ln.strip()]
    # Часто формат: "file: text" или просто text.
    texts = []
    for ln in lines:
        if ":" in ln and not ln.startswith("{"):
            # "audio.wav: hello" → hello
            _, _, rest = ln.partition(":")
            texts.append(rest.strip() or ln)
        else:
            texts.append(ln)
    return " ".join(t for t in texts if t).strip()


def stt_faster_whisper(model_path: str, audio_path: str, language: str | None) -> str:
    try:
        from faster_whisper import WhisperModel
    except ImportError as e:
        die(
            "для whisper нужен faster-whisper: "
            "pip install -r scripts/requirements-voice.txt",
            3,
        )

    # Путь к ct2/файлу или размер из имени (tiny/base/…).
    if os.path.isfile(model_path) or os.path.isdir(model_path):
        source: str | str = model_path  # type: ignore[assignment]
        size = "base"
    else:
        source = "base"
        for cand in ("tiny", "base", "small", "medium", "large-v3", "large-v2", "large"):
            if cand in model_path.lower():
                size = cand
                break

    lang = None
    if language:
        lang = language.split("-")[0].split("_")[0]
    else:
        # По умолчанию — русский домена приложения; модель multilingual.
        lang = "ru"

    try:
        model = WhisperModel(
            source,
            device="cuda" if _cuda_available() else "cpu",
            compute_type="float16" if _cuda_available() else "int8",
        )
        segments, _info = model.transcribe(audio_path, language=lang)
        return " ".join(seg.text.strip() for seg in segments).strip()
    except Exception as e:  # noqa: BLE001
        die(f"faster-whisper: {e}", 2)


def _cuda_available() -> bool:
    try:
        import torch

        return bool(torch.cuda.is_available())
    except Exception:  # noqa: BLE001
        return False


def _which(name: str) -> bool:
    for d in os.environ.get("PATH", "").split(os.pathsep):
        p = os.path.join(d, name)
        if os.path.isfile(p) and os.access(p, os.X_OK):
            return True
    return False


def choose_backend(model_path: str) -> str:
    low = model_path.lower()
    if model_path.endswith(".nemo"):
        return "nemo"
    # nemotron gguf — сначала nemo-speech, иначе transformers (если каталог рядом).
    if "nemotron" in low and low.endswith(".gguf"):
        return "nemo_or_tf"
    if "asr" in low or "parakeet" in low or "fastconformer" in low:
        return "tf_or_nemo"
    # CT2 (faster-whisper): каталог с model.bin. Проверяем раньше общего
    # HF-heuristic — у ct2 тоже есть config.json, иначе уходит в transformers.
    if os.path.isdir(model_path) and os.path.isfile(os.path.join(model_path, "model.bin")):
        return "faster_whisper"
    if looks_like_transformers_dir(model_path) and not low.endswith(
        (".ggml", ".bin", ".ct2")
    ):
        # каталог HF с config.json
        if os.path.isdir(model_path):
            return "transformers"
    if any(x in low for x in ("whisper", "ggml", "stt", "distil-whisper")):
        if low.endswith(".gguf") and ("asr" in low or "nemotron" in low):
            return "nemo_or_tf"
        return "faster_whisper"
    if os.path.isdir(model_path) and has_config(model_path):
        return "transformers"
    # .gguf без явных маркеров — пробуем transformers по каталогу, иначе fw.
    if low.endswith(".gguf"):
        return "nemo_or_tf"
    if low.endswith(".bin") or low.endswith(".ct2"):
        return "faster_whisper"
    return "unknown"


def main() -> None:
    parser = argparse.ArgumentParser(description="Local STT")
    parser.add_argument("--model", required=True, help="path to model file or dir")
    parser.add_argument("--language", default=None, help="e.g. ru-RU, en-US")
    args = parser.parse_args()

    audio = sys.stdin.buffer.read()
    if not audio:
        die("ERROR: no audio on stdin", 1)

    model_path = args.model
    if not os.path.exists(model_path):
        die(f"model path not found: {model_path}", 2)

    audio_path = write_temp_audio(audio)
    try:
        backend = choose_backend(model_path)
        if backend == "transformers":
            text = stt_transformers(model_path, audio_path, args.language)
        elif backend == "nemo":
            text = stt_nemo_speech(model_path, audio_path, args.language)
        elif backend == "nemo_or_tf":
            text = None  # type: ignore[assignment]
            if _which("nemo-speech"):
                text = stt_nemo_speech(model_path, audio_path, args.language)
            else:
                # transformers: родитель каталога модели
                base = model_path
                if os.path.isfile(base):
                    parent = os.path.dirname(base)
                    if parent and has_config(parent):
                        base = parent
                    elif has_config(os.path.dirname(parent) or ""):
                        base = os.path.dirname(parent)
                if os.path.isdir(base) and has_config(base):
                    text = stt_transformers(base, audio_path, args.language)
                else:
                    die(
                        "нужен nemo-speech для GGUF Nemotron либо HF-каталог с config.json "
                        "(скачайте repo целиком: без --filename). "
                        "Также помогает: pip install -r scripts/requirements-voice.txt",
                        3,
                    )
        elif backend == "tf_or_nemo":
            if os.path.isdir(model_path) and has_config(model_path):
                text = stt_transformers(model_path, audio_path, args.language)
            elif _which("nemo-speech"):
                text = stt_nemo_speech(model_path, audio_path, args.language)
            else:
                die("нужны transformers (HF-каталог) или nemo-speech", 3)
        elif backend == "faster_whisper":
            text = stt_faster_whisper(model_path, audio_path, args.language)
        else:
            die(
                f"unknown STT model format: {model_path}; "
                "ожидается whisper/ggml, Nemotron ASR, HF-каталог с config.json",
                2,
            )
        if text is None:
            die("STT: пустой результат", 2)
        sys.stdout.write(text)
    finally:
        try:
            os.unlink(audio_path)
        except OSError:
            pass


if __name__ == "__main__":
    main()
