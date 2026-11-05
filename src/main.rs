mod config;
mod desktop;

use config::{Config, KeybindConfig, WidgetConfig};
use desktop::{Desktop, IconKind};
use std::error::Error;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{
    Atom, AtomEnum, BackPixmap, ButtonIndex, ChangeGCAux, ChangeWindowAttributesAux, ClientMessageEvent, ImageFormat,
    ConfigWindow, CoordMode, CreateGCAux, CreateWindowAux, EventMask, Gcontext, GrabMode,
    InputFocus, KeyButMask, MapState, ModMask, Pixmap, Point, PropMode, Rectangle, StackMode, Window, GrabStatus,
    WindowClass,
};
use x11rb::{CURRENT_TIME, NONE};
use x11rb::protocol::xproto::ConnectionExt as XprotoExt;
use x11rb::protocol::Event;
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as WrapperExt;

const START_BUTTON_WIDTH: i32 = 60;
const PANEL_PADDING: i32 = 10;
const TRAY_ICON_SIZE: i32 = 22;
const NETWORK_ICON_WIDTH: i32 = 26;
const CLIENT_BORDER: u32 = 2;

#[derive(Clone, Copy)]
struct Palette {
    panel: u32,
    surface: u32,
    search: u32,
    search_text: u32,
    text: u32,
    muted: u32,
    accent: u32,
    border: u32,
}

#[derive(Default, Clone, Copy)]
struct NetworkState {
    wifi_present: bool,
    wifi_connected: bool,
    wired_present: bool,
    wired_connected: bool,
}

struct WidgetRuntime {
    spec: WidgetConfig,
    value: String,
    last_run: Instant,
}

struct StartImage {
    width: u16,
    height: u16,
    data: Vec<u8>,
}

struct AboutWindow {
    window: Window,
    width: u16,
    height: u16,
    lines: Vec<String>,
}

#[derive(Clone)]
struct BoundKey {
    keycode: u8,
    modifiers: ModMask,
    action: String,
}

#[derive(Clone, Copy)]
enum DragKind {
    Move,
    Resize,
}

struct DragState {
    window: Window,
    kind: DragKind,
    start_root_x: i16,
    start_root_y: i16,
    start_x: i16,
    start_y: i16,
    start_width: u16,
    start_height: u16,
}

struct Atoms {
    net_supported: Atom,
    net_supporting_wm_check: Atom,
    net_wm_name: Atom,
    utf8_string: Atom,
    net_client_list: Atom,
    net_active_window: Atom,
    net_wm_window_type: Atom,
    net_wm_window_type_dock: Atom,
    net_wm_strut: Atom,
    net_wm_strut_partial: Atom,
    wm_protocols: Atom,
    wm_delete_window: Atom,
    wm_name: Atom,
    tray_selection: Atom,
    tray_opcode: Atom,
    xembed: Atom,
    manager: Atom,
    net_close_window: Atom,
    xdnd_aware: Atom,
    xdnd_enter: Atom,
    xdnd_position: Atom,
    xdnd_status: Atom,
    xdnd_drop: Atom,
    xdnd_finished: Atom,
    xdnd_selection: Atom,
    xdnd_type_list: Atom,
    xdnd_action_copy: Atom,
    text_uri_list: Atom,
    xrootpmap_id: Atom,
    esetroot_pmap_id: Atom,
}

impl Atoms {
    fn new(conn: &RustConnection, screen_num: usize) -> Result<Self, Box<dyn Error>> {
        Ok(Self {
            net_supported: intern_atom(conn, "_NET_SUPPORTED")?,
            net_supporting_wm_check: intern_atom(conn, "_NET_SUPPORTING_WM_CHECK")?,
            net_wm_name: intern_atom(conn, "_NET_WM_NAME")?,
            utf8_string: intern_atom(conn, "UTF8_STRING")?,
            net_client_list: intern_atom(conn, "_NET_CLIENT_LIST")?,
            net_active_window: intern_atom(conn, "_NET_ACTIVE_WINDOW")?,
            net_wm_window_type: intern_atom(conn, "_NET_WM_WINDOW_TYPE")?,
            net_wm_window_type_dock: intern_atom(conn, "_NET_WM_WINDOW_TYPE_DOCK")?,
            net_wm_strut: intern_atom(conn, "_NET_WM_STRUT")?,
            net_wm_strut_partial: intern_atom(conn, "_NET_WM_STRUT_PARTIAL")?,
            wm_protocols: intern_atom(conn, "WM_PROTOCOLS")?,
            wm_delete_window: intern_atom(conn, "WM_DELETE_WINDOW")?,
            wm_name: intern_atom(conn, "WM_NAME")?,
            tray_selection: intern_atom(conn, &format!("_NET_SYSTEM_TRAY_S{screen_num}"))?,
            tray_opcode: intern_atom(conn, "_NET_SYSTEM_TRAY_OPCODE")?,
            xembed: intern_atom(conn, "_XEMBED")?,
            manager: intern_atom(conn, "MANAGER")?,
            net_close_window: intern_atom(conn, "_NET_CLOSE_WINDOW")?,
            xdnd_aware: intern_atom(conn, "XdndAware")?,
            xdnd_enter: intern_atom(conn, "XdndEnter")?,
            xdnd_position: intern_atom(conn, "XdndPosition")?,
            xdnd_status: intern_atom(conn, "XdndStatus")?,
            xdnd_drop: intern_atom(conn, "XdndDrop")?,
            xdnd_finished: intern_atom(conn, "XdndFinished")?,
            xdnd_selection: intern_atom(conn, "XdndSelection")?,
            xdnd_type_list: intern_atom(conn, "XdndTypeList")?,
            xdnd_action_copy: intern_atom(conn, "XdndActionCopy")?,
            text_uri_list: intern_atom(conn, "text/uri-list")?,
            xrootpmap_id: intern_atom(conn, "_XROOTPMAP_ID")?,
            esetroot_pmap_id: intern_atom(conn, "ESETROOT_PMAP_ID")?,
        })
    }
}

struct Manager {
    conn: RustConnection,
    root: Window,
    panel: Window,
    desktop_window: Option<Window>,
    desktop_root_pixmap: Option<Pixmap>,
    desktop_icon_windows: Vec<(PathBuf, Window)>,
    desktop: Option<Desktop>,
    tray_owner: Window,
    gc: Gcontext,
    atoms: Atoms,
    config: Config,
    palette: Palette,
    screen_width: u16,
    screen_height: u16,
    screen_num: usize,
    screen_depth: u8,
    screen_visual: u32,
    panel_height: u16,
    search_width: i32,
    start_image: Option<StartImage>,
    search_active: bool,
    search_text: String,
    about_window: Option<AboutWindow>,
    xdnd_source: Option<Window>,
    xdnd_accept_uri: bool,
    desktop_drag_start: Option<(usize, i16, i16, i16, i16)>,
    last_desktop_scan: Instant,
    clients: Vec<Window>,
    active: Option<Window>,
    task_hits: Vec<(i16, i16, Window)>,
    tray_windows: Vec<Window>,
    tray_enabled: bool,
    network: NetworkState,
    last_network_poll: Instant,
    widgets: Vec<WidgetRuntime>,
    keybinds: Vec<BoundKey>,
    drag: Option<DragState>,
    running: bool,
}

impl Manager {
    fn new() -> Result<Self, Box<dyn Error>> {
        let mut config = config::load();
        config.panel_height = config.panel_height.clamp(50, 80);
        config.search_width = config.search_width.clamp(180, 420);
        for (chord, action) in [("Super+d", "focus-search"), ("Super+Shift+F1", "about")] {
            if !config.keybinds.iter().any(|binding| binding.chord.eq_ignore_ascii_case(chord)) {
                config.keybinds.push(KeybindConfig { chord: chord.into(), action: action.into() });
            }
        }

        let (conn, screen_num) = x11rb::connect(None)?;
        let screen = &conn.setup().roots[screen_num];
        let root = screen.root;
        let screen_width = screen.width_in_pixels;
        let screen_height = screen.height_in_pixels;
        let screen_depth = screen.root_depth;
        let screen_visual = screen.root_visual;
        let panel_height = config.panel_height.min(screen_height.saturating_sub(1));
        let search_width = config.search_width as i32;
        let atoms = Atoms::new(&conn, screen_num)?;

        // Claim SubstructureRedirect before creating the panel.  If another WM
        // is active, the X server returns BadAccess here and HelloWM exits.
        conn.change_window_attributes(
            root,
            &ChangeWindowAttributesAux::new().event_mask(
                EventMask::SUBSTRUCTURE_REDIRECT
                    | EventMask::SUBSTRUCTURE_NOTIFY
                    | EventMask::PROPERTY_CHANGE,
            ),
        )?
        .check()
        .map_err(|error| {
            std::io::Error::new(
                std::io::ErrorKind::Other,
                format!(
                    "HelloWM could not take control of the X11 root window ({error}).\n\
                     This usually means another window manager is already running.\n\
                     Log out and select HelloWM at your display manager, or start it\n\
                     from a fresh virtual terminal with 'startx'. Do not launch it\n\
                     from inside an existing desktop session."
                ),
            )
        })?;

        let initial_palette = allocate_palette(&conn, screen.default_colormap, &config.theme)?;
        let start_image = load_start_image(&conn, screen, &config.start_image, &config.theme);

        let mut desktop_state = None;
        let mut desktop_window = None;
        if config.desktop_enabled {
            match Desktop::new() {
                Ok(desktop_model) => {
                    let window = conn.generate_id()?;
                    let desktop_height = screen_height.saturating_sub(panel_height).max(1);
                    conn.create_window(
                        screen.root_depth,
                        window,
                        root,
                        0,
                        0,
                        screen_width,
                        desktop_height,
                        0,
                        WindowClass::INPUT_OUTPUT,
                        screen.root_visual,
                        &CreateWindowAux::new()
                            .background_pixmap(BackPixmap::PARENT_RELATIVE)
                            .override_redirect(1u32)
                            .event_mask(
                                EventMask::EXPOSURE
                                    | EventMask::BUTTON_PRESS
                                    | EventMask::BUTTON_RELEASE
                                    | EventMask::STRUCTURE_NOTIFY,
                            ),
                    )?.check()?;
                    conn.change_property32(
                        PropMode::REPLACE,
                        window,
                        atoms.xdnd_aware,
                        AtomEnum::ATOM,
                        &[5],
                    )?.check()?;
                    conn.map_window(window)?.check()?;
                    conn.configure_window(
                        window,
                        &x11rb::protocol::xproto::ConfigureWindowAux::new().stack_mode(StackMode::BELOW),
                    )?.check()?;
                    desktop_state = Some(desktop_model);
                    desktop_window = Some(window);
                }
                Err(error) => eprintln!("HelloWM: desktop disabled: could not read ~/Desktop: {error}"),
            }
        }

        let panel = conn.generate_id()?;
        let panel_y = screen_height.saturating_sub(panel_height) as i16;
        conn.create_window(
            screen.root_depth,
            panel,
            root,
            0,
            panel_y,
            screen_width,
            panel_height,
            0,
            WindowClass::INPUT_OUTPUT,
            screen.root_visual,
            &CreateWindowAux::new()
                .background_pixel(initial_palette.panel)
                .border_pixel(initial_palette.panel)
                .override_redirect(1u32)
                .event_mask(
                    EventMask::EXPOSURE
                        | EventMask::BUTTON_PRESS
                        | EventMask::KEY_PRESS
                        | EventMask::SUBSTRUCTURE_NOTIFY
                        | EventMask::STRUCTURE_NOTIFY,
                ),
        )?
        .check()?;

        // The selection owner is intentionally separate from the visible panel,
        // as expected by the freedesktop system-tray protocol. It stays unmapped.
        let tray_owner = conn.generate_id()?;
        conn.create_window(
            screen.root_depth,
            tray_owner,
            root,
            -1,
            -1,
            1,
            1,
            0,
            WindowClass::INPUT_OUTPUT,
            screen.root_visual,
            &CreateWindowAux::new()
                .background_pixel(initial_palette.panel)
                .override_redirect(1u32),
        )?
        .check()?;

        let gc = conn.generate_id()?;
        conn.create_gc(
            gc,
            panel,
            &CreateGCAux::new()
                .foreground(initial_palette.text)
                .background(initial_palette.panel)
                .graphics_exposures(0u32),
        )?
        .check()?;

        let font = conn.generate_id()?;
        conn.open_font(font, b"fixed")?.check()?;
        conn.change_gc(gc, &ChangeGCAux::new().font(font))?.check()?;

        let desktop_root_pixmap = read_root_pixmap(
            &conn,
            root,
            atoms.xrootpmap_id,
            atoms.esetroot_pmap_id,
        );

        let mut manager = Self {
            conn,
            root,
            panel,
            desktop_window,
            desktop_root_pixmap,
            desktop_icon_windows: Vec::new(),
            desktop: desktop_state,
            tray_owner,
            gc,
            atoms,
            config,
            palette: initial_palette,
            screen_width,
            screen_height,
            screen_num,
            screen_depth,
            screen_visual,
            panel_height,
            search_width,
            start_image,
            search_active: false,
            search_text: String::new(),
            about_window: None,
            xdnd_source: None,
            xdnd_accept_uri: false,
            desktop_drag_start: None,
            last_desktop_scan: Instant::now(),
            clients: Vec::new(),
            active: None,
            task_hits: Vec::new(),
            tray_windows: Vec::new(),
            tray_enabled: false,
            network: NetworkState::default(),
            last_network_poll: Instant::now() - Duration::from_secs(10),
            widgets: Vec::new(),
            keybinds: Vec::new(),
            drag: None,
            running: true,
        };

        manager.claim_wm_selection(screen_num)?;
        manager.claim_tray_selection()?;
        manager.set_ewmh_properties()?;
        manager.set_panel_properties()?;
        manager.load_keybinds()?;
        manager.init_widgets();
        manager.conn.map_window(manager.panel)?.check()?;
        manager.apply_desktop_background()?;
        manager.draw_desktop()?;

        // Adopt already-mapped top-level windows.  The panel is skipped because
        // it is override-redirect and is not a normal client window.
        let existing = manager.conn.query_tree(manager.root)?.reply()?.children;
        for window in existing {
            if window == manager.panel || manager.desktop_window == Some(window) {
                continue;
            }
            // Finish the X11 attribute request before mutably borrowing the
            // manager to add the window. Keeping this in a separate binding
            // also ensures the reply cookie's connection borrow has ended.
            let should_manage = manager
                .conn
                .get_window_attributes(window)?
                .reply()
                .map(|attrs| {
                    attrs.map_state != MapState::UNMAPPED && !attrs.override_redirect
                })
                .unwrap_or(false);

            if should_manage {
                let _ = manager.manage_window(window);
            }
        }

        manager.refresh_external_state(true);
        manager.draw_panel()?;
        manager.conn.flush()?;

        for command in manager.config.autostart.clone() {
            spawn_shell(&command);
        }

        Ok(manager)
    }

