//! Small X11 desktop model: icon discovery, open/drag behavior, URI drops and
//! creation of freedesktop .desktop launchers. Rendering and Xdnd messages are
//! handled by the window-manager event loop in main.rs.

use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IconKind {
    Computer,
    Browser,
    Media,
    Picture,
    Folder,
    Launcher,
    File,
}

/// Fixed dimensions for a desktop icon's X11 child window, including label.
pub struct IconBounds;

impl IconBounds {
    // A square hit target makes selection, drag and right-click consistent.
    pub const WIDTH: i32 = 88;
    pub const HEIGHT: i32 = 88;
}

#[derive(Debug, Clone)]
pub struct DesktopIcon {
    pub path: PathBuf,
    pub name: String,
    pub x: i16,
    pub y: i16,
    pub kind: IconKind,
}

pub struct Desktop {
    pub directory: PathBuf,
    layout_file: PathBuf,
    pub icons: Vec<DesktopIcon>,
    /// Paths of all currently selected icons (supports rubber-band selection).
    pub selected: HashSet<PathBuf>,
    pub last_click: Option<(usize, Instant)>,
    pub dragging: Option<usize>,
    computer_hidden: bool,
}

impl Desktop {
    pub fn new() -> io::Result<Self> {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        let directory = home.join("Desktop");
        let config_directory = home.join(".config").join("hellowm");
        fs::create_dir_all(&directory)?;
        fs::create_dir_all(&config_directory)?;
        let mut desktop = Self {
            directory,
            layout_file: config_directory.join("desktop-layout.toml"),
            icons: Vec::new(),
            selected: HashSet::new(),
            last_click: None,
            dragging: None,
            computer_hidden: false,
        };
        desktop.reload()?;
        Ok(desktop)
    }

    pub fn reload(&mut self) -> io::Result<()> {
        let previous_positions = self.icons.iter()
            .map(|icon| (icon.path.clone(), (icon.x, icon.y)))
            .collect::<std::collections::HashMap<_, _>>();
        let saved_layout = fs::read_to_string(&self.layout_file)
            .ok()
            .and_then(|contents| toml::from_str::<SavedLayout>(&contents).ok())
            .unwrap_or_default();
        self.computer_hidden = saved_layout.computer_hidden;
        let saved_positions = saved_layout.positions;
        let dragged_path = self.dragging.and_then(|index| self.icons.get(index)).map(|icon| icon.path.clone());
        let mut paths = fs::read_dir(&self.directory)?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .collect::<Vec<_>>();
        paths.sort_by_key(|path| path.file_name().unwrap_or_default().to_os_string());

        let mut icons = Vec::with_capacity(paths.len() + 1);
        // A virtual Computer item is always present and opens HelloWM's native
        // About Computer window. Its URI-like path is only an in-memory ID.
        let computer_path = PathBuf::from("hellowm://computer");
        let previous_computer = previous_positions.get(&computer_path).map(|&(x, y)| [x, y]);
        let saved_computer = saved_positions.get(&computer_path.to_string_lossy().to_string()).copied();
        let [computer_x, computer_y] = saved_computer.or(previous_computer).unwrap_or([18, 18]);
        if !self.computer_hidden {
            icons.push(DesktopIcon {
                path: computer_path,
                name: "Computer".into(),
                x: computer_x,
                y: computer_y,
                kind: IconKind::Computer,
            });
        }

        for (index, path) in paths.into_iter().enumerate() {
            let name = display_name(&path);
            let kind = classify_icon(&path);
            let column = index / 8;
            let row = index % 8;
            let default_position = [
                18_i16.saturating_add((column as i16).saturating_mul(104)),
                112_i16.saturating_add((row as i16).saturating_mul(88)),
            ];
            let saved = saved_positions.get(&path.to_string_lossy().to_string()).copied();
            let previous = previous_positions.get(&path).map(|&(x, y)| [x, y]);
            let [x, y] = saved.or(previous).unwrap_or(default_position);
            icons.push(DesktopIcon { path, name, x, y, kind });
        }
        let valid_paths = icons.iter().map(|icon| icon.path.clone()).collect::<HashSet<_>>();
        self.selected.retain(|path| valid_paths.contains(path));
        self.dragging = dragged_path.and_then(|path| icons.iter().position(|icon| icon.path == path));
        self.icons = icons;
        Ok(())
    }

    pub fn hit_test(&self, x: i16, y: i16) -> Option<usize> {
        self.icons.iter().position(|icon| {
            let local_x = x as i32 - icon.x as i32;
            let local_y = y as i32 - icon.y as i32;
            (0..IconBounds::WIDTH).contains(&local_x)
                && (0..IconBounds::HEIGHT).contains(&local_y)
        })
    }

    pub fn move_icon(&mut self, index: usize, x: i16, y: i16) {
        if let Some(icon) = self.icons.get_mut(index) {
            icon.x = x.max(0);
            icon.y = y.max(0);
        }
    }

