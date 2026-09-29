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
# UTF-8 без BOM + `chcp 65001` на второй строке: cmd читает файл в кодовой
# странице консоли, поэтому первая строка — чисто ASCII, а после смены
# кодовой страницы кириллические сообщения читаются корректно.
$bat = @(
    "@echo off",
    "chcp 65001 >nul 2>&1",
    "setlocal",
    "cd /d `"%~dp0`"",
    "",
    "if not exist `"%~dp0negotiation-arena.exe`" goto no_exe",
    "if not exist `"%~dp0dist\index.html`" echo [ВНИМАНИЕ] Не найдена папка dist - интерфейс не откроется. Распакуйте архив полностью.",
    "",
    "rem снять отметку `"из интернета`" - иначе SmartScreen/антивирус может блокировать",
    "powershell -NoProfile -ExecutionPolicy Bypass -Command `"Get-ChildItem -File | Unblock-File -ErrorAction SilentlyContinue`" >nul 2>&1",
    "",
    "curl -s -o nul http://localhost:3001/health >nul 2>&1",
    "if not errorlevel 1 goto open",
    "",
    "echo Запускаю сервер - откроется отдельное окно с логами (не закрывайте его)...",
    "start `"Negotiation Arena - сервер`" cmd /k `"`"%~dp0negotiation-arena.exe`"`"",
    "set tries=0",
    ":wait",
    "ping -n 2 127.0.0.1 >nul 2>&1",
    "set /a tries+=1",
    "curl -s -o nul http://localhost:3001/health >nul 2>&1",
    "if not errorlevel 1 goto ready",
    "if %tries% lss 15 goto wait",
    "goto failed",
    "",
    ":ready",
    "echo Готово! Страница: http://localhost:3001   Логин: admin   Пароль: admin123",
    ":open",
    "start `"`" http://localhost:3001/#/login",
    "exit /b 0",
    "",
    ":no_exe",
    "echo.",
    "echo [ОШИБКА] negotiation-arena.exe не найден рядом с START.bat.",
    "echo.",
    "echo Скорее всего архив распакован не полностью, или START.bat запущен",
    "echo прямо из окна архива. Сделайте так:",
    "echo   1. Откройте zip в Проводнике.",
    "echo   2. Нажмите `"Извлечь все`" (выделить ВСЁ содержимое).",
    "echo   3. Зайдите в извлечённую папку и запустите START.bat оттуда.",
    "echo.",
    "echo Если Windows пишет, что файл заблокирован (из интернета):",
    "echo   ПКМ по negotiation-arena.exe - Свойства - галочка `"Разблокировать`" - OK.",
    "echo.",
    "pause",
    "exit /b 1",
    "",
    ":failed",
    "echo.",
    "echo [ОШИБКА] Сервер не поднялся за 30 секунд.",
    "echo Загляните в окно сервера - там будет текст ошибки.",
    "echo Частые причины:",
    "echo   - SmartScreen/антивирус заблокировал negotiation-arena.exe;",
    "echo   - порт 3001 занят (тогда: set PORT=3002 и запустите exe вручную);",
    "echo   - нет прав на запись в папку (база создается рядом с exe - не ставьте в Program Files).",
    "echo.",
    "pause",
    "exit /b 1",
    ""
) -join "`r`n"
[IO.File]::WriteAllText((Join-Path $stage "START.bat"), $bat, (New-Object Text.UTF8Encoding $false))

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

  ВАЖНО: не запускайте START.bat из окна архива! Сначала нажмите в
  Проводнике "Извлечь все", зайдите в извлечённую папку и запускайте
  оттуда - иначе рядом с батником не будет negotiation-arena.exe.

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

ЕСЛИ START.bat ЗАКРЫВАЕТСЯ ИЛИ НИЧЕГО НЕ ПРОИСХОДИТ
  1. Архив не распакован - запуск идёт из окна zip (см. ВАЖНО выше).
     Сам лаунчер теперь сам пишет причину и ждёт нажатия клавиши.
  2. SmartScreen / антивирус: ПКМ по negotiation-arena.exe - Свойства -
     поставить галочку "Разблокировать" - OK. Лаунчер снимает эту
     отметку автоматически при каждом запуске.
  3. Откройте PowerShell, перейдите в папку и выполните:
         .\negotiation-arena.exe
     Текст ошибки будет виден сразу.

  START.bat сам: проверяет наличие exe, снимает блокировку "из интернета",
  ждёт готовности сервера до 30 секунд и в случае неудачи печатает
  причину (окно не закроется, пока вы не нажмёте клавишу).

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
