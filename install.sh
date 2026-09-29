#!/usr/bin/env bash
# Установщик Negotiation Arena — один скрипт: установка / обновление / удаление.
#
# Использование:
#   ./install.sh                 интерактивное меню (1 Установить / 2 Обновить / 3 Удалить)
#   ./install.sh install         установить (клонирует репозиторий, собирает, создаёт ярлык)
#   ./install.sh update          git pull origin main + пересборка + обновление ярлыков
#   ./install.sh uninstall       убрать ярлык, команды и алиас (данные — по подтверждению)
#   curl -fsSL https://raw.githubusercontent.com/ApollieKastro/negotiation-arena/main/install.sh | bash
#
# Идемпотентен: повторная установка просто перезаписывает файлы без дублей.

# ── Самозащита: bash дочитывает скрипт с диска, а «Обновить» меняет install.sh
# в репозитории — выполняемся из временной копии, исходный файл можно менять.
if [ -f "${0:-}" ]; then
    _NA_ORIG_DIR="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
    export _NA_ORIG_DIR
fi
if [ "${_NA_REEXEC:-}" != "1" ] && [ -f "${0:-}" ] && head -n1 "$0" | grep -q "bash"; then
    _NA_TMP="$(mktemp)"
    cp "$0" "$_NA_TMP"
    export _NA_REEXEC=1 _NA_TMP
    exec bash "$_NA_TMP" "$@"
fi
[ -n "${_NA_TMP:-}" ] && trap 'rm -f "$_NA_TMP"' EXIT

set -euo pipefail
# ── Windows / WSL detection ──────────────────────────────────────────────────
if [[ "$OSTYPE" == "msys" || "$OSTYPE" == "cygwin" || -n "${WSL_DISTRO_NAME:-}" ]]; then
    echo "⚠ Windows detected. Use install.ps1 for native Windows installation:"
    echo "   PowerShell: .\install.ps1"
    echo "   Or run in WSL: wsl ./install.sh"
    if [[ -n "${WSL_DISTRO_NAME:-}" ]]; then
        echo "   (WSL detected: $WSL_DISTRO_NAME — continuing in Linux mode)"
    else
        exit 1
    fi
fi

# ── Параметры (переопределяются переменными окружения) ─────────────────────
REPO_URL="https://github.com/ApollieKastro/negotiation-arena.git"
BRANCH="main"
REPO_DIR="${NEGOTIATION_ARENA_HOME:-$HOME/App/negotiation-arena}"
BIN_DIR="${NA_BIN_DIR:-$HOME/.local/bin}"
ZSHRC="${NA_ZSHRC:-$HOME/.zshrc}"
DATA_DIR="${XDG_DATA_HOME:-$HOME/.local/share}"
DESKTOP_DIR="$DATA_DIR/applications"
CACHE_DIR="${XDG_CACHE_HOME:-$HOME/.cache}"
WRAPPER="$BIN_DIR/negotiation-arena"
LAUNCHER="$BIN_DIR/arena-launch"
DESKTOP="$DESKTOP_DIR/negotiation-arena.desktop"
PID_FILE="$CACHE_DIR/negotiation-arena.pid"
LOG_FILE="$CACHE_DIR/negotiation-arena.log"

# Если скрипт запущен из клона репозитория — работаем с ним, а не с дефолтным путём.
if [ -n "${_NA_ORIG_DIR:-}" ] && [ -e "$_NA_ORIG_DIR/.git" ] &&
    git -C "$_NA_ORIG_DIR" remote get-url origin 2>/dev/null | grep -q "negotiation-arena"; then
    REPO_DIR="$_NA_ORIG_DIR"
fi

# ── Вывод ──────────────────────────────────────────────────────────────────
c_ok=$'\033[32m'; c_warn=$'\033[33m'; c_err=$'\033[31m'; c_off=$'\033[0m'
[ -t 1 ] || { c_ok=""; c_warn=""; c_err=""; c_off=""; }
ok()   { printf '%s✓%s %s\n' "$c_ok" "$c_off" "$*"; }
warn() { printf '%s!%s %s\n' "$c_warn" "$c_off" "$*"; }
err()  { printf '%s✗%s %s\n' "$c_err" "$c_off" "$*" >&2; }
die()  { err "$*"; exit 1; }

