<# 
.SYNOPSIS
    Negotiation Arena Windows Installer - Install / Update / Uninstall

.DESCRIPTION
    One script for full lifecycle management on Windows.
    Creates Start Menu shortcut, Desktop shortcut, adds to PATH, creates 'arena' alias.

.USAGE
    iex ((New-Object Net.WebClient).DownloadString('https://raw.githubusercontent.com/ApollieKastro/negotiation-arena/main/install.ps1') -replace '^\uFEFF','')
    .\install.ps1 install
    .\install.ps1 update
    .\install.ps1 uninstall
#>

#requires -Version 5.1

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

# ─── Configuration ──────────────────────────────────────────────────────
$REPO_URL      = "https://github.com/ApollieKastro/negotiation-arena.git"
$BRANCH        = "main"
$REPO_DIR      = Join-Path $env:USERPROFILE "App\negotiation-arena"
$BIN_DIR       = Join-Path $env:USERPROFILE ".local\bin"
$START_MENU    = Join-Path $env:APPDATA "Microsoft\Windows\Start Menu\Programs\Negotiation Arena"
$DESKTOP       = [Environment]::GetFolderPath("Desktop")
$CACHE_DIR     = $env:LOCALAPPDATA
$PID_FILE      = Join-Path $CACHE_DIR "negotiation-arena.pid"
$LOG_FILE      = Join-Path $CACHE_DIR "negotiation-arena.log"
$WRAPPER       = Join-Path $BIN_DIR "negotiation-arena.bat"
$LAUNCHER      = Join-Path $BIN_DIR "arena-launch.bat"
$PS_PROFILE    = $PROFILE.CurrentUserAllHosts

# ─── UI helpers ─────────────────────────────────────────────────────────
function Ok   { param($m) Write-Host "✓ $m" -ForegroundColor Green }
function Warn { param($m) Write-Host "! $m" -ForegroundColor Yellow }
function Err  { param($m) Write-Host "✗ $m" -ForegroundColor Red }
function Info { param($m) Write-Host "  $m" -ForegroundColor Gray }

function Need-Cmd($name, $hint = "") {
    if (-not (Get-Command $name -ErrorAction SilentlyContinue)) {
        $msg = "Required command '$name' not found"
        if ($hint) { $msg += ". $hint" }
        throw $msg
    }
}

# Execute native command with proper exit code checking (works in PS 5.1+)
function Run-Cmd {
    param([scriptblock]$ScriptBlock, [string]$ErrorMsg)
    & $ScriptBlock
    if ($LASTEXITCODE -ne 0) { throw "$ErrorMsg (exit code $LASTEXITCODE)" }
}

# ─── Core operations ────────────────────────────────────────────────────
function Stop-Server {
    if (Test-Path $PID_FILE) {
        $pid = Get-Content $PID_FILE -ErrorAction SilentlyContinue
        if ($pid -and (Get-Process -Id $pid -ErrorAction SilentlyContinue)) {
            Stop-Process -Id $pid -Force -ErrorAction SilentlyContinue
            Start-Sleep 1
            Ok "Server stopped (pid $pid)"
        }
        Remove-Item $PID_FILE -Force -ErrorAction SilentlyContinue
    }
}

function Install-Files {
    New-Item -ItemType Directory -Force -Path $BIN_DIR, $START_MENU, $DESKTOP, $CACHE_DIR | Out-Null

    # negotiation-arena.bat
    @"
@echo off
set APP_DIR=%REPO_DIR%
set BIN=%APP_DIR%\target\release\negotiation-arena.exe
cd /d "%APP_DIR%"
if not exist "%BIN%" (
    echo [arena] Building release...
    cargo build --release --quiet
)
"%BIN%" %*
"@ -replace '%REPO_DIR%', $REPO_DIR | Set-Content -Encoding ASCII $WRAPPER

    # arena-launch.bat
    @"
@echo off
set HEALTH_URL=http://localhost:3001/health
set APP_URL=http://localhost:3001/#/login
set WRAPPER=%WRAPPER%
set LOG_FILE=%LOG_FILE%
set PID_FILE=%PID_FILE%

curl -fsS --max-time 2 "%HEALTH_URL%" >nul 2>&1
if errorlevel 1 (
    mkdir "%LOG_FILE%\.." 2>nul
    start /b "" "%WRAPPER%" >>"%LOG_FILE%" 2>&1
    echo %ERRORLEVEL% > "%PID_FILE%"
    for /l %%i in (1,1,50) do (
        curl -fsS --max-time 2 "%HEALTH_URL%" >nul 2>&1 && goto :open
        timeout /t 1 >nul
    )
    goto :fail
)

:open
start "" "%APP_URL%"
exit /b 0

:fail
start "" "%LOG_FILE%"
exit /b 1
"@ -replace '%WRAPPER%', $WRAPPER -replace '%LOG_FILE%', $LOG_FILE -replace '%PID_FILE%', $PID_FILE | Set-Content -Encoding ASCII $LAUNCHER

    # Start Menu shortcut
    $url = Join-Path $START_MENU "Negotiation Arena.url"
    @"
[InternetShortcut]
URL=file:///$LAUNCHER
IconFile=$REPO_DIR\static\team-logo-mark.png
IconIndex=0
"@ | Set-Content -Encoding ASCII $url

    # Desktop shortcut
    Copy-Item $url (Join-Path $DESKTOP "Negotiation Arena.url") -Force

    Ok "Wrapper: $WRAPPER"
    Ok "Launcher: $LAUNCHER"
    Ok "Start Menu: $url"
    Ok "Desktop: $url"
}

