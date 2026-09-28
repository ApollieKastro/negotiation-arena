<# 
.SYNOPSIS
    Negotiation Arena Windows Installer - Install / Update / Uninstall

.DESCRIPTION
    One script for full lifecycle management on Windows.
    Creates Start Menu shortcut, Desktop shortcut, adds to PATH, creates 'arena' alias.

.USAGE
    .\install.ps1                 # Interactive menu
    .\install.ps1 install         # Install
    .\install.ps1 update          # Update from GitHub
    .\install.ps1 uninstall       # Uninstall
#>

# Self-execution protection (like install.sh)
$origPath = $MyInvocation.MyCommand.Path
if ($origPath -and (Test-Path $origPath) -and -not $env:NA_REEXEC) {
    $tmp = [IO.Path]::GetTempFileName()
    Copy-Item $origPath $tmp -Force
    $env:NA_REEXEC = "1"
    & powershell -NoProfile -ExecutionPolicy Bypass -File $tmp @args
    Remove-Item $tmp -ErrorAction SilentlyContinue
    exit
}

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

# Configuration
$REPO_URL      = "https://github.com/ApollieKastro/negotiation-arena.git"
$BRANCH        = "main"
$REPO_DIR      = $env:NEGOTIATION_ARENA_HOME ?? "$env:USERPROFILE\App\negotiation-arena"
$BIN_DIR       = $env:NA_BIN_DIR ?? "$env:USERPROFILE\.local\bin"
$DATA_DIR      = $env:APPDATA
$START_MENU    = Join-Path $DATA_DIR "Microsoft\Windows\Start Menu\Programs\Negotiation Arena"
$DESKTOP       = [Environment]::GetFolderPath("Desktop")
$CACHE_DIR     = $env:LOCALAPPDATA ?? "$env:USERPROFILE\AppData\Local"
$PID_FILE      = Join-Path $CACHE_DIR "negotiation-arena.pid"
$LOG_FILE      = Join-Path $CACHE_DIR "negotiation-arena.log"
$WRAPPER       = Join-Path $BIN_DIR "negotiation-arena.bat"
$LAUNCHER      = Join-Path $BIN_DIR "arena-launch.bat"
$PS_PROFILE    = $PROFILE.CurrentUserAllHosts

# Colors
$c_ok   = "Green"
$c_warn = "Yellow"
$c_err  = "Red"
$c_off  = "Gray"

function Write-Ok   { param($msg) Write-Host "✓ $msg" -ForegroundColor $c_ok }
function Write-Warn { param($msg) Write-Host "! $msg" -ForegroundColor $c_warn }
function Write-Err  { param($msg) Write-Host "✗ $msg" -ForegroundColor $c_err }
function Write-Info { param($msg) Write-Host "  $msg" -ForegroundColor $c_off }

function Need-Cmd($name, $hint = "") {
    if (-not (Get-Command $name -ErrorAction SilentlyContinue)) {
        $msg = "Required command '$name' not found"
        if ($hint) { $msg += ". $hint" }
        throw $msg
    }
}

# Stop background server
function Stop-Server {
    if (Test-Path $PID_FILE) {
        $pid = Get-Content $PID_FILE -ErrorAction SilentlyContinue
        if ($pid -and (Get-Process -Id $pid -ErrorAction SilentlyContinue)) {
            Stop-Process -Id $pid -Force -ErrorAction SilentlyContinue
            Start-Sleep 1
            Write-Ok "Server stopped (pid $pid)"
        }
        Remove-Item $PID_FILE -Force -ErrorAction SilentlyContinue
    }
}