    fn claim_wm_selection(&self, screen_num: usize) -> Result<(), Box<dyn Error>> {
        let selection = intern_atom(&self.conn, &format!("WM_S{screen_num}"))?;
        self.conn
            .set_selection_owner(self.panel, selection, CURRENT_TIME)?
            .check()?;
        let owner = self.conn.get_selection_owner(selection)?.reply()?.owner;
        if owner != self.panel {
            return Err("another window manager already owns WM_Sn".into());
        }
        Ok(())
    }

    fn claim_tray_selection(&mut self) -> Result<(), Box<dyn Error>> {
        let owner = self
            .conn
            .get_selection_owner(self.atoms.tray_selection)?
            .reply()?
            .owner;
        if owner != NONE && owner != self.tray_owner {
            eprintln!("HelloWM: another system tray already owns this screen; tray hosting disabled");
            self.tray_enabled = false;
            return Ok(());
        }

        self.conn
            .set_selection_owner(self.tray_owner, self.atoms.tray_selection, CURRENT_TIME)?
            .check()?;
        let owner = self
            .conn
            .get_selection_owner(self.atoms.tray_selection)?
            .reply()?
            .owner;
        if owner == self.tray_owner {
            self.tray_enabled = true;
            let event = ClientMessageEvent::new(
                32,
                self.root,
                self.atoms.manager,
                [
                    CURRENT_TIME,
                    self.atoms.tray_selection,
                    self.tray_owner,
                    0,
                    0,
                ],
            );
            self.conn.send_event(
                false,
                self.root,
                EventMask::STRUCTURE_NOTIFY | EventMask::SUBSTRUCTURE_NOTIFY,
                event,
            )?;
        }
        Ok(())
    }

    fn set_ewmh_properties(&self) -> Result<(), Box<dyn Error>> {
        let supported = [
            self.atoms.net_supported,
            self.atoms.net_supporting_wm_check,
            self.atoms.net_wm_name,
            self.atoms.net_client_list,
            self.atoms.net_active_window,
            self.atoms.net_wm_window_type,
            self.atoms.net_wm_strut,
            self.atoms.net_wm_strut_partial,
            self.atoms.net_close_window,
        ];
        self.conn.change_property32(
            PropMode::REPLACE,
            self.root,
            self.atoms.net_supported,
            AtomEnum::ATOM,
            &supported,
        )?;
        self.conn.change_property32(
            PropMode::REPLACE,
            self.root,
            self.atoms.net_supporting_wm_check,
            AtomEnum::WINDOW,
            &[self.panel],
        )?;
        self.conn.change_property8(
            PropMode::REPLACE,
            self.root,
            self.atoms.net_wm_name,
            self.atoms.utf8_string,
            b"HelloWM",
        )?;
        self.conn.change_property32(
            PropMode::REPLACE,
            self.panel,
            self.atoms.net_supporting_wm_check,
            AtomEnum::WINDOW,
            &[self.panel],
        )?;
        self.conn.change_property8(
            PropMode::REPLACE,
            self.panel,
            self.atoms.net_wm_name,
            self.atoms.utf8_string,
            b"HelloWM",
        )?;
        Ok(())
    }

    fn set_panel_properties(&self) -> Result<(), Box<dyn Error>> {
        self.conn.change_property32(
            PropMode::REPLACE,
            self.panel,
            self.atoms.net_wm_window_type,
            AtomEnum::ATOM,
            &[self.atoms.net_wm_window_type_dock],
        )?;
        self.conn.change_property32(
            PropMode::REPLACE,
            self.panel,
            self.atoms.net_wm_strut,
            AtomEnum::CARDINAL,
            &[0, 0, 0, self.panel_height as u32],
        )?;
        self.conn.change_property32(
            PropMode::REPLACE,
            self.panel,
            self.atoms.net_wm_strut_partial,
            AtomEnum::CARDINAL,
            &[
                0,
                0,
                0,
                self.panel_height as u32,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                self.screen_width.saturating_sub(1) as u32,
            ],
        )?;
        Ok(())
    }

    fn init_widgets(&mut self) {
        self.widgets = self
            .config
            .widgets
            .clone()
            .into_iter()
            .map(|spec| WidgetRuntime {
                spec,
                value: String::new(),
                last_run: Instant::now() - Duration::from_secs(30),
            })
            .collect();
    }

    fn load_keybinds(&mut self) -> Result<(), Box<dyn Error>> {
        for item in &self.config.keybinds {
            match self.parse_keybind(item) {
                Ok(binding) => {
                    for extra in [
                        ModMask::from(0u16),
                        ModMask::LOCK,
                        ModMask::M2,
                        ModMask::LOCK | ModMask::M2,
                    ] {
                        let result = self.conn.grab_key(
                            false,
                            self.root,
                            binding.modifiers | extra,
                            binding.keycode,
                            GrabMode::ASYNC,
                            GrabMode::ASYNC,
                        );
                        if let Err(error) = result {
                            eprintln!("HelloWM: unable to register {}: {error}", item.chord);
                        }
                    }
                    self.keybinds.push(binding);
                }
                Err(error) => eprintln!("HelloWM: ignoring keybind '{}': {error}", item.chord),
            }
        }
        Ok(())
    }

    fn parse_keybind(&self, item: &KeybindConfig) -> Result<BoundKey, Box<dyn Error>> {
        let mut parts = item.chord.split('+').map(str::trim).collect::<Vec<_>>();
        let key_name = parts.pop().ok_or("empty key chord")?;
        let mut modifiers = ModMask::from(0u16);
        for part in parts {
            match part.to_ascii_lowercase().as_str() {
                "super" | "win" | "mod4" => modifiers |= ModMask::M4,
                "ctrl" | "control" => modifiers |= ModMask::CONTROL,
                "alt" | "mod1" => modifiers |= ModMask::M1,
                "shift" => modifiers |= ModMask::SHIFT,
                other => return Err(format!("unknown modifier '{other}'").into()),
            }
        }
        if modifiers == ModMask::from(0u16) {
            return Err("a keybind must include at least one modifier".into());
        }
        let keysym = keysym_from_name(key_name)
            .ok_or_else(|| format!("unsupported key name '{key_name}'"))?;
        let keycode = keycode_for_keysym(&self.conn, keysym)?
            .ok_or_else(|| format!("key '{key_name}' is not present in the keyboard map"))?;
        Ok(BoundKey {
            keycode,
            modifiers,
            action: item.action.clone(),
        })
    }

    fn run(&mut self) -> Result<(), Box<dyn Error>> {
        while self.running {
            // Motion events can arrive much faster than the display refreshes.
            // Keep only the newest pointer position in each queued batch so we
            // don't ask the X server to move the same child window dozens of
            // times before it has a chance to present a frame.
            let mut latest_icon_pointer = None;
            while let Some(event) = self.conn.poll_for_event()? {
                match event {
                    Event::MotionNotify(ev)
                        if self.desktop_drag_start.is_some()
                            && self.desktop_icon_windows.iter().any(|(_, window)| *window == ev.event) =>
                    {
                        latest_icon_pointer = Some((ev.root_x, ev.root_y));
                    }
                    other => {
                        if let Some((x, y)) = latest_icon_pointer.take() {
                            self.handle_desktop_motion(x, y)?;
                        }
                        self.handle_event(other)?;
                    }
                }
            }
            if let Some((x, y)) = latest_icon_pointer.take() {
                self.handle_desktop_motion(x, y)?;
            }
            self.tick();
            self.conn.flush()?;
            // Native icon windows move directly in the X server; short polling
            // keeps grabbed pointer-motion events responsive without redrawing.
            let poll_delay = if self.desktop_drag_start.is_some() { 2 } else { 20 };
            thread::sleep(Duration::from_millis(poll_delay));
        }
        Ok(())
    }

