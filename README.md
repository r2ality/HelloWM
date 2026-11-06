# HelloWM

HelloWM is a deliberately small, floating-only X11 window manager written in Rust and Shell. mimicking Windows 10 mainly.

Just for fun, not a serious WM, but usable and daily drivable.

## What is included

- Floating X11 windows with pointer-enter focus, a task list, a thin focus border, and raise-on-focus. Client windows are not wrapped in server-side title bars in this first cut.
- `Super+Enter` launches `alacritty` by default.
- `Super+Q` sends `WM_DELETE_WINDOW` to the focused app, falling back to `XKillClient` when the app does not advertise that protocol.
- `Super+left-drag` moves a window; `Super+right-drag` resizes it.
- A flat, left-aligned Windows 10-inspired taskbar: full-height Start button, rectangular `Type here to search` field, compact app buttons with running-state indicators, and an icon-only network area.
- Clicking the Start button or search field runs `rofi -show drun`, falling back to `dmenu_run` when rofi is unavailable. If only the `dmenu` binary is installed, HelloWM feeds it executable names from `PATH` for filtering. If no launcher is installed, the search field becomes an inline shell-command prompt; type a command and press Enter.
- `Super+D` focuses the taskbar command field so you can start typing immediately. When rofi/dmenu is installed, clicking the field delegates filtering/autocomplete to that launcher.
- A simple desktop surface inherits the X11 root wallpaper, shows files from `~/Desktop`, opens icons on double-click, allows icons to be repositioned by dragging, and supports Xdnd `text/uri-list` file drops from compatible file managers. Right-click an empty desktop area to create a launcher (using Zenity or KDialog when present, with terminal prompt fallback). Middle-click an empty desktop area to open About computer.
- The Start button accepts a custom image via `start_image`; the default is `~/.config/assets/windows.png`. Common raster images (PNG, JPEG, BMP, GIF, WebP, TIFF, and ICO) are scaled down, preserving aspect ratio, to fit a 30x30 box. The installer places a starter Windows-style PNG there only if the file does not already exist.
- `Super+Shift+F1` opens the native X11 **About computer** dialog. Its text is supplied by `scripts/about-computer.sh`, which uses basic `/etc/os-release`, `/proc`, `grep`, and `uname` queries.
- Right-clicking the taskbar switches between light and dark mode and saves the choice.
- Small Wi-Fi and wired indicators read NetworkManager state through `nmcli`.
- A command-widget area. The default VPN widget appears only when an active NetworkManager VPN connection is detected; additional widgets are configured in TOML.
- A standard XEmbed system-tray host. Compatible tray applets can dock themselves; HelloWM does not require a fixed list of applets.
- A LightDM session entry and a `startx` `.xinitrc` example.

HelloWM does not set a wallpaper. Use `feh` or `xwallpaper` yourself, as shown in the examples.

## Build

HelloWM needs an X11 session and Rust/Cargo. On NixOS, one way to get a temporary build environment is:

```sh
nix shell nixpkgs#rustc nixpkgs#cargo
```

Then, from this directory:

```sh
cargo build --release
```

The resulting binary is `target/release/hellowm`. Put it somewhere on your `PATH`, for example:

```sh
mkdir -p "$HOME/.local/bin"
install -m755 target/release/hellowm "$HOME/.local/bin/hellowm"
```

The panel uses the X11 core font named `fixed`. On distributions where that font is not installed, install the basic Xorg misc fonts package.

## Install

After extracting the source, run the installer as your regular user (not with `sudo`):

```sh
./install.sh
```

The installer runs `cargo build --release` when Cargo is available. If Cargo is not installed, it uses an existing `target/release/hellowm` binary. It creates `~/.config/hellowm`, installs the example config only when no config exists, and creates `~/.xinitrc` only when you do not already have one.

On generic Linux distributions (Ubuntu, Debian, Mint, Fedora, Arch, openSUSE, etc.) it installs the binary to `/usr/local/bin/hellowm` and the display-manager session to `/usr/share/xsessions/hellowm.desktop`. It uses `sudo` only for those system-wide files.

On NixOS it does not try to write into the immutable `/usr` tree. It installs the binary to `~/.local/bin/hellowm` and generates `~/.config/hellowm/nixos-session.nix`. Add that file to the existing `imports` list in `/etc/nixos/configuration.nix` using its absolute path (or copy it into `/etc/nixos` and import it relatively). The generated session package declares `providedSessions` for the NixOS display-manager module. Then run:

```sh
sudo nixos-rebuild switch
```

NixOS makes custom display-manager sessions available declaratively through `services.displayManager.sessionPackages`; the generated module uses that mechanism. If your configuration already has an `imports` list, add the module path to that list rather than declaring a second `imports` attribute.

The generic installer uses `/usr/local/bin` and `/usr/share/xsessions` by default. You can override the paths when installing, for example:

```sh
PREFIX=/opt/hellowm ./install.sh
# or set BINDIR and XSESSIONDIR explicitly
```

## Uninstall

Remove the program and session entry while keeping your personal settings:

```sh
./uninstall.sh
```

To also delete `~/.config/hellowm`:

```sh
./uninstall.sh --purge-config
```

On NixOS, remove the generated module from your NixOS configuration's `imports` list and run `sudo nixos-rebuild switch` before running `uninstall.sh`. The uninstaller checks `/etc/nixos` for a remaining import and refuses to remove the module if it is still referenced.

## Start with startx

The installer creates a minimal `~/.xinitrc` if one does not already exist. It leaves an existing file untouched. Add your wallpaper command above the final `exec` line, for example:

```sh
xwallpaper --zoom "$HOME/Pictures/wallpaper.jpg"
# or: feh --bg-fill "$HOME/Pictures/wallpaper.jpg"
exec hellowm
```