# Чтение ответа: stdin может быть piping (curl | bash) — тогда берём /dev/tty;
# если и его нет (CI/пайп без терминала) — возвращаем отказ, вызывающий даёт дефолт.
ask() {
    if [ -t 0 ]; then IFS= read -r "${1:?}" || return 1
    else IFS= read -r "${1:?}" 2>/dev/null </dev/tty || return 1; fi
}

need_cmd() { command -v "$1" >/dev/null 2>&1 || die "не найдена команда «$1» — установите её и повторите"; }

# ── Остановка фонового сервера (по pid-файлу; безопаснее pkill -f) ─────────
stop_server() {
    if [ -f "$PID_FILE" ]; then
        local pid
        pid="$(cat "$PID_FILE" 2>/dev/null || true)"
        if [ -n "$pid" ] && kill -0 "$pid" 2>/dev/null; then
            kill "$pid" 2>/dev/null || true
            sleep 1
            ok "сервер остановлен (pid $pid)"
        fi
        rm -f "$PID_FILE"
    fi
}

# ── Генерация файлов установки ─────────────────────────────────────────────
install_files() {
    mkdir -p "$BIN_DIR" "$DESKTOP_DIR" "$CACHE_DIR"

    # Обёртка запуска: cd в проект, авто-сборка при изменениях, запуск бинарника.
    sed "s|@REPO_DIR@|$REPO_DIR|g" > "$WRAPPER" <<'WRAP'
#!/bin/sh
# Запуск negotiation-arena из любого каталога (создано install.sh).
set -e
APP_DIR="@REPO_DIR@"
BIN="$APP_DIR/target/release/negotiation-arena"
cd "$APP_DIR"
if [ ! -x "$BIN" ] || find src Cargo.toml Cargo.lock -newer "$BIN" -print -quit 2>/dev/null | grep -q .; then
    echo "[arena] сборка release…" >&2
    cargo build --release --quiet
fi
exec "$BIN" "$@"
WRAP

    # Лаунчер ярлыка: поднять сервер при необходимости + открыть браузер.
    sed -e "s|@WRAPPER@|$WRAPPER|g" -e "s|@PID_FILE@|$PID_FILE|g" -e "s|@LOG_FILE@|$LOG_FILE|g" > "$LAUNCHER" <<'LAUNCH'
#!/bin/sh
# Ярлык: поднимает сервер (если его ещё нет) и открывает браузер.
# Повторный клик безопасен — второй экземпляр не поднимается.
HEALTH_URL="http://localhost:3001/health"
APP_URL="http://localhost:3001/#/login"

if ! curl -fsS --max-time 2 "$HEALTH_URL" >/dev/null 2>&1; then
    mkdir -p "$(dirname "@LOG_FILE@")"
    nohup "@WRAPPER@" >>"@LOG_FILE@" 2>&1 &
    echo $! > "@PID_FILE@"
    i=0
    while [ "$i" -lt 50 ]; do
        curl -fsS --max-time 2 "$HEALTH_URL" >/dev/null 2>&1 && break
        i=$((i + 1)); sleep 0.2
    done
fi

if curl -fsS --max-time 2 "$HEALTH_URL" >/dev/null 2>&1; then
    xdg-open "$APP_URL" >/dev/null 2>&1 &
else
    xdg-open "@LOG_FILE@" >/dev/null 2>&1 &
    exit 1
fi
LAUNCH

    # Ярлык приложения (freedesktop .desktop).
    sed -e "s|@LAUNCHER@|$LAUNCHER|g" -e "s|@ICON@|$REPO_DIR/static/team-logo-mark.png|g" > "$DESKTOP" <<'DESK'
[Desktop Entry]
Type=Application
Name=Negotiation Arena
GenericName=Тренажёр переговоров
Comment=Локальный тренажёр переговоров: поднимает сервер и открывает браузер
Exec=@LAUNCHER@
Icon=@ICON@
Terminal=false
Categories=Education;
Keywords=переговоры;negotiation;arena;training;
StartupNotify=true
DESK

    chmod +x "$WRAPPER" "$LAUNCHER"
    ok "команды: $WRAPPER, $LAUNCHER"
    ok "ярлык:   $DESKTOP"
}