# Create wrapper batch files
function Install-Files {
    New-Item -ItemType Directory -Force -Path $BIN_DIR, $START_MENU, $DESKTOP, $CACHE_DIR | Out-Null

    # negotiation-arena.bat - runs from repo dir, auto-builds
    @"
@echo off
set APP_DIR=$REPO_DIR
set BIN=%APP_DIR%\target\release\negotiation-arena.exe
cd /d "%APP_DIR%"
if not exist "%BIN%" (
    echo [arena] Building release...
    cargo build --release --quiet
)
"%BIN%" %*
"@
    | Set-Content -Encoding ASCII $WRAPPER

    # arena-launch.bat - starts server if needed, opens browser
    @"
@echo off
set HEALTH_URL=http://localhost:3001/health
set APP_URL=http://localhost:3001
set WRAPPER=$WRAPPER
set LOG_FILE=$LOG_FILE
set PID_FILE=$PID_FILE

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
"@
    | Set-Content -Encoding ASCII $LAUNCHER

    # Start Menu shortcut (.url)
    $url = Join-Path $START_MENU "Negotiation Arena.url"
    @"
[InternetShortcut]
URL=file:///$LAUNCHER
IconFile=$REPO_DIR\static\team-logo-mark.png
IconIndex=0
"@
    | Set-Content -Encoding ASCII $url

    # Desktop shortcut
    $deskUrl = Join-Path $DESKTOP "Negotiation Arena.url"
    Copy-Item $url $deskUrl -Force

    Write-Ok "Wrapper: $WRAPPER"
    Write-Ok "Launcher: $LAUNCHER"
    Write-Ok "Start Menu: $url"
    Write-Ok "Desktop: $deskUrl"
}

# Add to PATH (user scope)
function Ensure-Path {
    $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
    if ($userPath -notlike "*$BIN_DIR*") {
        [Environment]::SetEnvironmentVariable("Path", "$userPath;$BIN_DIR", "User")
        Write-Warn "Added $BIN_DIR to User PATH. Restart terminal."
    } else {
        Write-Ok "$BIN_DIR already in PATH"
    }
}

# Add arena alias to PowerShell profile
function Ensure-Alias {
    if (-not (Test-Path $PS_PROFILE)) {
        New-Item -ItemType File -Force -Path $PS_PROFILE | Out-Null
    }
    $profileContent = Get-Content $PS_PROFILE -Raw -ErrorAction SilentlyContinue
    if ($profileContent -notlike "*alias arena=*") {
        @"
# Negotiation Arena alias (added by install.ps1)
function arena { & negotiation-arena @args }
Set-Alias -Name arena -Value negotiation-arena
"@
        | Add-Content $PS_PROFILE
        Write-Ok "Added 'arena' alias to PowerShell profile"
    }
}

# Validate prerequisites
function Check-Prereqs {
    Need-Cmd "git"
    Need-Cmd "curl"
    Need-Cmd "cargo" "Install Rust: https://rustup.rs"
    Need-Cmd "python" "Install Python from python.org or Microsoft Store"
    Write-Ok "Prerequisites OK"
}

# 1. Install
function Cmd-Install {
    Check-Prereqs

    if (Test-Path "$REPO_DIR\.git") {
        Write-Ok "Repository exists: $REPO_DIR"
        try {
            git -C $REPO_DIR pull --ff-only --quiet
            Write-Ok "Code updated to $BRANCH"
        } catch {
            Write-Warn "Fast-forward failed (local changes?) - building as-is"
        }
    } else {
        Write-Info "Cloning $REPO_URL (branch $BRANCH) -> $REPO_DIR"
        New-Item -ItemType Directory -Force -Path (Split-Path $REPO_DIR) | Out-Null
        git clone --branch $BRANCH $REPO_URL $REPO_DIR
    }

    Write-Info "Building release (first time takes a few minutes)..."
    cd $REPO_DIR
    cargo build --release --quiet
    Write-Ok "Binary built: $REPO_DIR\target\release\negotiation-arena.exe"

    Install-Files
    Ensure-Path
    Ensure-Alias

    Write-Host ""
    Write-Ok "Installation complete."
    Write-Info "  Launch: Start Menu / Desktop / 'arena' / 'negotiation-arena'"
    Write-Info "  Stop:   Stop-Process -Id (Get-Content $PID_FILE)"
    Write-Info "  Update: .\install.ps1 update"
}