    fn tick(&mut self) {
        if self.last_network_poll.elapsed() >= Duration::from_secs(3) {
            self.refresh_external_state(false);
            let _ = self.draw_panel();
        }
        if self.last_desktop_scan.elapsed() >= Duration::from_secs(2) {
            self.last_desktop_scan = Instant::now();
            let mut redraw_desktop = false;
            if let Some(desktop) = &mut self.desktop {
                if desktop.dragging.is_none() {
                    let before = desktop.icons.iter().map(|icon| icon.path.clone()).collect::<Vec<_>>();
                    if desktop.reload().is_ok() {
                        let after = desktop.icons.iter().map(|icon| icon.path.clone()).collect::<Vec<_>>();
                        redraw_desktop = before != after;
                    }
                }
            }
            if redraw_desktop { let _ = self.draw_desktop(); }
        }
    }

    fn refresh_external_state(&mut self, force: bool) {
        if force || self.last_network_poll.elapsed() >= Duration::from_secs(3) {
            self.network = read_network_state();
            self.last_network_poll = Instant::now();
        }
        let now = Instant::now();
        let mut changed = false;
        for widget in &mut self.widgets {
            let interval = Duration::from_secs(widget.spec.interval_seconds.max(1));
            if force || widget.last_run.elapsed() >= interval {
                let value = run_widget_command(&widget.spec);
                if value != widget.value {
                    widget.value = value;
                    changed = true;
                }
                widget.last_run = now;
            }
        }
        if changed && !force {
            let _ = self.draw_panel();
        }
    }

