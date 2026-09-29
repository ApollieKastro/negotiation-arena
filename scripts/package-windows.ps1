<#
.SYNOPSIS
    Упаковка Windows-релиза: negotiation-arena.exe + интерфейс -> один zip.

.DESCRIPTION
    Собирает папку NegotiationArena (exe, dist, static, docs, scripts,
    START.bat, README-USTANOVKA.txt) и архивирует её.
    Используется локально и в .github/workflows/release.yml.

    Требуется собранный бинарь: cargo build --release

.EXAMPLE
    powershell -File scripts\package-windows.ps1
    powershell -File scripts\package-windows.ps1 -OutZip .\release.zip
#>
param(
    [string]$OutZip = ""
)

$ErrorActionPreference = "Stop"

$root    = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$exe     = Join-Path $root "target\release\negotiation-arena.exe"
$stage   = Join-Path $root "target\dist\NegotiationArena"
if (-not $OutZip) { $OutZip = Join-Path $root "negotiation-arena-windows-x64.zip" }

if (-not (Test-Path $exe)) {
    throw "Не найден $exe — сначала выполните: cargo build --release"
}

# --- staging ---------------------------------------------------------------
if (Test-Path $stage) { Remove-Item $stage -Recurse -Force }
New-Item -ItemType Directory -Force -Path $stage | Out-Null

Copy-Item $exe $stage
foreach ($d in @("dist", "static", "docs", "scripts")) {
    $src = Join-Path $root $d
    if (Test-Path $src) {
        Copy-Item $src -Destination $stage -Recurse
    } else {
        Write-Warning "нет папки $d — пропускаю (интерфейс dist обязателен!)"
    }
}
if (-not (Test-Path (Join-Path $stage "dist\index.html"))) {
    throw "dist\index.html не попал в пакет — без него интерфейс не запустится"
}

# --- лаунчер ---------------------------------------------------------------
# Только ASCII: cmd читает .bat в текущей кодовой странице консоли.
$bat = @(
    "@echo off",
    "cd /d `"%~dp0`"",
    "curl -s -o nul http://localhost:3001/health >nul 2>&1",
    "if errorlevel 1 (",
    "  echo Starting Negotiation Arena...",
    "  start `"Negotiation Arena`" cmd /k `"`"%~dp0negotiation-arena.exe`"`"",
    "  timeout /t 3 /nobreak >nul",
    ")",
    "start `"`" http://localhost:3001/#/login",
    ""
) -join "`r`n"
[IO.File]::WriteAllText((Join-Path $stage "START.bat"), $bat, (New-Object Text.ASCIIEncoding))

# --- инструкция (UTF-8 с BOM — корректно открывается в Блокноте) ----------
$readme = @"
Negotiation Arena - установка на Windows (быстрая)
==================================================

ЧТО В АРХИВЕ
  negotiation-arena.exe   - сервер (Rust, Python и прочее ставить не нужно)
  START.bat               - запуск в один клик
  dist\                   - веб-интерфейс (обязателен, должен лежать рядом с exe)
  static\, docs\          - логотипы, favicon, презентация
  scripts\                - локальный голос (необязателен, нужен Python)

УСТАНОВКА (2 шага)
  1. Распакуйте архив в любую папку (например C:\NegotiationArena)
  2. Запустите START.bat

  Откроется http://localhost:3001/#/login
  Логин:   admin
  Пароль:  admin123

ЗАПУСК / ОСТАНОВКА
  Сервер работает в отдельном окне с логами - закройте это окно, и он остановится.
  Повторный START.bat лишь откроет браузер, если сервер уже запущен.
  Порт занят? Задайте свой:  set PORT=3002  и запустите negotiation-arena.exe.

НАСТРОЙКИ (необязательно, через переменные окружения)
  PORT=3001               - порт
  DB_PATH=arena.db        - файл базы (создаётся рядом с exe)
  ADMIN_PASSWORD=...      - пароль админа при первом запуске
  JWT_SECRET=...          - секрет токенов (в продакшене обязателен!)
  Модели (OpenAI/Groq/Ollama) подключаются в админке: Провайдеры.
  Без моделей работает демо-собеседник (офлайн) - базовый режим.

ЕСЛИ ЧТО-ТО НЕ РАБОТАЕТ
  Откройте PowerShell, перейдите в папку и выполните:
      .\negotiation-arena.exe
  Текст ошибки будет виден сразу.

ФАЙЛЫ ДАННЫХ
  База создаётся рядом с .exe (negotiation_arena.db) - распакуйте архив в папку
  с правом на запись (не в C:\Program Files).

Удаление: просто удалите папку.
"@
$utf8Bom = New-Object Text.UTF8Encoding $true
[IO.File]::WriteAllText((Join-Path $stage "README-USTANOVKA.txt"), $readme, $utf8Bom)

# --- архив -----------------------------------------------------------------
$OutZip = [System.IO.Path]::GetFullPath($OutZip)
if (Test-Path $OutZip) { Remove-Item $OutZip -Force }
New-Item -ItemType Directory -Force -Path (Split-Path $OutZip) | Out-Null
Compress-Archive -Path $stage -DestinationPath $OutZip

$mb = [math]::Round((Get-Item $OutZip).Length / 1MB, 1)
Write-Host "OK: $OutZip ($mb MB)"