ensure_alias() {
    [ -f "$ZSHRC" ] || touch "$ZSHRC"
    if ! grep -q "^alias arena=" "$ZSHRC" 2>/dev/null; then
        printf '\n# Запуск negotiation-arena без cargo run (см. install.sh)\nalias arena='"'"'negotiation-arena'"'"'\n' >> "$ZSHRC"
        ok "алиас arena добавлен в $ZSHRC (актуализировать: exec zsh)"
    fi
}

PATH_COMMENT="# $BIN_DIR в PATH (install.sh)"
PATH_EXPORT="export PATH=\"$BIN_DIR:\$PATH\""
ensure_path() {
    # Два guard-а: строка уже в .zshrc (идемпотентность) или каталог уже в PATH.
    if grep -Fqx "$PATH_COMMENT" "$ZSHRC" 2>/dev/null; then
        return 0
    fi
    case ":$PATH:" in
        *":$BIN_DIR:"*) ok "$BIN_DIR уже в PATH" ;;
        *)
            printf '\n%s\n%s\n' "$PATH_COMMENT" "$PATH_EXPORT" >> "$ZSHRC"
            warn "$BIN_DIR добавлен в PATH — перезапустите шелл (exec zsh)"
            ;;
    esac
}

validate_desktop() {
    if command -v desktop-file-validate >/dev/null 2>&1; then
        desktop-file-validate "$DESKTOP" && ok "ярлык валиден"
    fi
}

# ── 1. Установка ───────────────────────────────────────────────────────────
cmd_install() {
    need_cmd git
    need_cmd curl

    if [ -d "$REPO_DIR/.git" ]; then
        ok "репозиторий уже есть: $REPO_DIR"
        git -C "$REPO_DIR" pull --ff-only --quiet 2>/dev/null &&
            ok "код обновлён до ветки $BRANCH" ||
            warn "не удалось fast-forward (есть локальные изменения?) — ставим как есть"
    else
        mkdir -p "$(dirname "$REPO_DIR")"
        need_cmd cargo || { err "для новой установки нужен Rust: curl https://sh.rustup.rs -sSf | sh"; exit 1; }
        ok "клонирую $REPO_URL (ветка $BRANCH) → $REPO_DIR"
        git clone --branch "$BRANCH" "$REPO_URL" "$REPO_DIR"
    fi

    command -v cargo >/dev/null 2>&1 || die "нет Rust/cargo — установите: curl https://sh.rustup.rs -sSf | sh"
    command -v python3 >/dev/null 2>&1 || warn "нет python3 — голос (TTS/STT) не заработает до установки Python"

    ok "сборка release (первый раз — до пары минут)…"
    (cd "$REPO_DIR" && cargo build --release --quiet)
    ok "бинарник собран: $REPO_DIR/target/release/negotiation-arena"

    install_files
    ensure_alias
    ensure_path
    validate_desktop

    echo
    ok "Установка завершена."
    printf '  Запуск:   ярлык «Negotiation Arena» | команда arena | negotiation-arena\n'
    printf '  Остановка: kill "$(cat %s)"\n' "$PID_FILE"
    printf '  Обновление: ./install.sh update (или из меню установщика)\n'
}