    pub fn save_layout(&self) -> io::Result<()> {
        let mut positions = BTreeMap::new();
        for icon in &self.icons {
            positions.insert(icon.path.to_string_lossy().to_string(), [icon.x, icon.y]);
        }
        let serialized = toml::to_string_pretty(&SavedLayout {
            positions,
            computer_hidden: self.computer_hidden,
        })
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        fs::write(&self.layout_file, serialized)
    }

    pub fn open_icon(&self, index: usize) {
        let Some(icon) = self.icons.get(index) else { return; };
        if icon.kind == IconKind::Computer {
            // Opened by the Rust/X11 front-end in main.rs.
            return;
        }
        if matches!(icon.kind, IconKind::Launcher | IconKind::Browser) {
            if let Some(exec) = desktop_exec(&icon.path) {
                spawn_command(&exec);
                return;
            }
        }
        let _ = Command::new("xdg-open").arg(&icon.path).spawn();
    }

    /// Remove a desktop item safely. Files and launchers are moved to the
    /// freedesktop Trash; the virtual Computer icon is hidden in the layout.
    pub fn remove_icon(&mut self, path: &Path) -> io::Result<()> {
        if path == Path::new("hellowm://computer") {
            self.computer_hidden = true;
            self.selected.remove(path);
            self.icons.retain(|icon| icon.path.as_path() != path);
            return self.save_layout();
        }
        if path.parent() != Some(self.directory.as_path()) {
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, "item is not directly inside the desktop directory"));
        }
        move_to_trash(path)?;
        self.selected.remove(path);
        self.reload()?;
        self.save_layout()
    }

    pub fn open_containing_folder(&self, path: &Path) {
        let folder = path.parent().unwrap_or(&self.directory);
        let _ = Command::new("xdg-open").arg(folder).spawn();
    }

    pub fn create_launcher(&self, name: &str, command: &str) -> io::Result<PathBuf> {
        let name = name.trim();
        let command = command.trim();
        if name.is_empty() || command.is_empty() {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "launcher name and command cannot be empty"));
        }

        let stem = safe_filename(name);
        let mut path = self.directory.join(format!("{stem}.desktop"));
        let mut suffix = 2_u32;
        while path.exists() {
            path = self.directory.join(format!("{stem}-{suffix}.desktop"));
            suffix += 1;
        }

        let escaped_name = name.replace('\\', "\\\\").replace('\n', " ").replace('\r', " ");
        let escaped_exec = command
            .replace('\\', "\\\\")
            .replace('\n', " ")
            .replace('\r', " ")
            .replace('%', "%%");
        let contents = format!(
            "[Desktop Entry]\nVersion=1.0\nType=Application\nName={escaped_name}\nExec={escaped_exec}\nIcon=application-x-executable\nTerminal=false\nCategories=Utility;\n"
        );
        fs::write(&path, contents)?;
        let mut permissions = fs::metadata(&path)?.permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&path, permissions)?;
        Ok(path)
    }

    /// Import file:// entries from an Xdnd text/uri-list selection.
    /// Files are copied into ~/Desktop and duplicates receive a numeric suffix.
    pub fn import_uri_list(&mut self, contents: &str) -> io::Result<usize> {
        let mut imported = 0;
        for line in contents.lines().map(str::trim) {
            if line.is_empty() || line.starts_with('#') || !line.starts_with("file://") {
                continue;
            }
            let Some(source) = file_uri_to_path(line) else { continue; };
            if !source.exists() || source == self.directory || source.starts_with(&self.directory) {
                continue;
            }
            let file_name = source.file_name().unwrap_or_default();
            if file_name.is_empty() {
                continue;
            }
            let destination = unique_destination(&self.directory, file_name);
            if copy_path(&source, &destination).is_ok() {
                imported += 1;
            }
        }
        if imported > 0 {
            self.reload()?;
        }
        Ok(imported)
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct SavedLayout {
    #[serde(default)]
    positions: BTreeMap<String, [i16; 2]>,
    #[serde(default)]
    computer_hidden: bool,
}

fn classify_icon(path: &Path) -> IconKind {
    if path.is_dir() {
        return IconKind::Folder;
    }
    let extension = path.extension().unwrap_or_default().to_string_lossy().to_ascii_lowercase();
    if extension == "desktop" {
        let contents = fs::read_to_string(path).unwrap_or_default().to_ascii_lowercase();
        if ["firefox", "chromium", "chrome", "brave", "vivaldi", "opera", "librewolf", "browser"]
            .iter().any(|needle| contents.contains(needle))
        {
            return IconKind::Browser;
        }
        return IconKind::Launcher;
    }
    if ["mp4", "mkv", "webm", "mov", "avi", "wmv", "flv", "m4v", "mpeg", "mpg", "ogv", "3gp", "ts", "mts", "m2ts"]
        .contains(&extension.as_str())
    {
        return IconKind::Media;
    }
    if ["png", "jpg", "jpeg", "gif", "bmp", "webp", "tif", "tiff", "svg", "ico", "avif", "heic", "heif"]
        .contains(&extension.as_str())
    {
        return IconKind::Picture;
    }
    IconKind::File
}

fn display_name(path: &Path) -> String {
    if path.extension().is_some_and(|extension| extension.to_string_lossy().eq_ignore_ascii_case("desktop")) {
        if let Ok(contents) = fs::read_to_string(path) {
            for line in contents.lines() {
                if let Some(value) = line.strip_prefix("Name=") {
                    return value.trim().to_string();
                }
            }
        }
    }
    path.file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string()
}

fn desktop_exec(path: &Path) -> Option<String> {
    let contents = fs::read_to_string(path).ok()?;
    let exec = contents.lines().find_map(|line| line.strip_prefix("Exec="))?;
    let cleaned = exec
        .split_whitespace()
        .filter(|part| !part.starts_with('%'))
        .collect::<Vec<_>>()
        .join(" ");
    if cleaned.trim().is_empty() { None } else { Some(cleaned) }
}

fn spawn_command(command: &str) {
    let _ = Command::new("sh").arg("-c").arg(command).spawn();
}

fn safe_filename(name: &str) -> String {
    let cleaned = name
        .chars()
        .map(|character| if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.') { character } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    if cleaned.is_empty() { "launcher".into() } else { cleaned }
}

fn unique_destination(directory: &Path, file_name: &std::ffi::OsStr) -> PathBuf {
    let original = PathBuf::from(file_name);
    let mut candidate = directory.join(&original);
    if !candidate.exists() {
        return candidate;
    }
    let stem = original.file_stem().unwrap_or_default().to_string_lossy();
    let extension = original.extension().map(|value| value.to_string_lossy().to_string());
    for index in 2_u32.. {
        let name = match &extension {
            Some(extension) => format!("{stem}-{index}.{extension}"),
            None => format!("{stem}-{index}"),
        };
        candidate = directory.join(name);
        if !candidate.exists() {
            return candidate;
        }
    }
    candidate
}

fn file_uri_to_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    let path = if let Some(local) = rest.strip_prefix("localhost/") {
        format!("/{local}")
    } else if rest.starts_with('/') {
        rest.to_string()
    } else {
        return None;
    };
    let bytes = path.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let Ok(value) = u8::from_str_radix(&path[index + 1..index + 3], 16) {
                decoded.push(value);
                index += 3;
                continue;
            }
        }
        decoded.push(bytes[index]);
        index += 1;
    }
    Some(PathBuf::from(std::ffi::OsString::from(String::from_utf8_lossy(&decoded).to_string())))
}