    fn handle_event(&mut self, event: Event) -> Result<(), Box<dyn Error>> {
        match event {
            Event::MapRequest(ev) => self.manage_window(ev.window)?,
            Event::ConfigureRequest(ev) => self.handle_configure_request(ev)?,
            Event::DestroyNotify(ev) => {
                self.remove_client(ev.window);
                self.tray_windows.retain(|window| *window != ev.window);
                self.draw_panel()?;
            }
            Event::UnmapNotify(ev) => {
                if self.clients.contains(&ev.window) {
                    self.remove_client(ev.window);
                    self.draw_panel()?;
                }
            }
            Event::ButtonPress(ev) => self.handle_button_press(ev)?,
            Event::ButtonRelease(_ev) => {
                self.drag = None;
                // Icon drags use an X11 pointer grab, so release is delivered
                // even when the pointer has moved outside the icon window.
                if self.desktop_drag_start.take().is_some() {
                    let _ = self.conn.ungrab_pointer(CURRENT_TIME);
                    if let Some(desktop) = &mut self.desktop {
                        desktop.dragging = None;
                        if let Err(error) = desktop.save_layout() {
                            eprintln!("HelloWM: could not save desktop icon positions: {error}");
                        }
                    }
                }
            }
            Event::MotionNotify(ev) => {
                if self.desktop_icon_windows.iter().any(|(_, window)| *window == ev.event) {
                    self.handle_desktop_motion(ev.root_x, ev.root_y)?;
                } else {
                    self.handle_motion(ev)?;
                }
            }
            Event::KeyPress(ev) => {
                if self.about_window.as_ref().is_some_and(|about| about.window == ev.event) {
                    self.handle_about_key(ev.detail)?;
                } else {
                    self.handle_key_press(ev.detail, ev.state)?;
                }
            }
            Event::EnterNotify(ev) => {
                if self.clients.contains(&ev.event) {
                    self.focus_window(ev.event)?;
                }
            }
            Event::Expose(ev) => {
                if ev.count == 0 {
                    if ev.window == self.panel {
                        self.draw_panel()?;
                    } else if let Some(index) = self.desktop_icon_index(ev.window) {
                        self.draw_desktop_icon_window(index)?;
                    } else if self.about_window.as_ref().is_some_and(|about| about.window == ev.window) {
                        self.draw_about()?;
                    }
                    // The desktop parent paints its configured wallpaper
                    // background in the X server; no client-side redraw is
                    // needed for its Expose events.
                }
            }
            Event::SelectionNotify(ev) => self.handle_selection_notify(ev)?,
            Event::PropertyNotify(ev) => {
                if ev.window == self.root
                    && (ev.atom == self.atoms.xrootpmap_id || ev.atom == self.atoms.esetroot_pmap_id)
                {
                    self.desktop_root_pixmap = read_root_pixmap(
                        &self.conn,
                        self.root,
                        self.atoms.xrootpmap_id,
                        self.atoms.esetroot_pmap_id,
                    );
                    self.apply_desktop_background()?;
                    self.draw_desktop()?;
                } else if self.clients.contains(&ev.window)
                    && (ev.atom == self.atoms.net_wm_name || ev.atom == self.atoms.wm_name)
                {
                    self.draw_panel()?;
                }
            }
            Event::ClientMessage(ev) => self.handle_client_message(ev)?,
            Event::MapNotify(ev) => {
                if self.tray_windows.contains(&ev.window) {
                    self.draw_panel()?;
                }
            }
            Event::ReparentNotify(ev) => {
                if self.tray_windows.contains(&ev.window) && ev.parent != self.panel {
                    self.tray_windows.retain(|window| *window != ev.window);
                    self.draw_panel()?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn manage_window(&mut self, window: Window) -> Result<(), Box<dyn Error>> {
        if window == self.panel || self.desktop_window == Some(window) || self.clients.contains(&window) || self.tray_windows.contains(&window) {
            return Ok(());
        }
        let attrs = match self.conn.get_window_attributes(window)?.reply() {
            Ok(attrs) => attrs,
            Err(_) => return Ok(()),
        };
        if attrs.override_redirect {
            self.conn.map_window(window)?;
            return Ok(());
        }

        let geometry = match self.conn.get_geometry(window)?.reply() {
            Ok(geometry) => geometry,
            Err(_) => return Ok(()),
        };
        let mut x = geometry.x as i32;
        let mut y = geometry.y as i32;
        let width = geometry.width.max(320);
        let height = geometry.height.max(180);
        if x == 0 && y == 0 {
            x = ((self.screen_width as i32 - width as i32) / 2).max(12);
            y = ((self.screen_height as i32 - self.panel_height as i32 - height as i32) / 2).max(12);
        }
        x = x.clamp(0, self.screen_width.saturating_sub(120) as i32);
        y = y.clamp(0, self.screen_height.saturating_sub(self.panel_height + 80) as i32);

        let border = if self.active == Some(window) { self.palette.accent } else { self.palette.border };
        self.conn.change_window_attributes(
            window,
            &ChangeWindowAttributesAux::new()
                .border_pixel(border)
                .event_mask(EventMask::ENTER_WINDOW | EventMask::PROPERTY_CHANGE | EventMask::STRUCTURE_NOTIFY),
        )?;
        self.conn.configure_window(
            window,
            &x11rb::protocol::xproto::ConfigureWindowAux::new()
                .x(x)
                .y(y)
                .width(width as u32)
                .height(height as u32)
                .border_width(CLIENT_BORDER)
                .stack_mode(StackMode::ABOVE),
        )?;

        // Super+left drag moves a client; Super+right drag resizes it.
        let drag_mask = EventMask::BUTTON_PRESS | EventMask::BUTTON_RELEASE | EventMask::POINTER_MOTION;
        for button in [ButtonIndex::M1, ButtonIndex::M3] {
            let _ = self.conn.grab_button(
                false,
                window,
                drag_mask,
                GrabMode::ASYNC,
                GrabMode::ASYNC,
                NONE,
                NONE,
                button,
                ModMask::M4,
            );
        }

        self.clients.push(window);
        self.conn.map_window(window)?;
        self.update_client_list()?;
        self.focus_window(window)?;
        self.draw_panel()?;
        Ok(())
    }

    fn remove_client(&mut self, window: Window) {
        self.clients.retain(|candidate| *candidate != window);
        if self.active == Some(window) {
            self.active = self.clients.last().copied();
            if let Some(next) = self.active {
                let _ = self.focus_window(next);
            } else {
                let _ = self.conn.set_input_focus(InputFocus::POINTER_ROOT, self.root, CURRENT_TIME);
                let _ = self.conn.delete_property(self.root, self.atoms.net_active_window);
            }
        }
        let _ = self.update_client_list();
    }

    fn update_client_list(&self) -> Result<(), Box<dyn Error>> {
        self.conn.change_property32(
            PropMode::REPLACE,
            self.root,
            self.atoms.net_client_list,
            AtomEnum::WINDOW,
            &self.clients,
        )?;
        Ok(())
    }

    fn focus_window(&mut self, window: Window) -> Result<(), Box<dyn Error>> {
        if !self.clients.contains(&window) {
            return Ok(());
        }
        if let Some(previous) = self.active {
            if previous != window {
                let _ = self.conn.change_window_attributes(
                    previous,
                    &ChangeWindowAttributesAux::new().border_pixel(self.palette.border),
                );
            }
        }
        self.conn.change_window_attributes(
            window,
            &ChangeWindowAttributesAux::new().border_pixel(self.palette.accent),
        )?;
        self.conn.configure_window(
            window,
            &x11rb::protocol::xproto::ConfigureWindowAux::new().stack_mode(StackMode::ABOVE),
        )?;
        self.conn.set_input_focus(InputFocus::POINTER_ROOT, window, CURRENT_TIME)?;
        self.conn.change_property32(
            PropMode::REPLACE,
            self.root,
            self.atoms.net_active_window,
            AtomEnum::WINDOW,
            &[window],
        )?;
        self.active = Some(window);
        self.draw_panel()?;
        Ok(())
    }

    fn handle_configure_request(
        &self,
        ev: x11rb::protocol::xproto::ConfigureRequestEvent,
    ) -> Result<(), Box<dyn Error>> {
        let mask = ev.value_mask;
        let mut aux = x11rb::protocol::xproto::ConfigureWindowAux::new();
        if mask.contains(ConfigWindow::X) {
            aux = aux.x(ev.x as i32);
        }
        if mask.contains(ConfigWindow::Y) {
            aux = aux.y(ev.y as i32);
        }
        if mask.contains(ConfigWindow::WIDTH) {
            aux = aux.width(ev.width as u32);
        }
        if mask.contains(ConfigWindow::HEIGHT) {
            aux = aux.height(ev.height as u32);
        }
        if mask.contains(ConfigWindow::BORDER_WIDTH) {
            aux = aux.border_width(ev.border_width as u32);
        }
        if mask.contains(ConfigWindow::SIBLING) && mask.contains(ConfigWindow::STACK_MODE) {
            aux = aux.sibling(ev.sibling);
        }
        if mask.contains(ConfigWindow::STACK_MODE) {
            aux = aux.stack_mode(ev.stack_mode);
        }
        self.conn.configure_window(ev.window, &aux)?;
        self.conn.configure_window(
            self.panel,
            &x11rb::protocol::xproto::ConfigureWindowAux::new().stack_mode(StackMode::ABOVE),
        )?;
        Ok(())
    }

    fn handle_button_press(
        &mut self,
        ev: x11rb::protocol::xproto::ButtonPressEvent,
    ) -> Result<(), Box<dyn Error>> {
        if ev.event == self.panel {
            if ev.detail == 3 {
                self.toggle_theme()?;
                return Ok(());
            }
            if ev.detail == 1 {
                let x = ev.event_x as i32;
                if x < self.search_end_x() {
                    self.launch_search();
                } else if let Some((_, _, window)) = self
                    .task_hits
                    .iter()
                    .find(|(start, end, _)| x >= *start as i32 && x < *end as i32)
                {
                    self.focus_window(*window)?;
                }
            }
            return Ok(());
        }

        if let Some(index) = self.desktop_icon_index(ev.event) {
            self.handle_desktop_icon_button(ev, index)?;
            return Ok(());
        }
        if self.desktop_window == Some(ev.event) {
            self.handle_desktop_button(ev)?;
            return Ok(());
        }
        if self.about_window.as_ref().is_some_and(|about| about.window == ev.event) {
            self.close_about()?;
            return Ok(());
        }

        if self.clients.contains(&ev.event) {
            self.focus_window(ev.event)?;
            if ev.state.contains(KeyButMask::MOD4) && (ev.detail == 1 || ev.detail == 3) {
                if let Ok(geometry) = self.conn.get_geometry(ev.event)?.reply() {
                    self.drag = Some(DragState {
                        window: ev.event,
                        kind: if ev.detail == 1 { DragKind::Move } else { DragKind::Resize },
                        start_root_x: ev.root_x,
                        start_root_y: ev.root_y,
                        start_x: geometry.x,
                        start_y: geometry.y,
                        start_width: geometry.width,
                        start_height: geometry.height,
                    });
                }
            }
        }
        Ok(())
    }

    fn desktop_icon_index(&self, window: Window) -> Option<usize> {
        let path = self.desktop_icon_windows.iter()
            .find(|(_, icon_window)| *icon_window == window)
            .map(|(path, _)| path)?;
        self.desktop.as_ref()?.icons.iter().position(|icon| &icon.path == path)
    }

    fn handle_desktop_icon_button(
        &mut self,
        ev: x11rb::protocol::xproto::ButtonPressEvent,
        index: usize,
    ) -> Result<(), Box<dyn Error>> {
        match ev.detail {
            1 => {
                let double_click = self.desktop.as_ref()
                    .and_then(|desktop| desktop.last_click.as_ref())
                    .is_some_and(|(previous, when)| *previous == index && when.elapsed() <= Duration::from_millis(450));
                if double_click {
                    if let Some(desktop) = &self.desktop { desktop.open_icon(index); }
                    if let Some(desktop) = &mut self.desktop { desktop.last_click = None; }
                    self.desktop_drag_start = None;
                    return Ok(());
                }

                let Some((icon_x, icon_y)) = self.desktop.as_ref()
                    .and_then(|desktop| desktop.icons.get(index))
                    .map(|icon| (icon.x, icon.y)) else {
                    return Ok(());
                };
                if let Some(desktop) = &mut self.desktop {
                    desktop.selected = Some(index);
                    desktop.last_click = Some((index, Instant::now()));
                }
                let grab = self.conn.grab_pointer(
                    false,
                    ev.event,
                    EventMask::BUTTON_RELEASE | EventMask::POINTER_MOTION,
                    GrabMode::ASYNC,
                    GrabMode::ASYNC,
                    NONE,
                    NONE,
                    CURRENT_TIME,
                )?.reply()?;
                if grab.status == GrabStatus::SUCCESS {
                    self.desktop_drag_start = Some((index, ev.root_x, ev.root_y, icon_x, icon_y));
                    if let Some(desktop) = &mut self.desktop {
                        desktop.dragging = Some(index);
                    }
                } else {
                    self.desktop_drag_start = None;
                    if let Some(desktop) = &mut self.desktop {
                        desktop.dragging = None;
                    }
                    eprintln!("HelloWM: could not grab the pointer for desktop icon drag: {:?}", grab.status);
                }
                self.draw_desktop()?;
            }
            3 => {
                if let Some(desktop) = &mut self.desktop {
                    desktop.selected = Some(index);
                    desktop.last_click = None;
                }
                self.draw_desktop()?;
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_desktop_button(&mut self, ev: x11rb::protocol::xproto::ButtonPressEvent) -> Result<(), Box<dyn Error>> {
        match ev.detail {
            1 => {
                if let Some(desktop) = &mut self.desktop {
                    desktop.selected = None;
                    desktop.last_click = None;
                    desktop.dragging = None;
                }
                self.desktop_drag_start = None;
                self.draw_desktop()?;
            }
            2 => self.open_about()?,
            3 => {
                let creator = config::expand_home(&self.config.launcher_creator);
                if creator.is_file() {
                    if let Err(error) = Command::new(&creator).spawn() {
                        eprintln!("HelloWM: could not start launcher creator: {error}");
                    }
                } else {
                    eprintln!("HelloWM: launcher creator not found at {}; reinstall HelloWM or set launcher_creator in config.toml", creator.display());
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Move the icon's native child window and explicitly invalidate the
    /// vacated area in the desktop parent. The icon is a retained X11 child
    /// window; only the exposed parent rectangle is cleared on each move.
    fn handle_desktop_motion(&mut self, root_x: i16, root_y: i16) -> Result<(), Box<dyn Error>> {
        let Some((index, start_x, start_y, icon_x, icon_y)) = self.desktop_drag_start else {
            return Ok(());
        };
        let dx = root_x as i32 - start_x as i32;
        let dy = root_y as i32 - start_y as i32;
        if dx.abs() <= 3 && dy.abs() <= 3 {
            return Ok(());
        }
        let new_x = (icon_x as i32 + dx)
            .clamp(0, (self.screen_width as i32 - desktop::IconBounds::WIDTH).max(0));
        let new_y = (icon_y as i32 + dy)
            .clamp(0, (self.screen_height as i32 - self.panel_height as i32 - desktop::IconBounds::HEIGHT).max(0));
        let Some((icon_window, old_x, old_y)) = self.desktop.as_ref()
            .and_then(|desktop| desktop.icons.get(index))
            .and_then(|icon| {
                self.desktop_icon_windows.iter()
                    .find(|(path, _)| path == &icon.path)
                    .map(|(_, window)| (*window, icon.x, icon.y))
            }) else {
            return Ok(());
        };
        if old_x as i32 == new_x && old_y as i32 == new_y {
            return Ok(());
        }
        if let Some(desktop) = &mut self.desktop {
            desktop.move_icon(index, new_x as i16, new_y as i16);
        }
        self.conn.configure_window(
            icon_window,
            &x11rb::protocol::xproto::ConfigureWindowAux::new().x(new_x).y(new_y),
        )?;

        // Explicitly invalidate the vacated parent area after the child window
        // has moved. Some X servers/compositors do not promptly repaint the
        // parent when a rapidly-moving override-redirect child is reconfigured.
        // Clearing the parent (not the child) restores its wallpaper pixmap;
        // child icon windows remain above it and are unaffected where they
        // overlap this rectangle. Keep this request asynchronous in the drag
        // path so pointer movement is not serialized behind round trips.
        if let Some(parent) = self.desktop_window {
            self.conn.clear_area(
                false,
                parent,
                old_x,
                old_y,
                desktop::IconBounds::WIDTH as u16,
                desktop::IconBounds::HEIGHT as u16,
            )?;
        }
        Ok(())
    }

    /// Synchronize the retained icon model to native X11 child windows.
    /// Each icon owns its drawable and position, much like a canvas item in
    /// xfdesktop/Nemo. X11 handles clipping, movement and parent exposure.
    fn draw_desktop(&mut self) -> Result<(), Box<dyn Error>> {
        self.sync_desktop_icon_windows()?;
        let count = self.desktop.as_ref().map_or(0, |desktop| desktop.icons.len());
        for index in 0..count {
            self.draw_desktop_icon_window(index)?;
        }
        Ok(())
    }

    fn apply_desktop_background(&self) -> Result<(), Box<dyn Error>> {
        let Some(window) = self.desktop_window else { return Ok(()); };
        let aux = if let Some(pixmap) = self.desktop_root_pixmap {
            ChangeWindowAttributesAux::new().background_pixmap(pixmap)
        } else {
            ChangeWindowAttributesAux::new().background_pixmap(BackPixmap::PARENT_RELATIVE)
        };
        self.conn.change_window_attributes(window, &aux)?.check()?;
        self.conn.clear_area(
            false,
            window,
            0,
            0,
            self.screen_width,
            self.screen_height.saturating_sub(self.panel_height).max(1),
        )?.check()?;
        for (_, icon_window) in &self.desktop_icon_windows {
            self.conn.clear_area(
                false,
                *icon_window,
                0,
                0,
                desktop::IconBounds::WIDTH as u16,
                desktop::IconBounds::HEIGHT as u16,
            )?.check()?;
        }
        Ok(())
    }

    fn sync_desktop_icon_windows(&mut self) -> Result<(), Box<dyn Error>> {
        let Some(parent) = self.desktop_window else { return Ok(()); };
        let icons = self.desktop.as_ref().map(|desktop| desktop.icons.clone()).unwrap_or_default();
        let current_paths = icons.iter().map(|icon| icon.path.clone()).collect::<std::collections::HashSet<_>>();

        let obsolete = self.desktop_icon_windows.iter()
            .filter(|(path, _)| !current_paths.contains(path))
            .map(|(_, window)| *window)
            .collect::<Vec<_>>();
        for window in obsolete {
            self.conn.destroy_window(window)?;
        }
        self.desktop_icon_windows.retain(|(path, _)| current_paths.contains(path));

        for icon in &icons {
            let existing = self.desktop_icon_windows.iter()
                .find(|(path, _)| path == &icon.path)
                .map(|(_, window)| *window);
            let window = if let Some(window) = existing {
                self.conn.configure_window(
                    window,
                    &x11rb::protocol::xproto::ConfigureWindowAux::new()
                        .x(icon.x as i32)
                        .y(icon.y as i32),
                )?;
                window
            } else {
                let window = self.conn.generate_id()?;
                self.conn.create_window(
                    self.screen_depth,
                    window,
                    parent,
                    icon.x,
                    icon.y,
                    desktop::IconBounds::WIDTH as u16,
                    desktop::IconBounds::HEIGHT as u16,
                    0,
                    WindowClass::INPUT_OUTPUT,
                    self.screen_visual,
                    &CreateWindowAux::new()
                        .background_pixmap(BackPixmap::PARENT_RELATIVE)
                        .border_pixel(self.palette.panel)
                        .override_redirect(1u32)
                        .event_mask(
                            EventMask::EXPOSURE
                                | EventMask::BUTTON_PRESS
                                | EventMask::BUTTON_RELEASE
                                | EventMask::POINTER_MOTION,
                        ),
                )?.check()?;
                self.conn.map_window(window)?.check()?;
                self.desktop_icon_windows.push((icon.path.clone(), window));
                window
            };
            // Moving an existing child window is handled by the server; only
            // content changes (selection, icon reload, or Expose) need drawing.
            let _ = window;
        }
        Ok(())
    }

    fn draw_desktop_icon_window(&self, index: usize) -> Result<(), Box<dyn Error>> {
        let Some(desktop) = &self.desktop else { return Ok(()); };
        let Some(icon) = desktop.icons.get(index) else { return Ok(()); };
        let Some(window) = self.desktop_icon_windows.iter()
            .find(|(path, _)| path == &icon.path)
            .map(|(_, window)| *window) else { return Ok(()); };
        // Clear the item's own surface back to its parent-relative wallpaper
        // before painting its current selection state. This also erases an old
        // selection fill without touching neighbouring desktop items.
        self.conn.clear_area(
            false,
            window,
            0,
            0,
            desktop::IconBounds::WIDTH as u16,
            desktop::IconBounds::HEIGHT as u16,
        )?.check()?;
        self.draw_desktop_icon(window, icon, desktop.selected == Some(index))
    }

    fn draw_desktop_icon(
        &self,
        drawable: Window,
        icon: &desktop::DesktopIcon,
        selected: bool,
    ) -> Result<(), Box<dyn Error>> {
        // Coordinates are local to an icon's own X11 child window. The window
        // is moved as a whole during dragging, so drawing is never repeated.
        let x = 0_i32;
        let y = 0_i32;
        if selected {
            self.fill_rect_on(
                drawable,
                0,
                0,
                desktop::IconBounds::WIDTH,
                desktop::IconBounds::HEIGHT,
                self.palette.accent,
            )?;
        }
        match icon.kind {
            IconKind::Folder => {
                self.fill_rect_on(drawable, x + 11, y + 13, 36, 25, 0x00a0e6)?;
                self.fill_rect_on(drawable, x + 13, y + 8, 17, 8, 0x00a0e6)?;
                self.fill_rect_on(drawable, x + 12, y + 15, 34, 20, 0x00b8ff)?;
            }
            IconKind::Launcher => {
                self.fill_rect_on(drawable, x + 10, y + 7, 40, 40, self.palette.accent)?;
                self.draw_text_on(drawable, ">", x + 24, y + 34, 0x00ffffff, self.palette.accent)?;
            }
            IconKind::File => {
                self.fill_rect_on(drawable, x + 15, y + 6, 31, 41, 0x00eeeeee)?;
                self.set_gc(self.palette.border, self.palette.panel)?;
                self.conn.poly_rectangle(drawable, self.gc, &[Rectangle {
                    x: (x + 15) as i16,
                    y: (y + 6) as i16,
                    width: 31,
                    height: 41,
                }])?;
                self.fill_rect_on(drawable, x + 21, y + 19, 19, 2, 0x00808080)?;
                self.fill_rect_on(drawable, x + 21, y + 25, 19, 2, 0x00808080)?;
                self.fill_rect_on(drawable, x + 21, y + 31, 14, 2, 0x00808080)?;
            }
        }
        let label = truncate_ascii(&icon.name, 11);
        let text_x = ((desktop::IconBounds::WIDTH - label.len() as i32 * 8) / 2).max(1);
        self.draw_transparent_text_on(drawable, &label, text_x + 1, y + 65, 0x00000000)?;
        self.draw_transparent_text_on(drawable, &label, text_x, y + 64, 0x00ffffff)?;
        Ok(())
    }

    fn handle_xdnd_message(&mut self, ev: ClientMessageEvent) -> Result<(), Box<dyn Error>> {
        let Some(target) = self.desktop_window else { return Ok(()); };
        let data = ev.data.as_data32();
        if ev.type_ == self.atoms.xdnd_enter {
            self.xdnd_source = Some(data[0]);
            let mut types = Vec::new();
            if data[1] & 1 != 0 {
                if let Ok(reply) = self.conn.get_property(false, data[0], self.atoms.xdnd_type_list, AtomEnum::ATOM, 0, 32)?.reply() {
                    types = reply.value32().map(|values| values.collect()).unwrap_or_default();
                }
            } else {
                types.extend([data[2], data[3], data[4]]);
            }
            self.xdnd_accept_uri = types.contains(&self.atoms.text_uri_list);
        } else if ev.type_ == self.atoms.xdnd_position {
            let source = data[0];
            self.xdnd_source = Some(source);
            let accept = self.xdnd_accept_uri;
            let status = ClientMessageEvent::new(
                32,
                source,
                self.atoms.xdnd_status,
                [target, if accept { 1 } else { 0 }, 0, 0, if accept { self.atoms.xdnd_action_copy } else { 0 }],
            );
            self.conn.send_event(false, source, EventMask::NO_EVENT, status)?;
        } else if ev.type_ == self.atoms.xdnd_drop {
            let source = data[0];
            self.xdnd_source = Some(source);
            if self.xdnd_accept_uri {
                self.conn.convert_selection(target, self.atoms.xdnd_selection, self.atoms.text_uri_list, self.atoms.xdnd_selection, data[2])?;
            } else {
                self.finish_xdnd(false)?;
            }
        }
        Ok(())
    }

    fn handle_selection_notify(&mut self, ev: x11rb::protocol::xproto::SelectionNotifyEvent) -> Result<(), Box<dyn Error>> {
        if ev.selection != self.atoms.xdnd_selection || self.desktop_window != Some(ev.requestor) {
            return Ok(());
        }
        if ev.property == NONE {
            self.finish_xdnd(false)?;
            return Ok(());
        }
        let property = self.conn.get_property(true, ev.requestor, ev.property, AtomEnum::ANY, 0, 65536)?.reply()?;
        let content = String::from_utf8_lossy(&property.value).to_string();
        let imported = if let Some(desktop) = &mut self.desktop {
            desktop.import_uri_list(&content).unwrap_or(0)
        } else { 0 };
        self.finish_xdnd(imported > 0)?;
        self.draw_desktop()?;
        Ok(())
    }

    fn finish_xdnd(&mut self, success: bool) -> Result<(), Box<dyn Error>> {
        if let (Some(source), Some(target)) = (self.xdnd_source.take(), self.desktop_window) {
            let finished = ClientMessageEvent::new(
                32,
                source,
                self.atoms.xdnd_finished,
                [target, if success { 1 } else { 0 }, if success { self.atoms.xdnd_action_copy } else { 0 }, 0, 0],
            );
            self.conn.send_event(false, source, EventMask::NO_EVENT, finished)?;
        }
        self.xdnd_accept_uri = false;
        Ok(())
    }

    fn handle_motion(&self, ev: x11rb::protocol::xproto::MotionNotifyEvent) -> Result<(), Box<dyn Error>> {
        let Some(drag) = &self.drag else { return Ok(()); };
        if drag.window != self.active.unwrap_or(NONE) {
            return Ok(());
        }
        let dx = ev.root_x as i32 - drag.start_root_x as i32;
        let dy = ev.root_y as i32 - drag.start_root_y as i32;
        match drag.kind {
            DragKind::Move => {
                let x = (drag.start_x as i32 + dx)
                    .clamp(0, self.screen_width.saturating_sub(120) as i32);
                let y = (drag.start_y as i32 + dy)
                    .clamp(0, self.screen_height.saturating_sub(self.panel_height + 48) as i32);
                self.conn.configure_window(
                    drag.window,
                    &x11rb::protocol::xproto::ConfigureWindowAux::new().x(x).y(y),
                )?;
            }
            DragKind::Resize => {
                let width = (drag.start_width as i32 + dx).clamp(200, self.screen_width as i32) as u32;
                let height = (drag.start_height as i32 + dy)
                    .clamp(120, self.screen_height.saturating_sub(self.panel_height) as i32) as u32;
                self.conn.configure_window(
                    drag.window,
                    &x11rb::protocol::xproto::ConfigureWindowAux::new()
                        .width(width)
                        .height(height),
                )?;
            }
        }
        Ok(())
    }

    fn handle_key_press(&mut self, keycode: u8, state: KeyButMask) -> Result<(), Box<dyn Error>> {
        let normalized = state.bits() & !(ModMask::LOCK.bits() | ModMask::M2.bits());
        // Global chords continue to work while the in-panel command field has focus.
        for binding in self.keybinds.clone() {
            if binding.keycode == keycode && binding.modifiers.bits() == normalized {
                self.perform_action(&binding.action)?;
                return Ok(());
            }
        }
        if self.search_active {
            self.handle_search_key(keycode, state)?;
        }
        Ok(())
    }

    fn handle_search_key(&mut self, keycode: u8, state: KeyButMask) -> Result<(), Box<dyn Error>> {
        let keysym = keysym_for_keycode(&self.conn, keycode, state.contains(KeyButMask::SHIFT))?;
        match keysym {
            Some(0xff1b) => {
                self.search_active = false;
                self.search_text.clear();
                let _ = self.conn.set_input_focus(InputFocus::POINTER_ROOT, self.root, CURRENT_TIME);
            }
            Some(0xff0d) => {
                let command = self.search_text.trim().to_string();
                self.search_active = false;
                self.search_text.clear();
                let _ = self.conn.set_input_focus(InputFocus::POINTER_ROOT, self.root, CURRENT_TIME);
                if !command.is_empty() { spawn_shell(&command); }
            }
            Some(0xff08) => { self.search_text.pop(); }
            Some(value) if value >= 0x20 && value <= 0x7e
                && !state.contains(KeyButMask::CONTROL)
                && !state.contains(KeyButMask::MOD1)
                && !state.contains(KeyButMask::MOD4) => {
                    if self.search_text.len() < 512 { self.search_text.push(char::from_u32(value).unwrap_or_default()); }
                }
            _ => {}
        }
        self.draw_panel()?;
        Ok(())
    }

    fn focus_search(&mut self) -> Result<(), Box<dyn Error>> {
        self.search_active = true;
        self.search_text.clear();
        self.conn.set_input_focus(InputFocus::PARENT, self.panel, CURRENT_TIME)?;
        self.draw_panel()?;
        Ok(())
    }

    fn perform_action(&mut self, action: &str) -> Result<(), Box<dyn Error>> {
        match action {
            "terminal" => spawn_shell(&self.config.terminal),
            "close" => self.close_focused()?,
            "search" => self.launch_search(),
            "focus-search" => self.focus_search()?,
            "about" => self.open_about()?,
            "quit" => self.running = false,
            command if command.starts_with("exec:") => spawn_shell(command.trim_start_matches("exec:")),
            other => eprintln!("HelloWM: unknown keybind action '{other}'"),
        }
        Ok(())
    }

    fn close_focused(&self) -> Result<(), Box<dyn Error>> {
        let Some(window) = self.active else { return Ok(()); };
        let protocols = self
            .conn
            .get_property(false, window, self.atoms.wm_protocols, AtomEnum::ATOM, 0, 64)?
            .reply()?;
        let supports_delete = protocols
            .value32()
            .map(|values| values.into_iter().any(|atom| atom == self.atoms.wm_delete_window))
            .unwrap_or(false);
        if supports_delete {
            let event = ClientMessageEvent::new(
                32,
                window,
                self.atoms.wm_protocols,
                [self.atoms.wm_delete_window, CURRENT_TIME, 0, 0, 0],
            );
            self.conn.send_event(false, window, EventMask::NO_EVENT, event)?;
        } else {
            self.conn.kill_client(window)?;
        }
        Ok(())
    }

    fn launch_search(&mut self) {
        let configured = self.config.search_command.trim();
        let fallback = self.config.fallback_search_command.trim();
        let configured_program = configured.split_whitespace().next().unwrap_or("");
        let fallback_program = fallback.split_whitespace().next().unwrap_or("");
        self.search_active = false;
        self.search_text.clear();
        if !configured.is_empty() && command_exists(configured_program) {
            spawn_shell(configured);
        } else if !fallback.is_empty() && command_exists(fallback_program) {
            spawn_shell(fallback);
        } else if command_exists("dmenu") {
            // Some distributions ship dmenu without its dmenu_run helper.
            // Feed it executable names from PATH so it still provides filtering.
            spawn_shell(r#"(
                old_ifs=$IFS
                IFS=:
                for directory in $PATH; do
                    [ -d "$directory" ] || continue
                    for executable in "$directory"/*; do
                        [ -f "$executable" ] && [ -x "$executable" ] && printf '%s\n' "${executable##*/}"
                    done
                done
                IFS=$old_ifs
            ) | sort -u | dmenu -i -p 'Run:' | while IFS= read -r selected; do
                [ -n "$selected" ] && exec sh -c "$selected"
            done"#);
        } else {
            // No rofi/dmenu installed: the taskbar field becomes a run-command prompt.
            let _ = self.focus_search();
        }
    }

    fn open_about(&mut self) -> Result<(), Box<dyn Error>> {
        if let Some(about) = &self.about_window {
            self.conn.map_window(about.window)?;
            self.conn.configure_window(about.window, &x11rb::protocol::xproto::ConfigureWindowAux::new().stack_mode(StackMode::ABOVE))?;
            self.conn.set_input_focus(InputFocus::PARENT, about.window, CURRENT_TIME)?;
            return Ok(());
        }

        let script = config::expand_home(&self.config.about_script);
        let output = if script.is_file() {
            Command::new("sh").arg(&script).output().ok()
        } else {
            None
        };
        let mut lines = output
            .map(|value| String::from_utf8_lossy(&value.stdout).lines().map(|line| line.to_string()).collect::<Vec<_>>())
            .unwrap_or_default();
        if lines.is_empty() {
            lines = vec![
                format!("Operating system: {}", std::env::consts::OS),
                format!("Architecture: {}", std::env::consts::ARCH),
                format!("Shell: {}", std::env::var("SHELL").unwrap_or_else(|_| "Unknown".into())),
                "System information script not found. Reinstall HelloWM or set about_script in config.toml.".into(),
            ];
        }
        lines.retain(|line| !line.trim().is_empty());
        lines.truncate(12);

        let width = 560_u16.min(self.screen_width.saturating_sub(40).max(320));
        let height = 300_u16.min(self.screen_height.saturating_sub(self.panel_height + 40).max(220));
        let x = (self.screen_width.saturating_sub(width) / 2) as i16;
        let y = (self.screen_height.saturating_sub(self.panel_height).saturating_sub(height) / 2) as i16;
        let window = self.conn.generate_id()?;
        self.conn.create_window(
            self.screen_depth,
            window,
            self.root,
            x,
            y,
            width,
            height,
            1,
            WindowClass::INPUT_OUTPUT,
            self.screen_visual,
            &CreateWindowAux::new()
                .background_pixel(self.palette.surface)
                .border_pixel(self.palette.border)
                .override_redirect(1u32)
                .event_mask(EventMask::EXPOSURE | EventMask::BUTTON_PRESS | EventMask::KEY_PRESS),
        )?.check()?;
        self.conn.change_property8(PropMode::REPLACE, window, self.atoms.wm_name, AtomEnum::STRING, b"About computer")?.check()?;
        self.conn.map_window(window)?.check()?;
        self.conn.configure_window(window, &x11rb::protocol::xproto::ConfigureWindowAux::new().stack_mode(StackMode::ABOVE))?;
        self.conn.set_input_focus(InputFocus::PARENT, window, CURRENT_TIME)?;
        self.about_window = Some(AboutWindow { window, width, height, lines });
        self.draw_about()?;
        Ok(())
    }

    fn draw_about(&self) -> Result<(), Box<dyn Error>> {
        let Some(about) = &self.about_window else { return Ok(()); };
        let window = about.window;
        self.fill_rect_on(window, 0, 0, about.width as i32, about.height as i32, self.palette.surface)?;
        self.fill_rect_on(window, 0, 0, about.width as i32, 38, self.palette.panel)?;
        self.draw_text_on(window, "About computer", 18, 25, self.palette.text, self.palette.panel)?;
        self.draw_text_on(window, "HelloWM", 18, 66, self.palette.accent, self.palette.surface)?;
        let mut y = 94;
        for line in &about.lines {
            self.draw_text_on(window, &truncate_ascii(line, ((about.width as i32 - 40) / 8) as usize), 18, y, self.palette.text, self.palette.surface)?;
            y += 20;
        }
        self.draw_text_on(window, "Press Esc or click to close", 18, about.height as i32 - 18, self.palette.muted, self.palette.surface)?;
        Ok(())
    }

    fn handle_about_key(&mut self, keycode: u8) -> Result<(), Box<dyn Error>> {
        let keysym = keysym_for_keycode(&self.conn, keycode, false)?;
        if matches!(keysym, Some(0xff1b) | Some(0xff0d)) {
            self.close_about()?;
        }
        Ok(())
    }

    fn close_about(&mut self) -> Result<(), Box<dyn Error>> {
        if let Some(about) = self.about_window.take() {
            let _ = self.conn.destroy_window(about.window);
        }
        if let Some(active) = self.active {
            let _ = self.conn.set_input_focus(InputFocus::POINTER_ROOT, active, CURRENT_TIME);
        } else {
            let _ = self.conn.set_input_focus(InputFocus::POINTER_ROOT, self.root, CURRENT_TIME);
        }
        Ok(())
    }

    fn toggle_theme(&mut self) -> Result<(), Box<dyn Error>> {
        self.config.theme = if self.config.theme.eq_ignore_ascii_case("dark") {
            "light".into()
        } else {
            "dark".into()
        };
        self.palette = allocate_palette_for_window(&self.conn, self.panel, &self.config.theme)?;
        let refreshed_start_image = load_start_image(
            &self.conn,
            &self.conn.setup().roots[self.screen_num],
            &self.config.start_image,
            &self.config.theme,
        );
        self.start_image = refreshed_start_image;
        self.conn.change_window_attributes(
            self.panel,
            &ChangeWindowAttributesAux::new().background_pixel(self.palette.panel),
        )?;
        self.conn.change_gc(
            self.gc,
            &ChangeGCAux::new()
                .foreground(self.palette.text)
                .background(self.palette.panel),
        )?;
        for window in &self.clients {
            let border = if Some(*window) == self.active { self.palette.accent } else { self.palette.border };
            let _ = self.conn.change_window_attributes(
                *window,
                &ChangeWindowAttributesAux::new().border_pixel(border),
            );
        }
        if let Err(error) = config::save(&self.config) {
            eprintln!("HelloWM: could not save theme setting: {error}");
        }
        self.draw_panel()?;
        self.draw_desktop()?;
        if self.about_window.is_some() { self.draw_about()?; }
        Ok(())
    }

    fn handle_client_message(&mut self, ev: ClientMessageEvent) -> Result<(), Box<dyn Error>> {
        let data = ev.data.as_data32();
        if [self.atoms.xdnd_enter, self.atoms.xdnd_position, self.atoms.xdnd_drop].contains(&ev.type_) {
            self.handle_xdnd_message(ev)?;
        } else if ev.window == self.tray_owner && ev.type_ == self.atoms.tray_opcode && self.tray_enabled && data[1] == 0 {
            self.dock_tray_icon(data[2])?;
        } else if ev.type_ == self.atoms.net_active_window {
            self.focus_window(ev.window)?;
        } else if ev.type_ == self.atoms.net_close_window {
            if self.clients.contains(&ev.window) {
                self.close_window(ev.window)?;
            }
        }
        Ok(())
    }

    fn close_window(&self, window: Window) -> Result<(), Box<dyn Error>> {
        let protocols = self
            .conn
            .get_property(false, window, self.atoms.wm_protocols, AtomEnum::ATOM, 0, 64)?
            .reply()?;
        let supports_delete = protocols
            .value32()
            .map(|values| values.into_iter().any(|atom| atom == self.atoms.wm_delete_window))
            .unwrap_or(false);
        if supports_delete {
            let event = ClientMessageEvent::new(
                32,
                window,
                self.atoms.wm_protocols,
                [self.atoms.wm_delete_window, CURRENT_TIME, 0, 0, 0],
            );
            self.conn.send_event(false, window, EventMask::NO_EVENT, event)?;
        } else {
            self.conn.kill_client(window)?;
        }
        Ok(())
    }

    fn dock_tray_icon(&mut self, window: Window) -> Result<(), Box<dyn Error>> {
        if window == NONE || self.tray_windows.contains(&window) {
            return Ok(());
        }
        let attrs = match self.conn.get_window_attributes(window)?.reply() {
            Ok(attrs) => attrs,
            Err(_) => return Ok(()),
        };
        if attrs.override_redirect {
            return Ok(());
        }

        self.conn.change_window_attributes(
            window,
            &ChangeWindowAttributesAux::new().event_mask(
                EventMask::STRUCTURE_NOTIFY | EventMask::PROPERTY_CHANGE,
            ),
        )?;
        self.conn.reparent_window(window, self.panel, 0, 0)?;
        self.conn.configure_window(
            window,
            &x11rb::protocol::xproto::ConfigureWindowAux::new()
                .width(TRAY_ICON_SIZE as u32)
                .height(TRAY_ICON_SIZE as u32)
                .border_width(0),
        )?;
        self.conn.map_window(window)?;
        let embedded = ClientMessageEvent::new(
            32,
            window,
            self.atoms.xembed,
            [CURRENT_TIME, 0, 0, self.panel, 0],
        );
        self.conn.send_event(false, window, EventMask::NO_EVENT, embedded)?;
        self.tray_windows.push(window);
        self.draw_panel()?;
        Ok(())
    }

    fn search_end_x(&self) -> i32 {
        let width = self.screen_width as i32;
        let maximum = (width - START_BUTTON_WIDTH - 80).max(120);
        START_BUTTON_WIDTH + self.search_width.min(maximum)
    }

    fn draw_panel(&mut self) -> Result<(), Box<dyn Error>> {
        let height = self.panel_height as i32;
        let width = self.screen_width as i32;
        let search_end = self.search_end_x();
        let search_y = 4;
        let search_height = (height - 8).max(1);

        // Windows 10's taskbar is a single, flat strip: full-height Start button,
        // inset rectangular search field, then left-aligned app buttons.
        self.fill_rect(0, 0, width, height, self.palette.panel)?;
        self.draw_start_icon()?;
        self.fill_rect(
            START_BUTTON_WIDTH,
            search_y,
            search_end - START_BUTTON_WIDTH,
            search_height,
            self.palette.search,
        )?;
        self.draw_magnifier(START_BUTTON_WIDTH + 12, height / 2 - 5, self.palette.search_text)?;
        let raw_search_text = if self.search_active {
            if self.search_text.is_empty() { "Type a command...".to_string() } else { format!("{}|", self.search_text) }
        } else {
            "Type here to search".to_string()
        };
        let search_text = truncate_ascii(&raw_search_text, ((search_end - START_BUTTON_WIDTH - 42).max(0) / 8) as usize);
        let search_color = if self.search_active { self.palette.text } else { self.palette.search_text };
        self.draw_text(
            &search_text,
            START_BUTTON_WIDTH + 34,
            height / 2 + 5,
            search_color,
            self.palette.search,
        )?;
        if self.search_active {
            self.conn.poly_rectangle(
                self.panel,
                self.gc,
                &[Rectangle { x: START_BUTTON_WIDTH as i16, y: search_y as i16, width: (search_end - START_BUTTON_WIDTH).max(1) as u16, height: search_height as u16 }],
            )?;
        }

        let tray_width = self.tray_windows.len() as i32 * TRAY_ICON_SIZE;
        let widget_texts = self
            .widgets
            .iter()
            .filter(|widget| !widget.value.trim().is_empty())
            .map(|widget| widget.value.chars().filter(|c| c.is_ascii()).collect::<String>())
            .collect::<Vec<_>>();
        let widgets_width: i32 = widget_texts.iter().map(|text| (text.len() as i32 * 8) + 22).sum();
        let network_width = (if self.network.wifi_present { NETWORK_ICON_WIDTH } else { 0 })
            + (if self.network.wired_present { NETWORK_ICON_WIDTH } else { 0 });
        let right_region = tray_width + widgets_width + network_width + PANEL_PADDING * 2;
        let tasks_end = (width - right_region).max(search_end);
        let available = (tasks_end - search_end).max(0);
        let minimum_task_width = 64;
        let visible_count = if available < minimum_task_width {
            0
        } else {
            self.clients.len().min((available / minimum_task_width) as usize)
        };
        let task_width = if visible_count == 0 {
            0
        } else {
            (available / visible_count as i32).clamp(minimum_task_width, 184)
        };

        self.task_hits.clear();
        let mut x = search_end;
        for window in self.clients.iter().take(visible_count).copied().collect::<Vec<_>>() {
            let end = (x + task_width).min(tasks_end);
            let button_width = end - x;
            let focused = self.active == Some(window);
            let background = if focused { self.palette.surface } else { self.palette.panel };
            self.fill_rect(x, 0, button_width, height, background)?;

            // The compact square glyph makes the task buttons read like classic
            // Windows 10 app buttons without depending on application icon APIs.
            let title = self.window_title(window).unwrap_or_else(|| "Application".into());
            let glyph = title
                .chars()
                .find(|character| character.is_ascii_alphanumeric())
                .unwrap_or('?')
                .to_ascii_uppercase()
                .to_string();
            let glyph_x = x + 10;
            let glyph_y = (height - 18) / 2;
            self.fill_rect(glyph_x, glyph_y, 18, 18, self.palette.accent)?;
            self.draw_text(&glyph, glyph_x + 6, glyph_y + 14, self.palette.text, self.palette.accent)?;

            if button_width >= 104 {
                let text_limit = ((button_width - 42).max(1) / 8) as usize;
                let short_title = truncate_ascii(&title, text_limit);
                self.draw_text(
                    &short_title,
                    x + 36,
                    height / 2 + 5,
                    self.palette.text,
                    background,
                )?;
            }

            // Open windows get a small bottom indicator; the focused window uses
            // the Windows-blue accent, inactive windows use a muted line.
            let indicator_color = if focused { self.palette.accent } else { self.palette.muted };
            let indicator_width = button_width.min(28).max(12);
            self.fill_rect(
                x + (button_width - indicator_width) / 2,
                height - 3,
                indicator_width,
                2,
                indicator_color,
            )?;
            self.task_hits.push((x as i16, end as i16, window));
            x = end;
        }

        let mut right_x = width - PANEL_PADDING - tray_width - network_width - widgets_width;
        for text in &widget_texts {
            let widget_width = text.len() as i32 * 8 + 22;
            self.draw_text(text, right_x + 8, height / 2 + 5, self.palette.accent, self.palette.panel)?;
            right_x += widget_width;
        }
        if self.network.wifi_present {
            self.draw_wifi_icon(right_x + 2, height / 2 - 8, self.network.wifi_connected)?;
            right_x += NETWORK_ICON_WIDTH;
        }
        if self.network.wired_present {
            self.draw_wired_icon(right_x + 2, height / 2 - 8, self.network.wired_connected)?;
        }

        let mut tray_x = width - PANEL_PADDING - tray_width;
        for window in &self.tray_windows {
            self.conn.configure_window(
                *window,
                &x11rb::protocol::xproto::ConfigureWindowAux::new()
                    .x(tray_x)
                    .y((height - TRAY_ICON_SIZE) / 2)
                    .width(TRAY_ICON_SIZE as u32)
                    .height(TRAY_ICON_SIZE as u32)
                    .border_width(0),
            )?;
            tray_x += TRAY_ICON_SIZE;
        }
        // The panel is a dock and should remain visible over floating clients.
        self.conn.configure_window(
            self.panel,
            &x11rb::protocol::xproto::ConfigureWindowAux::new().stack_mode(StackMode::ABOVE),
        )?;
        Ok(())
    }

    fn draw_start_icon(&self) -> Result<(), Box<dyn Error>> {
        if let Some(image) = &self.start_image {
            let x = ((START_BUTTON_WIDTH - image.width as i32) / 2).max(0) as i16;
            let y = ((self.panel_height as i32 - image.height as i32) / 2).max(0) as i16;
            self.conn.put_image(
                ImageFormat::Z_PIXMAP,
                self.panel,
                self.gc,
                image.width,
                image.height,
                x,
                y,
                0,
                self.screen_depth,
                &image.data,
            )?.check()?;
            return Ok(());
        }

        let icon = self.config.start_icon.trim();
        let center_y = self.panel_height as i32 / 2;
        if icon.eq_ignore_ascii_case("windows") {
            let c = self.palette.text;
            self.fill_rect(18, center_y - 9, 9, 8, c)?;
            self.fill_rect(29, center_y - 10, 9, 8, c)?;
            self.fill_rect(18, center_y + 1, 9, 8, c)?;
            self.fill_rect(29, center_y + 1, 9, 8, c)?;
        } else if icon.eq_ignore_ascii_case("hello") {
            self.draw_text("H", 25, center_y + 6, self.palette.text, self.palette.panel)?;
        } else {
            self.draw_text(&truncate_ascii(icon, 3), 19, center_y + 5, self.palette.text, self.palette.panel)?;
        }
        Ok(())
    }

    fn draw_magnifier(&self, x: i32, y: i32, color: u32) -> Result<(), Box<dyn Error>> {
        self.set_gc(color, self.palette.search)?;
        let points = [
            Point { x: (x + 4) as i16, y: y as i16 },
            Point { x: (x + 7) as i16, y: (y + 1) as i16 },
            Point { x: (x + 8) as i16, y: (y + 4) as i16 },
            Point { x: (x + 7) as i16, y: (y + 7) as i16 },
            Point { x: (x + 4) as i16, y: (y + 8) as i16 },
            Point { x: (x + 1) as i16, y: (y + 7) as i16 },
            Point { x: x as i16, y: (y + 4) as i16 },
            Point { x: (x + 1) as i16, y: (y + 1) as i16 },
            Point { x: (x + 4) as i16, y: y as i16 },
            Point { x: (x + 10) as i16, y: (y + 10) as i16 },
            Point { x: (x + 12) as i16, y: (y + 12) as i16 },
        ];
        self.conn.poly_line(CoordMode::ORIGIN, self.panel, self.gc, &points)?;
        Ok(())
    }

    fn draw_wifi_icon(&self, x: i32, y: i32, connected: bool) -> Result<(), Box<dyn Error>> {
        let color = if connected { self.palette.text } else { self.palette.muted };
        self.set_gc(color, self.palette.panel)?;
        let points = [
            Point { x: (x + 1) as i16, y: (y + 6) as i16 },
            Point { x: (x + 8) as i16, y: (y + 1) as i16 },
            Point { x: (x + 15) as i16, y: (y + 6) as i16 },
        ];
        self.conn.poly_line(CoordMode::ORIGIN, self.panel, self.gc, &points)?;
        let points2 = [
            Point { x: (x + 4) as i16, y: (y + 9) as i16 },
            Point { x: (x + 8) as i16, y: (y + 6) as i16 },
            Point { x: (x + 12) as i16, y: (y + 9) as i16 },
        ];
        self.conn.poly_line(CoordMode::ORIGIN, self.panel, self.gc, &points2)?;
        self.fill_rect(x + 7, y + 12, 3, 3, color)?;
        Ok(())
    }

    fn draw_wired_icon(&self, x: i32, y: i32, connected: bool) -> Result<(), Box<dyn Error>> {
        let color = if connected { self.palette.text } else { self.palette.muted };
        self.set_gc(color, self.palette.panel)?;
        self.conn.poly_rectangle(
            self.panel,
            self.gc,
            &[Rectangle { x: (x + 2) as i16, y: (y + 2) as i16, width: 14, height: 10 }],
        )?;
        let points = [
            Point { x: (x + 5) as i16, y: (y + 12) as i16 },
            Point { x: (x + 5) as i16, y: (y + 16) as i16 },
            Point { x: (x + 13) as i16, y: (y + 16) as i16 },
            Point { x: (x + 13) as i16, y: (y + 12) as i16 },
        ];
        self.conn.poly_line(CoordMode::ORIGIN, self.panel, self.gc, &points)?;
        Ok(())
    }

    fn window_title(&self, window: Window) -> Option<String> {
        let title = self
            .conn
            .get_property(false, window, self.atoms.net_wm_name, self.atoms.utf8_string, 0, 256)
            .ok()?
            .reply()
            .ok()?;
        if !title.value.is_empty() {
            return Some(String::from_utf8_lossy(&title.value).replace('\0', ""));
        }
        let legacy = self
            .conn
            .get_property(false, window, self.atoms.wm_name, AtomEnum::STRING, 0, 256)
            .ok()?
            .reply()
            .ok()?;
        if legacy.value.is_empty() {
            None
        } else {
            Some(String::from_utf8_lossy(&legacy.value).replace('\0', ""))
        }
    }

    fn fill_rect(&self, x: i32, y: i32, width: i32, height: i32, color: u32) -> Result<(), Box<dyn Error>> {
        self.fill_rect_on(self.panel, x, y, width, height, color)
    }

    fn fill_rect_on(&self, drawable: Window, x: i32, y: i32, width: i32, height: i32, color: u32) -> Result<(), Box<dyn Error>> {
        if width <= 0 || height <= 0 { return Ok(()); }
        self.set_gc(color, self.palette.panel)?;
        self.conn.poly_fill_rectangle(
            drawable,
            self.gc,
            &[Rectangle {
                x: x.clamp(i16::MIN as i32, i16::MAX as i32) as i16,
                y: y.clamp(i16::MIN as i32, i16::MAX as i32) as i16,
                width: width.min(u16::MAX as i32) as u16,
                height: height.min(u16::MAX as i32) as u16,
            }],
        )?;
        Ok(())
    }

    fn draw_text(&self, text: &str, x: i32, y: i32, foreground: u32, background: u32) -> Result<(), Box<dyn Error>> {
        self.draw_text_on(self.panel, text, x, y, foreground, background)
    }

    fn draw_text_on(&self, drawable: Window, text: &str, x: i32, y: i32, foreground: u32, background: u32) -> Result<(), Box<dyn Error>> {
        let safe = text.chars().filter(|character| character.is_ascii() && !character.is_control()).take(255).collect::<String>();
        if safe.is_empty() { return Ok(()); }
        self.set_gc(foreground, background)?;
        self.conn.image_text8(
            drawable,
            self.gc,
            x.clamp(i16::MIN as i32, i16::MAX as i32) as i16,
            y.clamp(i16::MIN as i32, i16::MAX as i32) as i16,
            safe.as_bytes(),
        )?;
        Ok(())
    }

    fn draw_transparent_text_on(
        &self,
        drawable: Window,
        text: &str,
        x: i32,
        y: i32,
        foreground: u32,
    ) -> Result<(), Box<dyn Error>> {
        let safe = text.chars()
            .filter(|character| character.is_ascii() && !character.is_control())
            .take(254)
            .collect::<String>();
        if safe.is_empty() {
            return Ok(());
        }
        let mut items = Vec::with_capacity(safe.len() + 2);
        items.push(safe.len() as u8);
        items.push(0); // zero character-position delta
        items.extend_from_slice(safe.as_bytes());
        self.set_gc(foreground, self.palette.panel)?;
        self.conn.poly_text8(
            drawable,
            self.gc,
            x.clamp(i16::MIN as i32, i16::MAX as i32) as i16,
            y.clamp(i16::MIN as i32, i16::MAX as i32) as i16,
            &items,
        )?;
        Ok(())
    }

    fn set_gc(&self, foreground: u32, background: u32) -> Result<(), Box<dyn Error>> {
        self.conn.change_gc(
            self.gc,
            &ChangeGCAux::new().foreground(foreground).background(background),
        )?;
        Ok(())
    }
}

fn command_exists(program: &str) -> bool {
    if program.is_empty() { return false; }
    let candidate = PathBuf::from(program);
    if candidate.components().count() > 1 || candidate.is_absolute() {
        return candidate.is_file();
    }
    std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).any(|directory| directory.join(program).is_file()))
        .unwrap_or(false)
}

fn keysym_for_keycode(conn: &RustConnection, keycode: u8, shifted: bool) -> Result<Option<u32>, Box<dyn Error>> {
    let setup = conn.setup();
    if keycode < setup.min_keycode || keycode > setup.max_keycode { return Ok(None); }
    let count = setup.max_keycode.saturating_sub(setup.min_keycode).saturating_add(1);
    let mapping = conn.get_keyboard_mapping(setup.min_keycode, count)?.reply()?;
    let columns = mapping.keysyms_per_keycode.max(1) as usize;
    let row = keycode.saturating_sub(setup.min_keycode) as usize;
    let start = row.saturating_mul(columns);
    let symbols = mapping.keysyms.get(start..start + columns).unwrap_or(&[]);
    let index = if shifted && symbols.len() > 1 { 1 } else { 0 };
    Ok(symbols.get(index).copied().filter(|symbol| *symbol != 0))
}

fn load_start_image(
    conn: &RustConnection,
    screen: &x11rb::protocol::xproto::Screen,
    configured_path: &str,
    theme: &str,
) -> Option<StartImage> {
    if configured_path.trim().is_empty() {
        return None;
    }
    let path = config::expand_home(configured_path);
    let source = if path.is_file() {
        image::open(&path).ok()
    } else {
        None
    }.or_else(|| image::load_from_memory(include_bytes!("../assets/windows.png")).ok());
    let Some(source) = source else { return None; };
    let image = source.thumbnail(30, 30).to_rgba8();
    let (width, height) = image.dimensions();
    let setup = conn.setup();
    let pixmap_format = setup.pixmap_formats.iter().find(|format| format.depth == screen.root_depth)?;
    let visual = screen.allowed_depths.iter()
        .flat_map(|depth| depth.visuals.iter())
        .find(|visual| visual.visual_id == screen.root_visual)?;
    let bits_per_pixel = pixmap_format.bits_per_pixel as u32;
    let bytes_per_pixel = ((bits_per_pixel + 7) / 8) as usize;
    let pad = pixmap_format.scanline_pad as u32;
    if bytes_per_pixel == 0 || pad == 0 { return None; }
    let line_bits = width.saturating_mul(bits_per_pixel);
    let stride = (((line_bits + pad - 1) / pad) * pad / 8) as usize;
    let mut data = vec![0_u8; stride * height as usize];
    let background = if theme.eq_ignore_ascii_case("light") { (227_u8, 227_u8, 227_u8) } else { (31_u8, 31_u8, 31_u8) };
    let little_endian = setup.image_byte_order == x11rb::protocol::xproto::ImageOrder::LSB_FIRST;

    for y in 0..height {
        for x in 0..width {
            let pixel = image.get_pixel(x, y).0;
            let alpha = pixel[3] as u32;
            let blend = |foreground: u8, backdrop: u8| -> u8 {
                ((foreground as u32 * alpha + backdrop as u32 * (255 - alpha) + 127) / 255) as u8
            };
            let red = masked_component(blend(pixel[0], background.0), visual.red_mask);
            let green = masked_component(blend(pixel[1], background.1), visual.green_mask);
            let blue = masked_component(blend(pixel[2], background.2), visual.blue_mask);
            let value = red | green | blue;
            let offset = y as usize * stride + x as usize * bytes_per_pixel;
            for byte in 0..bytes_per_pixel {
                let shift = if little_endian { byte * 8 } else { (bytes_per_pixel - byte - 1) * 8 };
                data[offset + byte] = ((value >> shift) & 0xff) as u8;
            }
        }
    }
    Some(StartImage { width: width as u16, height: height as u16, data })
}

fn masked_component(channel: u8, mask: u32) -> u32 {
    if mask == 0 { return 0; }
    let shift = mask.trailing_zeros();
    let maximum = mask >> shift;
    (((channel as u32 * maximum + 127) / 255) << shift) & mask
}

fn read_root_pixmap(
    conn: &RustConnection,
    root: Window,
    preferred_atom: Atom,
    fallback_atom: Atom,
) -> Option<Pixmap> {
    for atom in [preferred_atom, fallback_atom] {
        let Ok(cookie) = conn.get_property(false, root, atom, AtomEnum::PIXMAP, 0, 1) else {
            continue;
        };
        let Ok(property) = cookie.reply() else {
            continue;
        };
        let pixmap = property.value32().and_then(|mut values| values.next());
        if let Some(pixmap) = pixmap {
            return Some(pixmap);
        }
    }
    None
}

fn intern_atom(conn: &RustConnection, name: &str) -> Result<Atom, Box<dyn Error>> {
    Ok(conn.intern_atom(false, name.as_bytes())?.reply()?.atom)
}

fn allocate_palette(
    conn: &RustConnection,
    colormap: u32,
    theme: &str,
) -> Result<Palette, Box<dyn Error>> {
    let light = theme.eq_ignore_ascii_case("light");
    let color = |hex: u32| -> Result<u32, Box<dyn Error>> {
        let red = ((hex >> 16) & 0xff) as u16 * 257;
        let green = ((hex >> 8) & 0xff) as u16 * 257;
        let blue = (hex & 0xff) as u16 * 257;
        Ok(conn.alloc_color(colormap, red, green, blue)?.reply()?.pixel)
    };
    Ok(if light {
        Palette {
            panel: color(0xe3e3e3)?,
            surface: color(0xf5f5f5)?,
            search: color(0xffffff)?,
            search_text: color(0x4a4a4a)?,
            text: color(0x202020)?,
            muted: color(0x777777)?,
            accent: color(0x0078d7)?,
            border: color(0xc8c8c8)?,
        }
    } else {
        Palette {
            panel: color(0x1f1f1f)?,
            surface: color(0x2d2d2d)?,
            search: color(0x2b2b2b)?,
            search_text: color(0xe2e2e2)?,
            text: color(0xf1f1f1)?,
            muted: color(0xa0a0a0)?,
            accent: color(0x0078d7)?,
            border: color(0x353535)?,
        }
    })
}

fn allocate_palette_for_window(
    conn: &RustConnection,
    panel: Window,
    theme: &str,
) -> Result<Palette, Box<dyn Error>> {
    let attributes = conn.get_window_attributes(panel)?.reply()?;
    allocate_palette(conn, attributes.colormap, theme)
}

fn keysym_from_name(name: &str) -> Option<u32> {
    match name {
        "Return" | "Enter" => Some(0xff0d),
        "Escape" | "Esc" => Some(0xff1b),
        "Tab" => Some(0xff09),
        "Space" => Some(0x0020),
        "BackSpace" => Some(0xff08),
        "Delete" => Some(0xffff),
        "Left" => Some(0xff51),
        "Up" => Some(0xff52),
        "Right" => Some(0xff53),
        "Down" => Some(0xff54),
        "F1" => Some(0xffbe),
        "F2" => Some(0xffbf),
        "F3" => Some(0xffc0),
        "F4" => Some(0xffc1),
        "F5" => Some(0xffc2),
        "F6" => Some(0xffc3),
        "F7" => Some(0xffc4),
        "F8" => Some(0xffc5),
        "F9" => Some(0xffc6),
        "F10" => Some(0xffc7),
        "F11" => Some(0xffc8),
        "F12" => Some(0xffc9),
        value if value.len() == 1 => Some(value.as_bytes()[0] as u32),
        _ => None,
    }
}

fn keycode_for_keysym(conn: &RustConnection, keysym: u32) -> Result<Option<u8>, Box<dyn Error>> {
    let setup = conn.setup();
    let first = setup.min_keycode;
    let count = setup.max_keycode.saturating_sub(first).saturating_add(1);
    let mapping = conn.get_keyboard_mapping(first, count)?.reply()?;
    let columns = mapping.keysyms_per_keycode.max(1) as usize;
    for (index, group) in mapping.keysyms.chunks(columns).enumerate() {
        if group.iter().any(|candidate| *candidate == keysym) {
            return Ok(Some(first.saturating_add(index as u8)));
        }
    }
    Ok(None)
}

fn spawn_shell(command: &str) {
    if command.trim().is_empty() {
        return;
    }
    let result = Command::new("sh")
        .arg("-c")
        .arg(command)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    if let Err(error) = result {
        eprintln!("HelloWM: could not launch '{command}': {error}");
    }
}

fn read_network_state() -> NetworkState {
    let output = Command::new("nmcli")
        .args(["-t", "-f", "TYPE,STATE", "device"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output();
    let Ok(output) = output else { return NetworkState::default(); };
    if !output.status.success() {
        return NetworkState::default();
    }
    let mut state = NetworkState::default();
    let contents = String::from_utf8_lossy(&output.stdout);
    for line in contents.lines() {
        let Some((kind, status)) = line.split_once(':') else { continue; };
        let connected = status.starts_with("connected");
        match kind {
            "wifi" => {
                state.wifi_present = true;
                state.wifi_connected |= connected;
            }
            "ethernet" => {
                state.wired_present = true;
                state.wired_connected |= connected;
            }
            _ => {}
        }
    }
    state
}

fn run_widget_command(spec: &WidgetConfig) -> String {
    if spec.command.trim().is_empty() {
        return String::new();
    }
    let output = Command::new("sh")
        .arg("-c")
        .arg(&spec.command)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output();
    let Ok(output) = output else { return String::new(); };
    if !output.status.success() {
        return String::new();
    }
    let value = String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .chars()
        .filter(|character| character.is_ascii() && !character.is_control())
        .take(32)
        .collect::<String>();
    if value.is_empty() { String::new() } else { value }
}

fn truncate_ascii(text: &str, max_chars: usize) -> String {
    let filtered = text
        .chars()
        .filter(|character| character.is_ascii() && !character.is_control())
        .collect::<String>();
    if filtered.chars().count() <= max_chars {
        filtered
    } else if max_chars <= 1 {
        filtered.chars().take(max_chars).collect()
    } else {
        format!("{}…", filtered.chars().take(max_chars - 1).collect::<String>())
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let arguments = std::env::args().skip(1).collect::<Vec<_>>();
    if arguments.first().is_some_and(|argument| argument == "--create-launcher") {
        if arguments.len() < 3 {
            return Err("usage: hellowm --create-launcher <name> <command>".into());
        }
        let desktop = Desktop::new()?;
        let created = desktop.create_launcher(&arguments[1], &arguments[2])?;
        println!("Created desktop launcher: {}", created.display());
        return Ok(());
    }
    if !arguments.is_empty() && arguments[0] != "--help" {
        eprintln!("HelloWM: unknown option '{}'.", arguments[0]);
        eprintln!("Usage: hellowm [--create-launcher <name> <command>]");
        std::process::exit(2);
    }
    if arguments.first().is_some_and(|argument| argument == "--help") {
        println!("HelloWM - lightweight floating X11 window manager
Usage: hellowm [--create-launcher <name> <command>]");
        return Ok(());
    }
    let mut manager = Manager::new()?;
    eprintln!("HelloWM: running. Super+Enter opens a terminal; Super+Q closes the focused window; Super+D focuses taskbar search.");
    manager.run()
}