function Ensure-Path {
    # Прим.: оператор `??` в PS 7, здесь — совместимо с Windows PowerShell 5.1
    $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
    if (-not $userPath) { $userPath = "" }
    if ($userPath -notlike "*$BIN_DIR*") {
        [Environment]::SetEnvironmentVariable("Path", "$userPath;$BIN_DIR", "User")
        Warn "Added $BIN_DIR to User PATH. Restart terminal."
    } else { Ok "$BIN_DIR already in PATH" }
}

function Ensure-Alias {
    if (-not (Test-Path $PS_PROFILE)) { New-Item -ItemType File -Force -Path $PS_PROFILE | Out-Null }
    $content = Get-Content $PS_PROFILE -Raw -ErrorAction SilentlyContinue
    if ($content -notlike "*alias arena=*") {
        @"
# Negotiation Arena alias (added by install.ps1)
function arena { & negotiation-arena @args }
Set-Alias -Name arena -Value negotiation-arena
"@ | Add-Content $PS_PROFILE
        Ok "Added 'arena' alias to PowerShell profile"
    }
}

function Check-Prereqs {
    Need-Cmd "git"
    Need-Cmd "curl"
    Need-Cmd "cargo" "Install Rust from https://rustup.rs (needed to build from source)"
    # Python нужен только для локального озвучивания/распознавания (scripts/*.py):
    # без него всё остальное работает, поэтому это предупреждение, а не ошибка.
    if (Get-Command "python" -ErrorAction SilentlyContinue) {
        Ok "Prerequisites OK"
    } else {
        Warn "Python not found - voice (STT/TTS) unavailable. Core app works fine."
    }
}

# ─── Commands ───────────────────────────────────────────────────────────
function Cmd-Install {
    Check-Prereqs
    if (Test-Path "$REPO_DIR\.git") {
        Ok "Repository exists: $REPO_DIR"
        Run-Cmd { git -C $REPO_DIR pull --ff-only --quiet } "git pull failed"
        Ok "Code updated to $BRANCH"
    } else {
        Info "Cloning $REPO_URL (branch $BRANCH) -> $REPO_DIR"
        New-Item -ItemType Directory -Force -Path (Split-Path $REPO_DIR) | Out-Null
        Run-Cmd { git clone --branch $BRANCH $REPO_URL $REPO_DIR } "git clone failed"
    }
    Info "Building release (first time takes a few minutes)..."
    cd $REPO_DIR
    Run-Cmd { cargo build --release --quiet } "cargo build failed"
    Ok "Binary built: $REPO_DIR\target\release\negotiation-arena.exe"
    # Голос (STT/TTS) — опционально: без Python/paketов приложение работает,
    # но озвучивание и распознавание вернут 502. Скрипт сам всё чинит или
    # мягко предупреждает, поэтому падать тут нельзя.
    Info "Voice setup (piper + faster-whisper, optional)..."
    & powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $REPO_DIR "scripts\setup-voice.ps1")
    Install-Files
    Ensure-Path
    Ensure-Alias
    Write-Host ""
    Ok "Installation complete."
    Info "  Launch: Start Menu / Desktop / 'arena' / 'negotiation-arena'"
    Info "  Stop:   Stop-Process -Id (Get-Content $PID_FILE)"
    Info "  Update: .\install.ps1 update"
}