If you prefer to create the file yourself, `examples/xinitrc` is provided. Do not put commands after the `exec` line.

## Start with LightDM

Install the session file in LightDM's X session directory (usually `/usr/share/xsessions`):

```sh
sudo install -Dm644 share/xsessions/hellowm.desktop /usr/share/xsessions/hellowm.desktop
```

Make sure `hellowm` is on the session's `PATH`, log out, and select **HelloWM** from LightDM's session selector.

For a wallpaper with LightDM, put the relevant `feh` or `xwallpaper` command in `~/.xprofile` (or use the supplied `examples/xprofile` as a starting point). LightDM does not normally use `~/.xinitrc` for the selected session.

### NixOS LightDM example

After copying the compiled binary to `~/.local/bin/hellowm`, import `examples/nixos-lightdm.nix` from your NixOS configuration. It installs a small PATH wrapper plus the HelloWM session file using `services.displayManager.sessionPackages`. Adjust the wrapper if you install the binary somewhere else. Then rebuild the system and choose HelloWM in LightDM.

This repository intentionally avoids a pre-filled Nix `cargoHash`: it would be a placeholder until Cargo has resolved the exact dependency graph for your nixpkgs revision.

## Configuration

On first launch HelloWM creates:

```text
~/.config/hellowm/config.toml
```

Example:

```toml
terminal = "alacritty"
search_command = "rofi -show drun"
fallback_search_command = "dmenu_run"
panel_height = 56
search_width = 300 # width of the taskbar search field in pixels
start_icon = "windows" # fallback: also "hello" or a short ASCII label
start_image = "~/.config/assets/windows.png"
about_script = "~/.local/libexec/hellowm/about-computer.sh"
launcher_creator = "~/.local/libexec/hellowm/create-launcher.sh"
desktop_enabled = true
theme = "dark"
# Optional commands launched once at session start:
# autostart = ["nm-applet"]

[[keybinds]]
chord = "Super+Return"
action = "terminal"

[[keybinds]]
chord = "Super+q"
action = "close"

[[keybinds]]
chord = "Super+d"
action = "focus-search"

[[keybinds]]
chord = "Super+Shift+F1"
action = "about"

[[widgets]]
name = "VPN"
command = "nmcli -t -f TYPE connection show --active | grep -q '^vpn:' && printf 'VPN' || true"
interval_seconds = 4

```

Available keybind modifiers: `Super`, `Ctrl`, `Alt`, and `Shift`. Supported key names include `Return`, `Escape`, `Tab`, `Space`, the arrow keys, `F1`–`F12`, and single-character keys. Supported actions include `terminal`, `close`, `search`, `focus-search`, `about`, `quit`, or `exec:COMMAND`.

A command widget runs at its configured interval. The first non-empty line of stdout is displayed; empty stdout hides that widget. This is intentionally simple so you can add status text without changing the panel code. For a more involved custom widget, add a renderer in `src/main.rs` and its settings type in `src/config.rs`.

To use a custom taskbar Start image, set `start_image` in `config.toml`; the default is `~/.config/assets/windows.png`. Common raster formats (PNG, JPEG, BMP, GIF, WebP, TIFF, and ICO) are scaled down, preserving their aspect ratio, to fit a 30x30-pixel area (smaller images are not enlarged). Set `start_image = ""` to use the `start_icon` fallback (`windows`, `hello`, or a short ASCII label). Desktop icon positions are saved in `~/.config/hellowm/desktop-layout.toml`.

## Tray and network notes

The tray host implements the freedesktop XEmbed docking flow. It works with XEmbed-compatible applets (for example, legacy tray-mode applets). Some newer applications expose only StatusNotifierItem over D-Bus; that protocol is not implemented in this first version, so those icons need an external SNI-to-XEmbed bridge.

The built-in network indicators depend on NetworkManager's `nmcli`. If `nmcli` is not available, the indicators are simply omitted. The VPN text widget uses the same NetworkManager CLI and can be replaced with any shell command.

## Starting HelloWM from an existing desktop

HelloWM must be the window manager for its X11 session. It cannot take over an X server where another window manager (for example, Mutter, KWin, Xfwm, or Openbox) already owns the root window's `SubstructureRedirect` event. If HelloWM reports X11 `BadAccess` while changing root-window attributes, log out and select HelloWM in the display manager, or switch to a fresh virtual terminal and run `startx`. Do not launch it from a terminal inside another desktop session.

## Deliberate limits in this first version

- X11 only; this is not a Wayland compositor.
- One desktop, no workspaces.
- No wallpaper handling.
- Rofi/dmenu are optional. Without them, the taskbar command prompt runs typed commands.
- Desktop icons are intentionally basic: there is no icon theme support or graphical properties dialog in this cut.
- No compositor, shadows, animations, or transparency.
- Basic client handling and EWMH properties only; this is a small personal WM, not a complete replacement for a mature desktop environment.

### Native desktop icon windows

Desktop icons are represented by individual X11 child windows, following the retained-item model used by XFCE and Nemo. Dragging moves the icon window rather than repainting the icon at each pointer coordinate. After each move, HelloWM explicitly clears the vacated rectangle on the desktop parent so the parent's wallpaper background is repainted even on X servers that do not promptly expose that area for an override-redirect child. Icon labels use transparent `PolyText8` drawing, and each item has its own selection and expose handling. The desktop parent uses the wallpaper pixmap from `_XROOTPMAP_ID` / `ESETROOT_PMAP_ID` when the wallpaper setter publishes it; otherwise it inherits the root background. Pointer-motion batches are coalesced so only the latest position is sent to the X server, and the clear request is asynchronous to avoid a round trip during dragging.
