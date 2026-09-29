<#
.SYNOPSIS
    Установка голосовых зависимостей: piper-tts (TTS) + faster-whisper (STT).

.DESCRIPTION
    Python нужен только для локального озвучивания и распознавания речи
    (scripts/local_tts.py, scripts/local_stt.py). Без этих пакетов приложение
    работает полностью, но голосовые функции вернут 502.

    Используется install.ps1 и вручную:
        powershell -NoProfile -ExecutionPolicy Bypass -File scripts\setup-voice.ps1

    Логика:
      1. Быстрая проверка — если piper и faster-whisper уже есть, сразу OK.
      2. Минимальный набор (piper-tts, faster-whisper, soundfile) — критичен
         для TTS/STT, ~150 МБ.
      3. Полный requirements-voice.txt (torch/transformers для Nemotron ASR) —
         best-effort: сбой не фатален, whisper и piper работают и без него.

    Кодировка: UTF-8 с BOM (Windows PowerShell 5.1 читает .ps1 как ANSI без BOM).

.NOTES
    Не фатален: любая ошибка → предупреждение и выход с кодом 0,
    чтобы не ломать основную установку.
#>
[CmdletBinding()]
param()

$ErrorActionPreference = "Stop"

function Ok   { param($m) Write-Host "OK  $m" -ForegroundColor Green }
function Warn { param($m) Write-Host "!   $m" -ForegroundColor Yellow }
function Info { param($m) Write-Host "    $m" -ForegroundColor Gray }

# piper (TTS) + faster-whisper (STT) — то, без чего голос не работает.
$CoreCheck = "import piper, faster_whisper; print('core-ok')"

function Test-VoiceCore {
    try {
        $r = & python -c $CoreCheck 2>&1
        return ("$r" -like "*core-ok*")
    } catch { return $false }
}

function Install-Pkgs {
    param([string[]]$Pkgs, [string]$Label)
    Info "Installing $Label : $($Pkgs -join ', ')"
    & python -m pip install --disable-pip-version-check --quiet --retries 5 --timeout 60 @Pkgs 2>&1 |
        ForEach-Object { Info "$_" }
    return ($LASTEXITCODE -eq 0)
}

try {
    $py = Get-Command "python" -ErrorAction SilentlyContinue
    if (-not $py) {
        Warn "Python not found - voice (STT/TTS) unavailable. Core app works fine."
        Info "Install Python 3.10+ from https://python.org and re-run this script."
        exit 0
    }

    # Версия — отдельно, чтобы битый ярлык WindowsApps не ронял скрипт.
    $ver = & python --version 2>&1
    if ($LASTEXITCODE -ne 0 -or "$ver" -notlike "*Python 3*") {
        Warn "python is not a real Python install (WindowsApps stub?) - skipping voice setup."
        Info "Install Python 3.10+ from https://python.org (check 'Add to PATH')."
        exit 0
    }
    Ok "Python: $ver"

    # 1) Уже установлено? — выходим мгновенно.
    if (Test-VoiceCore) {
        Ok "Voice ready: TTS (piper) + STT (faster-whisper)"
        exit 0
    }

    # 2) Минимальный набор — без него голос не работает.
    if (-not (Install-Pkgs @("piper-tts", "faster-whisper", "soundfile") "core voice packages")) {
        Warn "Core voice packages failed to install (network?). Voice will return 502."
        Info "Retry: python -m pip install piper-tts faster-whisper soundfile"
        exit 0
    }

    # 3) Полный набор (torch/transformers) — нужен только для Nemotron ASR,
    #    большой (~2 ГБ). Сбой здесь не критичен.
    $root = Split-Path -Parent $PSScriptRoot   # корень репозитория
    $req  = Join-Path $root "scripts\requirements-voice.txt"
    if (Test-Path $req) {
        Info "Installing full voice extras from requirements-voice.txt (torch, ~2 GB, optional)..."
        & python -m pip install --disable-pip-version-check --quiet --retries 5 --timeout 60 -r $req 2>&1 |
            ForEach-Object { Info "$_" }
        if ($LASTEXITCODE -ne 0) {
            Warn "Optional extras failed - whisper/piper still work; Nemotron ASR unavailable."
        }
    }

    # 4) Итоговая проверка ядра.
    if (Test-VoiceCore) {
        Ok "Voice ready: TTS (piper) + STT (faster-whisper)"
    } else {
        Warn "Packages installed, but import check failed. Voice may not work."
        Info "Retry: python -m pip install piper-tts faster-whisper soundfile"
    }
    exit 0
}
catch {
    Warn "Voice setup failed: $($_.Exception.Message)"
    Info "Core app works without voice; retry: powershell -File scripts\setup-voice.ps1"
    exit 0
}