# ── 2. Обновление с GitHub (main) ─────────────────────────────────────────
cmd_update() {
    need_cmd git
    [ -e "$REPO_DIR/.git" ] || die "репозиторий не найден: $REPO_DIR — сначала установите (./install.sh install)"

    stop_server

    local old new
    old="$(git -C "$REPO_DIR" rev-parse HEAD)"
    if ! git -C "$REPO_DIR" pull --ff-only --quiet; then
        die "git pull не прошёл — в $REPO_DIR есть локальные изменения; разберитесь с ними (git status) и повторите"
    fi
    new="$(git -C "$REPO_DIR" rev-parse HEAD)"

    if [ "$old" = "$new" ]; then
        ok "уже актуально (ветка $BRANCH, ничего не изменилось)"
    else
        ok "обновлено: $(git -C "$REPO_DIR" rev-list --count "$old".."$new") коммит(ов)"
        git -C "$REPO_DIR" log --oneline "$old".."$new" | sed 's/^/    /'
    fi

    ok "пересборка release…"
    (cd "$REPO_DIR" && cargo build --release --quiet)
    ok "бинарник собран"

    # Ярлыки/скрипты могли обновиться в новых коммитах — перегенерируем.
    install_files
    validate_desktop
    echo
    ok "Обновление завершено. Запуск: ярлык / arena"
}

# ── 3. Удаление ───────────────────────────────────────────────────────────
cmd_uninstall() {
    stop_server
    rm -f "$WRAPPER" "$LAUNCHER" "$DESKTOP" "$PID_FILE" "$LOG_FILE"
    ok "удалены команды, ярлык и логи"

    if [ -f "$ZSHRC" ]; then
        # Точное совпадение строк (без regex) — удаляем только то, что писали сами.
        # grep возвращает 1, если после фильтра не осталось строк, — это норма.
        grep -vxF \
            -e "alias arena='negotiation-arena'" \
            -e "# Запуск negotiation-arena без cargo run (см. install.sh)" \
            -e "# $BIN_DIR в PATH (install.sh)" \
            -e "export PATH=\"$BIN_DIR:\$PATH\"" \
            "$ZSHRC" > "$ZSHRC.tmp" || true
        mv "$ZSHRC.tmp" "$ZSHRC"
        ok "алиас arena и строка PATH убраны из $ZSHRC"
    fi

    echo
    printf 'Удалить также каталог проекта %s (БД, истории сессий, пользователи, модели)?) [y/N]: ' "$REPO_DIR"
    local answer=""
    ask answer || answer=""
    case "$answer" in
        y|Y|д|Д)
            # Защита от rm -rf не в том месте: только наш git-репозиторий под $HOME.
            case "$REPO_DIR" in
                "$HOME"/*)
                    if [ -e "$REPO_DIR/.git" ] &&
                        git -C "$REPO_DIR" remote get-url origin 2>/dev/null | grep -q "negotiation-arena"; then
                        rm -rf "$REPO_DIR"
                        ok "каталог удалён: $REPO_DIR"
                    else
                        warn "$REPO_DIR не похож на клон negotiation-arena — не удаляю"
                    fi
                    ;;
                *)
                    warn "путь вне домашнего каталога — не удаляю: $REPO_DIR"
                    ;;
            esac
            ;;
        *)
            warn "каталог проекта сохранён: $REPO_DIR"
            ;;
    esac

    echo
    ok "Удаление завершено. Rust/cargo не трогал."
}

# ── Меню ───────────────────────────────────────────────────────────────────
show_menu() {
    echo
    echo "Установщик Negotiation Arena (ветка $BRANCH)"
    echo "  1) Установить"
    echo "  2) Обновить (с GitHub)"
    echo "  3) Удалить"
    echo "  0) Выход"
    printf "Выбор: "
}

main_menu() {
    while true; do
        show_menu
        local choice=""
        ask choice || exit 0
        case "$choice" in
            1) cmd_install; return ;;
            2) cmd_update;  return ;;
            3) cmd_uninstall; return ;;
            0|"") return ;;
            *) warn "неизвестный пункт: $choice" ;;
        esac
    done
}

case "${1:-}" in
    install)   cmd_install ;;
    update)    cmd_update ;;
    uninstall|remove) cmd_uninstall ;;
    ""|menu)   main_menu ;;
    *) die "неизвестная команда: $1 (install|update|uninstall)" ;;
esac