fn move_to_trash(path: &Path) -> io::Result<()> {
    // Prefer GLib's implementation when available; it handles mounts and
    // desktop-specific trash locations correctly.
    if let Ok(status) = Command::new("gio").arg("trash").arg(path).status() {
        if status.success() {
            return Ok(());
        }
    }

    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let data_home = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".local").join("share"));
    let trash = data_home.join("Trash");
    let files = trash.join("files");
    let info = trash.join("info");
    fs::create_dir_all(&files)?;
    fs::create_dir_all(&info)?;

    let original_name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
    let mut candidate = original_name.clone();
    let mut suffix = 2_u32;
    while files.join(&candidate).exists() || info.join(format!("{candidate}.trashinfo")).exists() {
        candidate = format!("{original_name}.{suffix}");
        suffix = suffix.saturating_add(1);
    }
    let destination = files.join(&candidate);
    match fs::rename(path, &destination) {
        Ok(()) => {},
        Err(error) if error.raw_os_error() == Some(18) => {
            copy_path(path, &destination)?;
            remove_path(path)?;
        }
        Err(error) => return Err(error),
    }

    let escaped_path = percent_encode_path(&path.to_string_lossy());
    let deletion_date = Command::new("date")
        .arg("-u")
        .arg("+%Y-%m-%dT%H:%M:%S")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "1970-01-01T00:00:00".into());
    let info_path = info.join(format!("{candidate}.trashinfo"));
    let info_contents = format!(
        "[Trash Info]\nPath={escaped_path}\nDeletionDate={deletion_date}\n"
    );
    if let Err(error) = fs::write(&info_path, info_contents) {
        let _ = fs::rename(&destination, path);
        return Err(error);
    }
    Ok(())
}

fn percent_encode_path(path: &str) -> String {
    let mut encoded = String::with_capacity(path.len());
    for byte in path.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'-' | b'_' | b'.' | b'~') {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

fn remove_path(path: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || metadata.is_file() {
        fs::remove_file(path)
    } else if metadata.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}

fn copy_path(source: &Path, destination: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(source)?;
    if metadata.file_type().is_symlink() {
        let target = fs::read_link(source)?;
        #[cfg(unix)]
        std::os::unix::fs::symlink(target, destination)?;
        #[cfg(not(unix))]
        return Err(io::Error::new(io::ErrorKind::Unsupported, "symbolic-link import is unsupported"));
    } else if metadata.is_dir() {
        fs::create_dir(destination)?;
        for entry in fs::read_dir(source)? {
            let entry = entry?;
            copy_path(&entry.path(), &destination.join(entry.file_name()))?;
        }
    } else {
        fs::copy(source, destination)?;
    }
    Ok(())
}
