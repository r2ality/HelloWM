#!/bin/sh
# Basic, dependency-light system summary for HelloWM's About Computer dialog.

read_os_name() {
    if [ -r /etc/os-release ]; then
        grep '^PRETTY_NAME=' /etc/os-release | head -n 1 | cut -d= -f2- | tr -d '"'
    fi
}

read_cpu() {
    grep -m 1 -E '^(model name|Hardware)[[:space:]]*:' /proc/cpuinfo 2>/dev/null | cut -d: -f2- | sed 's/^[[:space:]]*//'
}

read_memory() {
    grep '^MemTotal:' /proc/meminfo 2>/dev/null | awk '{ printf "%.1f GiB", $2 / 1048576 }'
}

read_desktop_session() {
    printf '%s' "${XDG_CURRENT_DESKTOP:-${DESKTOP_SESSION:-HelloWM}}"
}

OS_NAME=$(read_os_name)
CPU_NAME=$(read_cpu)
MEMORY=$(read_memory)
[ -n "$OS_NAME" ] || OS_NAME="Linux"
[ -n "$CPU_NAME" ] || CPU_NAME="Unknown CPU"
[ -n "$MEMORY" ] || MEMORY="Unknown"

printf 'Operating system: %s\n' "$OS_NAME"
printf 'Kernel: %s\n' "$(uname -sr 2>/dev/null || printf Unknown)"
printf 'Architecture: %s\n' "$(uname -m 2>/dev/null || printf Unknown)"
printf 'Processor: %s\n' "$CPU_NAME"
printf 'Memory: %s\n' "$MEMORY"
printf 'Desktop: %s\n' "$(read_desktop_session)"
printf 'Shell: %s\n' "${SHELL:-Unknown}"
printf 'Hostname: %s\n' "$(hostname 2>/dev/null || printf Unknown)"