# 2. Update
function Cmd-Update {
    Need-Cmd "git"
    if (-not (Test-Path "$REPO_DIR\.git")) {
        throw "Repository not found: $REPO_DIR - run install first"
    }

    Stop-Server

    $old = git -C $REPO_DIR rev-parse HEAD
    try {
        git -C $REPO_DIR pull --ff-only --quiet
    } catch {
        throw "git pull failed - local changes in $REPO_DIR (git status)"
    }
    $new = git -C $REPO_DIR rev-parse HEAD

    if ($old -eq $new) {
        Write-Ok "Already up to date (branch $BRANCH)"
    } else {
        $count = (git -C $REPO_DIR rev-list --count "$old..$new")
        Write-Ok "Updated: $count commit(s)"
        git -C $REPO_DIR log --oneline "$old..$new" | ForEach-Object { Write-Info "    $_" }
    }

    Write-Info "Rebuilding release..."
    cd $REPO_DIR
    cargo build --release --quiet
    Write-Ok "Binary rebuilt"

    Install-Files
    Write-Ok "Update complete. Launch: Start Menu / 'arena'"
}

# 3. Uninstall
function Cmd-Uninstall {
    Stop-Server
    Remove-Item $WRAPPER, $LAUNCHER, $PID_FILE, $LOG_FILE -Force -ErrorAction SilentlyContinue
    Remove-Item $START_MENU -Recurse -Force -ErrorAction SilentlyContinue
    Remove-Item (Join-Path $DESKTOP "Negotiation Arena.url") -Force -ErrorAction SilentlyContinue
    Write-Ok "Removed shortcuts, commands, logs"

    # Remove from PATH
    $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
    if ($userPath -like "*$BIN_DIR*") {
        $newPath = ($userPath -split ';' | Where-Object { $_ -ne $BIN_DIR }) -join ';'
        [Environment]::SetEnvironmentVariable("Path", $newPath, "User")
        Write-Ok "Removed $BIN_DIR from User PATH (restart terminal)"
    }

    # Remove alias from profile
    if (Test-Path $PS_PROFILE) {
        $content = Get-Content $PS_PROFILE -Raw
        $newContent = $content -replace "(?ms)^# Negotiation Arena alias.*?Set-Alias -Name arena -Value negotiation-arena\s*", ""
        if ($newContent -ne $content) {
            Set-Content $PS_PROFILE $newContent
            Write-Ok "Removed 'arena' alias from PowerShell profile"
        }
    }

    Write-Host ""
    $confirm = Read-Host "Also delete project folder $REPO_DIR (DB, history, models)? [y/N]"
    if ($confirm -match '^[yYдД]') {
        if (Test-Path "$REPO_DIR\.git" -and (git -C $REPO_DIR remote get-url origin 2>$null | Select-String "negotiation-arena")) {
            Remove-Item $REPO_DIR -Recurse -Force
            Write-Ok "Deleted: $REPO_DIR"
        } else {
            Write-Warn "$REPO_DIR doesn't look like negotiation-arena clone - skipping"
        }
    } else {
        Write-Warn "Project folder kept: $REPO_DIR"
    }

    Write-Ok "Uninstall complete. Rust/Python not touched."
}

# Menu
function Show-Menu {
    Write-Host "`nNegotiation Arena Installer (branch $BRANCH)" -ForegroundColor Cyan
    Write-Host "  1) Install"
    Write-Host "  2) Update (from GitHub)"
    Write-Host "  3) Uninstall"
    Write-Host "  0) Exit"
    Write-Host "Choice: " -NoNewline
}

# Main
$cmd = $args[0] ?? ""
switch ($cmd) {
    "install"   { Cmd-Install;   break }
    "update"    { Cmd-Update;    break }
    "uninstall" { Cmd-Uninstall; break }
    ""          {
        while ($true) {
            Show-Menu
            $choice = Read-Host
            switch ($choice) {
                "1" { Cmd-Install;   break }
                "2" { Cmd-Update;    break }
                "3" { Cmd-Uninstall; break }
                "0" { break }
                default { Write-Warn "Unknown choice: $choice" }
            }
            if ($choice -in @("1","2","3","0")) { break }
        }
    }
    default { throw "Unknown command: $cmd (install|update|uninstall)" }
}