function Cmd-Update {
    Need-Cmd "git"
    if (-not (Test-Path "$REPO_DIR\.git")) { throw "Repository not found: $REPO_DIR - run install first" }
    Stop-Server
    $old = git -C $REPO_DIR rev-parse HEAD
    Run-Cmd { git -C $REPO_DIR pull --ff-only --quiet } "git pull failed - local changes? (git status)"
    $new = git -C $REPO_DIR rev-parse HEAD
    if ($old -eq $new) { Ok "Already up to date (branch $BRANCH)" }
    else {
        $count = (git -C $REPO_DIR rev-list --count "$old..$new")
        Ok "Updated: $count commit(s)"
        git -C $REPO_DIR log --oneline "$old..$new" | ForEach-Object { Info "    $_" }
    }
    Info "Rebuilding release..."
    cd $REPO_DIR
    Run-Cmd { cargo build --release --quiet } "cargo build failed"
    Ok "Binary rebuilt"
    Install-Files
    Ok "Update complete. Launch: Start Menu / 'arena'"
}

function Cmd-Uninstall {
    Stop-Server
    Remove-Item $WRAPPER, $LAUNCHER, $PID_FILE, $LOG_FILE -Force -ErrorAction SilentlyContinue
    Remove-Item $START_MENU -Recurse -Force -ErrorAction SilentlyContinue
    Remove-Item (Join-Path $DESKTOP "Negotiation Arena.url") -Force -ErrorAction SilentlyContinue
    Ok "Removed shortcuts, commands, logs"

    $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
    if (-not $userPath) { $userPath = "" }
    if ($userPath -like "*$BIN_DIR*") {
        $newPath = ($userPath -split ';' | Where-Object { $_ -ne $BIN_DIR }) -join ';'
        [Environment]::SetEnvironmentVariable("Path", $newPath, "User")
        Ok "Removed $BIN_DIR from User PATH (restart terminal)"
    }
    if (Test-Path $PS_PROFILE) {
        $content = Get-Content $PS_PROFILE -Raw
        $new = $content -replace "(?ms)^# Negotiation Arena alias.*?Set-Alias -Name arena -Value negotiation-arena\s*", ""
        if ($new -ne $content) { Set-Content $PS_PROFILE $new; Ok "Removed 'arena' alias from PowerShell profile" }
    }
    Write-Host ""
    $confirm = Read-Host "Also delete project folder $REPO_DIR (DB, history, models)? [y/N]"
    if ($confirm -match '^[yYдД]') {
        if (Test-Path "$REPO_DIR\.git" -and (git -C $REPO_DIR remote get-url origin 2>$null | Select-String "negotiation-arena")) {
            Remove-Item $REPO_DIR -Recurse -Force
            Ok "Deleted: $REPO_DIR"
        } else { Warn "$REPO_DIR doesn't look like negotiation-arena clone - skipping" }
    } else { Warn "Project folder kept: $REPO_DIR" }
    Ok "Uninstall complete. Rust/Python not touched."
}

# ─── Menu / Entry point ────────────────────────────────────────────────
function Show-Menu {
    Write-Host "`nNegotiation Arena Installer (branch $BRANCH)" -ForegroundColor Cyan
    Write-Host "  1) Install"
    Write-Host "  2) Update (from GitHub)"
    Write-Host "  3) Uninstall"
    Write-Host "  0) Exit"
}

# Support both: .\install.ps1  |  .\install.ps1 install  |  iex (WebClient ...)
$cmd = if ($args.Count -gt 0) { $args[0] } else { "" }

switch ($cmd) {
    "install"   { Cmd-Install;   break }
    "update"    { Cmd-Update;    break }
    "uninstall" { Cmd-Uninstall; break }
    "" {
        while ($true) {
            Write-Host "`nNegotiation Arena Installer (branch $BRANCH)" -ForegroundColor Cyan
            Write-Host "  1) Install"
            Write-Host "  2) Update (from GitHub)"
            Write-Host "  3) Uninstall"
            Write-Host "  0) Exit"
            $choice = Read-Host "Choice"
            if ($null -eq $choice) { break }
            switch ($choice) {
                "1" { Cmd-Install;   break }
                "2" { Cmd-Update;    break }
                "3" { Cmd-Uninstall; break }
                "0" { break }
                default { Warn "Unknown choice: $choice" }
            }
            if ($choice -in @("1","2","3","0")) { break }
        }
        break
    }
    default { throw "Unknown command: $cmd (install|update|uninstall)" }
}