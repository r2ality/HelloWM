//! Runtime configuration and the small, intentionally hackable widget API.
//!
//! Most day-to-day settings live in ~/.config/hellowm/config.toml.  To add a
//! new kind of widget in Rust, extend WidgetConfig or add a renderer in
//! main.rs.  Command widgets are useful for quick personal additions without
//! recompiling the window manager.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Terminal command launched by Super+Enter.
    pub terminal: String,
    /// Main launcher.  The default falls back to dmenu_run if rofi is absent.
    pub search_command: String,
    pub fallback_search_command: String,
    /// Bottom panel height in pixels.
    pub panel_height: u16,
    /// Width of the rectangular taskbar search field in pixels.
    pub search_width: u16,
    /// X11 core font name. HelloWM falls back to "fixed" when unavailable.
    pub font_name: String,
    /// Shell command that prints two clock lines (time, then day/date).
    /// Set to an empty string to hide the clock.
    pub clock_command: String,
    /// Screenshot command; defaults to shotgun.
    pub screenshot_command: String,
    /// "windows", "hello", or any short ASCII label (fallback when no image loads).
    pub start_icon: String,
    /// Image path used for the Start button. Supports a leading ~/ .
    pub start_image: String,
    /// Script that returns system details for the About Computer window.
    pub about_script: String,
    /// Helper that asks for a name/command and creates a desktop launcher.
    pub launcher_creator: String,
    /// Show a simple desktop surface with icons from ~/Desktop.
    pub desktop_enabled: bool,
    /// Initial theme. Right-clicking the panel switches between light and dark.
    pub theme: String,
    /// Optional commands started once when HelloWM starts.
    pub autostart: Vec<String>,
    /// Global key chords. Actions: terminal, close, search, focus-search, about, quit, or exec:COMMAND.
    pub keybinds: Vec<KeybindConfig>,
    /// Small text widgets rendered before the network indicators and tray.
    pub widgets: Vec<WidgetConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct KeybindConfig {
    /// Example: "Super+Return", "Super+q", "Alt+F4".
    pub chord: String,
    /// See Config::keybinds documentation.
    pub action: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WidgetConfig {
    /// A short name used in logs and as a fallback display value.
    pub name: String,
    /// Shell command; trimmed stdout is displayed when it is non-empty.
    pub command: String,
    /// How often the command is run. Values below one second become one second.
    pub interval_seconds: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            terminal: "alacritty".into(),
            search_command: "rofi -show drun".into(),
            fallback_search_command: "dmenu_run".into(),
            panel_height: 56,
            search_width: 300,
            font_name: "9x15".into(),
            clock_command: "date '+%H:%M'; date '+%a, %d %b'".into(),
            screenshot_command: "shotgun".into(),
            start_icon: "windows".into(),
            start_image: "~/.config/assets/windows.png".into(),
            about_script: "~/.local/libexec/hellowm/about-computer.sh".into(),
            launcher_creator: "~/.local/libexec/hellowm/create-launcher.sh".into(),
            desktop_enabled: true,
            theme: "dark".into(),
            keybinds: vec![
                KeybindConfig {
                    chord: "Super+Return".into(),
                    action: "terminal".into(),
                },
                KeybindConfig {
                    chord: "Super+q".into(),
                    action: "close".into(),
                },
                KeybindConfig {
                    chord: "Super+d".into(),
                    action: "focus-search".into(),
                },
                KeybindConfig {
                    chord: "Super+Shift+F1".into(),
                    action: "about".into(),
                },
                KeybindConfig {
                    chord: "Super+r".into(),
                    action: "reload".into(),
                },
                KeybindConfig {
                    chord: "Super+s".into(),
                    action: "screenshot".into(),
                },
                KeybindConfig {
                    chord: "Super+Up".into(),
                    action: "maximize".into(),
                },
                KeybindConfig {
                    chord: "Super+Down".into(),
                    action: "minimize".into(),
                },
            ],
            widgets: vec![WidgetConfig {
                name: "VPN".into(),
                command: "nmcli -t -f TYPE connection show --active | grep -q '^vpn:' && printf 'VPN' || true".into(),
                interval_seconds: 4,
            }],
            autostart: Vec::new(),
        }
    }
}

impl Default for KeybindConfig {
    fn default() -> Self {
        Self {
            chord: String::new(),
            action: String::new(),
        }
    }
}

impl Default for WidgetConfig {
    fn default() -> Self {
        Self {
            name: String::new(),
            command: String::new(),
            interval_seconds: 5,
        }
    }
}

pub fn config_path() -> PathBuf {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join(".config").join("hellowm").join("config.toml")
}

pub fn load() -> Config {
    let path = config_path();
    if let Some(parent) = path.parent() {
        if let Err(error) = fs::create_dir_all(parent) {
            eprintln!("HelloWM: could not create config directory: {error}");
        }
    }

    if !path.exists() {
        let config = Config::default();
        if let Err(error) = save(&config) {
            eprintln!("HelloWM: could not write default config: {error}");
        }
        return config;
    }

    match fs::read_to_string(&path) {
        Ok(contents) => match toml::from_str::<Config>(&contents) {
            Ok(config) => config,
            Err(error) => {
                eprintln!("HelloWM: invalid config at {}: {error}; using defaults", path.display());
                Config::default()
            }
        },
        Err(error) => {
            eprintln!("HelloWM: could not read {}: {error}; using defaults", path.display());
            Config::default()
        }
    }
}

pub fn save(config: &Config) -> Result<(), Box<dyn std::error::Error>> {
    let path = config_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, toml::to_string_pretty(config)?)?;
    Ok(())
}

/// Expand a leading tilde without invoking a shell.
pub fn expand_home(path: &str) -> PathBuf {
    if path == "~" {
        return std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    }
    if let Some(rest) = path.strip_prefix("~/") {
        let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
        return home.join(rest);
    }
    PathBuf::from(path)
}
