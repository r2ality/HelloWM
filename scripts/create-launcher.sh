#!/bin/sh
# Optional launcher-creation prompt. zenity/kdialog are used when present;
# otherwise ask in a terminal. The actual .desktop file is written by HelloWM.
set -eu

find_hellowm() {
    if [ -n "${HELLOWM_BIN:-}" ] && [ -x "$HELLOWM_BIN" ]; then printf '%s' "$HELLOWM_BIN"; return; fi
    command -v hellowm 2>/dev/null || [ ! -x "$HOME/.local/bin/hellowm" ] || printf '%s' "$HOME/.local/bin/hellowm"
}
HELLOWM=$(find_hellowm)
if [ -z "$HELLOWM" ]; then
    printf '%s\n' 'HelloWM binary not found on PATH.' >&2
    exit 1
fi
export HELLOWM

if command -v zenity >/dev/null 2>&1; then
    values=$(zenity --forms --title='Create desktop launcher' \
        --text='Create a new application launcher on the desktop.' \
        --add-entry='Launcher name' --add-entry='Command to run' --separator='|') || exit 0
    name=${values%%|*}
    command=${values#*|}
elif command -v kdialog >/dev/null 2>&1; then
    name=$(kdialog --inputbox 'Launcher name:' 'New launcher') || exit 0
    command=$(kdialog --inputbox 'Command to run:' '') || exit 0
else
    terminal=${TERMINAL:-}
    if [ -z "$terminal" ]; then
        if command -v x-terminal-emulator >/dev/null 2>&1; then terminal=x-terminal-emulator
        elif command -v alacritty >/dev/null 2>&1; then terminal=alacritty
        elif command -v xterm >/dev/null 2>&1; then terminal=xterm
        else
            printf '%s\n' 'HelloWM: install zenity/kdialog or a terminal emulator to create launchers.' >&2
            exit 1
        fi
    fi
    prompt_script=$(mktemp)
    cat > "$prompt_script" <<'PROMPT'
#!/bin/sh
printf 'Launcher name: '
IFS= read -r name || exit 1
printf 'Command to run: '
IFS= read -r command || exit 1
exec "$HELLOWM" --create-launcher "$name" "$command"
PROMPT
    chmod 700 "$prompt_script"
    # TERMINAL may include arguments (for example, 'alacritty --class hello').
    # This is a user-controlled desktop setting, so invoke it through the shell.
    sh -c "$terminal -e '$prompt_script'"
    rm -f "$prompt_script"
    exit 0
fi

exec "$HELLOWM" --create-launcher "$name" "$command"
