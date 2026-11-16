use crate::storage::{canonical_user_path, create_user_directories, safe_user_path, AppPaths, Settings};
use crate::terminal::{TerminalState, TerminalTask};
use crate::trash::{self, TrashEntry};
use crate::apps;
use crate::{isolation, unikey, unicode, security_tools::{self, SecurityTool}};
use eframe::egui::{self, Color32, FontId, RichText, TextEdit};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};
use std::process::Child;
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum UniKeyProcessKind { Download, Running }

#[derive(Clone, Copy)]
enum SnapSide { Left, Right, Maximize }

/// Single source of truth for desktop shortcuts, Start pins and the All apps view.
#[derive(Clone)]
struct CachedFileEntry {
    path: PathBuf,
    name: String,
    is_dir: bool,
    display_size: String,
}

#[derive(Clone, Copy)]
struct StartApp {
    name: &'static str,
    icon: &'static str,
    description: &'static str,
    category: &'static str,
}

const START_APPS: [StartApp; 15] = [
    StartApp { name: "Files", icon: "▦", description: "Browse your portable workspace", category: "Workspace" },
    StartApp { name: "Terminal", icon: "⌘", description: "Commands and Wi-Fi tools", category: "Workspace" },
    StartApp { name: "Editor", icon: "✎", description: "Write and edit text files", category: "Productivity" },
    StartApp { name: "Calculator", icon: "∑", description: "Quick calculations", category: "Productivity" },
    StartApp { name: "Monitor", icon: "◉", description: "CPU, memory and processes", category: "System" },
    StartApp { name: "Trash", icon: "▤", description: "Restore deleted NOVA files", category: "System" },
    StartApp { name: "Google Chrome", icon: "🌐", description: "Normal or isolated browser", category: "Internet" },
    StartApp { name: "UniKey", icon: "文", description: "Vietnamese Telex and VNI", category: "Productivity" },
    StartApp { name: "Security Center", icon: "🛡", description: "Sandbox and security workspaces", category: "Security" },
    StartApp { name: "Network Discovery", icon: "◎", description: "Zenmap visual discovery", category: "Security" },
    StartApp { name: "Vulnerability Management", icon: "◈", description: "Greenbone management console", category: "Security" },
    StartApp { name: "Web Security", icon: "⌁", description: "OWASP ZAP workspace", category: "Security" },
    StartApp { name: "Packet Analyzer", icon: "▥", description: "Wireshark packet analysis", category: "Security" },
    StartApp { name: "Web / API Inspector", icon: "⇄", description: "Burp Suite web and API testing", category: "Security" },
    StartApp { name: "Settings", icon: "⚙", description: "Theme, accent and motion", category: "System" },
];

const START_PINS: [&str; 12] = [
    "Files", "Terminal", "Editor", "Calculator", "Monitor", "Google Chrome",
    "UniKey", "Security Center", "Settings", "Trash", "Network Discovery", "Packet Analyzer",
];

pub struct NovaApp {
    paths: AppPaths,
    settings: Settings,
    terminal: TerminalState,
    terminal_job_receiver: Option<Receiver<String>>,
    terminal_busy: bool,
    terminal_open: bool,
    files_open: bool,
    editor_open: bool,
    settings_open: bool,
    calculator_open: bool,
    monitor_open: bool,
    start_open: bool,
    start_show_all: bool,
    trash_open: bool,
    chrome_open: bool,
    unikey_open: bool,
    security_open: bool,
    security_tool_open: [bool; 5],
    security_tool_status: [String; 5],
    security_tool_installed: [bool; 5],
    security_tool_paths: [Option<PathBuf>; 5],
    security_tools_checked: bool,
    sandbox_available: Option<bool>,
    security_scope: String,
    security_scope_confirmed: bool,
    greenbone_url: String,
    security_confirm_clear_quarantine: bool,
    chrome_confirm_host: bool,
    trash_confirm_empty: bool,
    trash_confirm_delete: bool,
    trash_entries: Vec<TrashEntry>,
    trash_selected: Option<String>,
    files_selected: Option<PathBuf>,
    files_entries: Vec<CachedFileEntry>,
    files_cache_dir: Option<PathBuf>,
    files_error: Option<String>,
    files_grid_view: bool,
    files_search: String,
    notification_open: bool,
    settings_page: usize,
    window_snap_requests: HashMap<String, SnapSide>,
    icon_galleys: HashMap<(&'static str, u32, u32, bool), std::sync::Arc<egui::Galley>>,
    start_search: String,
    chrome_path: Option<PathBuf>,
    chrome_checked: bool,
    unikey_path: Option<PathBuf>,
    unikey_checked: bool,
    quarantine_count_loaded: bool,
    unikey_child: Option<Child>,
    unikey_process_kind: Option<UniKeyProcessKind>,
    unikey_status: String,
    quarantine_count: usize,
    chrome_url: String,
    files_dir: PathBuf,
    files_new_folder: String,
    editor_path: String,
    editor_text: String,
    editor_status: String,
    calc_left: String,
    calc_right: String,
    calc_operator: String,
    calc_result: String,
    monitor: Option<System>,
    monitor_error: Option<String>,
    last_monitor_refresh: Instant,
    last_process_refresh: Instant,
    applied_theme: String,
    applied_effective_theme: String,
    applied_accent: String,
    status_message: String,
}

impl NovaApp {
    pub fn new(cc: &eframe::CreationContext<'_>, paths: AppPaths) -> Self {
        let (settings, settings_warning) = match paths.load_settings() {
            Ok(settings) => (settings, None),
            Err(error) => (Settings::default(), Some(format!("Settings could not be loaded; defaults are active: {error}"))),
        };
        let portable = paths.portable;
        // Host discovery, quarantine enumeration and system telemetry are deferred
        // until their corresponding workspace is first opened.
        let mut app = Self {
            terminal: TerminalState::new(&paths),
            terminal_job_receiver: None,
            terminal_busy: false,
            files_dir: paths.user_dir.clone(),
            paths,
            settings,
            terminal_open: false,
            files_open: false,
            editor_open: false,
            settings_open: false,
            calculator_open: false,
            monitor_open: false,
            start_open: false,
            start_show_all: false,
            trash_open: false,
            chrome_open: false,
            unikey_open: false,
            security_open: false,
            security_tool_open: [false; 5],
            security_tool_status: std::array::from_fn(|_| "Ready. Tool is not started.".to_owned()),
            security_tool_installed: [false; 5],
            security_tool_paths: std::array::from_fn(|_| None),
            security_tools_checked: false,
            sandbox_available: None,
            security_scope: String::new(),
            security_scope_confirmed: false,
            greenbone_url: "https://127.0.0.1:9392".to_owned(),
            security_confirm_clear_quarantine: false,
            chrome_confirm_host: false,
            trash_confirm_empty: false,
            trash_confirm_delete: false,
            trash_entries: Vec::new(),
            trash_selected: None,
            files_selected: None,
            files_entries: Vec::new(),
            files_cache_dir: None,
            files_error: None,
            files_grid_view: false,
            files_search: String::new(),
            notification_open: false,
            settings_page: 0,
            window_snap_requests: HashMap::new(),
            icon_galleys: HashMap::new(),
            start_search: String::new(),
            chrome_path: None,
            chrome_checked: false,
            chrome_url: "https://www.google.com".to_owned(),
            unikey_path: None,
            unikey_checked: false,
            quarantine_count_loaded: false,
            unikey_child: None,
            unikey_process_kind: None,
            unikey_status: "UniKey is not running.".to_owned(),
            quarantine_count: 0,
            files_new_folder: String::new(),
            editor_path: "Documents/Note.txt".to_owned(),
            editor_text: String::new(),
            editor_status: "Ready".to_owned(),
            calc_left: "12".to_owned(),
            calc_right: "3".to_owned(),
            calc_operator: "+".to_owned(),
            calc_result: "15".to_owned(),
            monitor: None,
            monitor_error: None,
            last_monitor_refresh: Instant::now() - Duration::from_secs(2),
            last_process_refresh: Instant::now() - Duration::from_secs(10),
            applied_theme: String::new(),
            applied_effective_theme: String::new(),
            applied_accent: String::new(),
            status_message: settings_warning.unwrap_or_else(|| {
                if portable {
                    "Portable mode active — data is stored beside NOVA.exe".to_owned()
                } else {
                    "Local mode active — user data is stored in LocalAppData".to_owned()
                }
            }),
        };
        app.apply_theme(&cc.egui_ctx);
        app
    }

    fn apply_theme(&mut self, ctx: &egui::Context) {
        let preference = match self.settings.theme.as_str() {
            "light" => egui::ThemePreference::Light,
            "system" => egui::ThemePreference::System,
            _ => egui::ThemePreference::Dark,
        };
        let effective_dark = match self.settings.theme.as_str() {
            "light" => false,
            "system" => ctx.system_theme().unwrap_or(egui::Theme::Dark) == egui::Theme::Dark,
            _ => true,
        };
        let effective_theme = if effective_dark { "dark" } else { "light" };
        if self.applied_theme == self.settings.theme
            && self.applied_effective_theme == effective_theme
            && self.applied_accent == self.settings.accent
        {
            return;
        }
        ctx.set_theme(preference);
        let mut visuals = if effective_dark { egui::Visuals::dark() } else { egui::Visuals::light() };
        if effective_dark {
            visuals.panel_fill = Color32::from_rgb(17, 23, 38);
            visuals.window_fill = Color32::from_rgb(23, 31, 49);
            visuals.extreme_bg_color = Color32::from_rgb(10, 15, 27);
        } else {
            visuals.panel_fill = Color32::from_rgb(242, 245, 250);
            visuals.window_fill = Color32::from_rgb(255, 255, 255);
            visuals.extreme_bg_color = Color32::from_rgb(233, 237, 244);
        }
        let accent = match self.settings.accent.as_str() {
            "violet" => Color32::from_rgb(151, 120, 255),
            "teal" => Color32::from_rgb(50, 190, 174),
            "rose" => Color32::from_rgb(238, 112, 151),
            "amber" => Color32::from_rgb(232, 171, 76),
            _ => Color32::from_rgb(92, 139, 245),
        };
        visuals.selection.bg_fill = accent;
        visuals.hyperlink_color = accent;
        visuals.window_corner_radius = egui::CornerRadius::same(15);
        visuals.menu_corner_radius = egui::CornerRadius::same(12);
        visuals.window_shadow = egui::Shadow {
            offset: [0, 12],
            blur: 28,
            spread: 0,
            color: Color32::from_black_alpha(86),
        };
        ctx.set_visuals(visuals);
        ctx.all_styles_mut(|style| {
            style.spacing.item_spacing = egui::vec2(10.0, 8.0);
            style.spacing.button_padding = egui::vec2(12.0, 8.0);
            style.visuals.window_corner_radius = egui::CornerRadius::same(15);
            style.spacing.window_margin = egui::Margin::same(14);
        });
        self.applied_theme.clone_from(&self.settings.theme);
        self.applied_effective_theme = effective_theme.to_owned();
        self.applied_accent.clone_from(&self.settings.accent);
    }

    fn is_dark_mode(&self, ctx: &egui::Context) -> bool {
        if self.settings.theme == "system" {
            ctx.theme() == egui::Theme::Dark
        } else {
            self.settings.theme != "light"
        }
    }

    fn open_app(&mut self, app: &str) {
        match app {
            "Files" => {
                self.files_open = true;
                self.ensure_files_loaded();
            },
            "Terminal" => self.terminal_open = true,
            "Editor" => self.editor_open = true,
            "Settings" => self.settings_open = true,
            "Calculator" => self.calculator_open = true,
            "Monitor" => self.monitor_open = true,
            "Trash" => {
                self.trash_open = true;
                self.refresh_trash();
            }
            "Google Chrome" => {
                self.chrome_open = true;
                self.detect_chrome_if_needed();
            },
            "UniKey" => {
                self.unikey_open = true;
                self.detect_unikey_if_needed();
            },
            "Security Center" => {
                self.security_open = true;
                self.load_quarantine_count_if_needed();
            },
            "Network Discovery" => self.security_tool_open[SecurityTool::NetworkDiscovery.index()] = true,
            "Vulnerability Management" => self.security_tool_open[SecurityTool::VulnerabilityManagement.index()] = true,
            "Web Security" => self.security_tool_open[SecurityTool::WebSecurity.index()] = true,
            "Packet Analyzer" => self.security_tool_open[SecurityTool::PacketAnalyzer.index()] = true,
            "Web / API Inspector" => self.security_tool_open[SecurityTool::WebApiInspector.index()] = true,
            _ => {}
        }
        self.start_open = false;
        self.start_show_all = false;
    }

    /// Clicking an active taskbar item hides its workspace window; clicking it
    /// again from Start or a desktop shortcut brings it back.
    fn toggle_app(&mut self, app: &str) {
        match app {
            "Files" => {
                self.files_open = !self.files_open;
                if self.files_open { self.ensure_files_loaded(); }
            },
            "Terminal" => self.terminal_open = !self.terminal_open,
            "Editor" => self.editor_open = !self.editor_open,
            "Settings" => self.settings_open = !self.settings_open,
            "Calculator" => self.calculator_open = !self.calculator_open,
            "Monitor" => self.monitor_open = !self.monitor_open,
            "Trash" => {
                self.trash_open = !self.trash_open;
                if self.trash_open { self.refresh_trash(); }
            }
            "Google Chrome" => {
                self.chrome_open = !self.chrome_open;
                if self.chrome_open { self.detect_chrome_if_needed(); }
            },
            "UniKey" => {
                self.unikey_open = !self.unikey_open;
                if self.unikey_open { self.detect_unikey_if_needed(); }
            },
            "Security Center" => {
                self.security_open = !self.security_open;
                if self.security_open { self.load_quarantine_count_if_needed(); }
            },
            "Network Discovery" => { let i = SecurityTool::NetworkDiscovery.index(); self.security_tool_open[i] = !self.security_tool_open[i]; },
            "Vulnerability Management" => { let i = SecurityTool::VulnerabilityManagement.index(); self.security_tool_open[i] = !self.security_tool_open[i]; },
            "Web Security" => { let i = SecurityTool::WebSecurity.index(); self.security_tool_open[i] = !self.security_tool_open[i]; },
            "Packet Analyzer" => { let i = SecurityTool::PacketAnalyzer.index(); self.security_tool_open[i] = !self.security_tool_open[i]; },
            "Web / API Inspector" => { let i = SecurityTool::WebApiInspector.index(); self.security_tool_open[i] = !self.security_tool_open[i]; },
            _ => {}
        }
    }

    fn detect_chrome_if_needed(&mut self) {
        if !self.chrome_checked {
            self.chrome_path = apps::find_chrome();
            self.chrome_checked = true;
        }
    }

    fn detect_unikey_if_needed(&mut self) {
        if !self.unikey_checked {
            self.unikey_path = unikey::find_portable(&self.paths.app_dir);
            self.unikey_checked = true;
        }
    }

    fn load_security_detection_if_needed(&mut self) {
        if !self.security_tools_checked {
            for info in &security_tools::TOOLS {
                let index = info.tool.index();
                let detected = if info.requires_local_executable {
                    info.tool.detect(&self.paths.app_dir)
                } else {
                    None
                };
                self.security_tool_installed[index] = !info.requires_local_executable || detected.is_some();
                self.security_tool_paths[index] = detected;
            }
            self.sandbox_available = Some(isolation::is_available());
            self.security_tools_checked = true;
        }
    }

    fn load_quarantine_count_if_needed(&mut self) {
        if !self.quarantine_count_loaded {
            self.quarantine_count = isolation::quarantine_item_count(&self.paths).unwrap_or(0);
            self.quarantine_count_loaded = true;
        }
    }

    fn ensure_files_loaded(&mut self) {
        if self.files_cache_dir.as_ref() != Some(&self.files_dir) {
            self.refresh_files();
        }
    }

    /// Enumerate the current folder only when it changes or the user requests refresh.
    /// The cached rows avoid read_dir, canonicalization and metadata calls on every frame.
    fn refresh_files(&mut self) {
        let folder = match canonical_user_path(&self.paths.user_dir, &self.files_dir) {
            Ok(folder) if folder.is_dir() => folder,
            Ok(_) => {
                self.files_error = Some("The current path is not a folder.".to_owned());
                self.files_entries.clear();
                self.files_cache_dir = None;
                return;
            }
            Err(error) => {
                self.files_error = Some(format!("Cannot access folder: {error}"));
                self.files_entries.clear();
                self.files_cache_dir = None;
                return;
            }
        };
        let entries = match fs::read_dir(&folder) {
            Ok(entries) => entries,
            Err(error) => {
                self.files_error = Some(format!("Cannot read folder: {error}"));
                self.files_entries.clear();
                self.files_cache_dir = None;
                return;
            }
        };
        let mut rows = Vec::new();
        for entry in entries.filter_map(Result::ok) {
            let Ok(kind) = entry.file_type() else { continue };
            if kind.is_symlink() { continue; }
            let path = entry.path();
            if canonical_user_path(&self.paths.user_dir, &path).is_err() { continue; }
            let name = entry.file_name().to_string_lossy().to_string();
            let is_dir = kind.is_dir();
            let display_size = entry.metadata().ok()
                .filter(|meta| meta.is_file())
                .map(|meta| format_size(meta.len()))
                .unwrap_or_else(|| if is_dir { "Folder".to_owned() } else { "File".to_owned() });
            rows.push(CachedFileEntry { path, name, is_dir, display_size });
        }
        rows.sort_by_key(|entry| (!entry.is_dir, entry.name.to_lowercase()));
        self.files_entries = rows;
        self.files_dir = folder.clone();
        self.files_cache_dir = Some(folder);
        self.files_error = None;
    }

    fn refresh_trash(&mut self) {
        match trash::list_trash(&self.paths) {
            Ok(entries) => {
                self.trash_entries = entries;
                if self.trash_selected.as_ref().is_some_and(|selected| !self.trash_entries.iter().any(|entry| &entry.id == selected)) {
                    self.trash_selected = None;
                }
            }
            Err(error) => self.status_message = format!("Could not read NOVA Trash: {error}"),
        }
    }

    fn launch_chrome(&mut self) {
        match self.chrome_path.as_ref() {
            Some(path) => match apps::launch_chrome(path, &self.chrome_url) {
                Ok(()) => self.status_message = "Opened the address in Google Chrome on the Windows host.".to_owned(),
                Err(error) => self.status_message = format!("Could not launch Google Chrome: {error}"),
            },
            None => self.status_message = "Google Chrome is not installed on this Windows user or machine. Use the official download page below.".to_owned(),
        }
    }

    fn poll_unikey_child(&mut self) {
        let result = match self.unikey_child.as_mut() {
            Some(child) => match child.try_wait() {
                Ok(Some(status)) => Some(Ok(status.success())),
                Ok(None) => None,
                Err(error) => Some(Err(error.to_string())),
            },
            None => None,
        };
        if let Some(result) = result {
            let kind = self.unikey_process_kind.take();
            self.unikey_child = None;
            self.unikey_path = unikey::find_portable(&self.paths.app_dir);
            self.unikey_checked = true;
            match (kind, result) {
                (Some(UniKeyProcessKind::Download), Ok(true)) if self.unikey_path.is_some() => {
                    self.unikey_status = "UniKey downloaded and its digital signature was verified. Choose Start UniKey.".to_owned();
                }
                (Some(UniKeyProcessKind::Download), Ok(_)) => {
                    self.unikey_status = "UniKey download did not complete. Review the PowerShell window and try again.".to_owned();
                }
                (Some(UniKeyProcessKind::Running), Ok(true)) => {
                    self.unikey_status = "UniKey has exited.".to_owned();
                }
                (Some(UniKeyProcessKind::Running), Ok(false)) => {
                    self.unikey_status = "UniKey was closed.".to_owned();
                }
                (_, Err(error)) => self.unikey_status = format!("Could not monitor UniKey process: {error}"),
                _ => self.unikey_status = "UniKey process finished.".to_owned(),
            }
        }
    }

    fn download_unikey(&mut self) {
        if self.unikey_child.is_some() {
            self.unikey_status = "A UniKey-related process is already running.".to_owned();
            return;
        }
        match unikey::start_download_helper(&self.paths.app_dir) {
            Ok(child) => {
                self.unikey_child = Some(child);
                self.unikey_process_kind = Some(UniKeyProcessKind::Download);
                self.unikey_status = "Downloading UniKey from unikey.org and verifying its publisher…".to_owned();
            }
            Err(error) => self.unikey_status = format!("Could not start UniKey download helper: {error}"),
        }
    }

    fn start_unikey(&mut self) {
        self.unikey_path = unikey::find_portable(&self.paths.app_dir);
        self.unikey_checked = true;
        let Some(path) = self.unikey_path.clone() else {
            self.unikey_status = "UniKey is not on this USB yet. Use Download UniKey first.".to_owned();
            return;
        };
        if self.unikey_child.is_some() {
            self.unikey_status = "A UniKey-related process is already running.".to_owned();
            return;
        }
        match unikey::launch_verified(&path) {
            Ok(child) => {
                self.unikey_child = Some(child);
                self.unikey_process_kind = Some(UniKeyProcessKind::Running);
                self.unikey_status = "UniKey started. Choose Unicode + Telex or VNI in the UniKey tray menu.".to_owned();
            }
            Err(error) => self.unikey_status = format!("UniKey launch blocked: {error}"),
        }
    }

    fn stop_unikey(&mut self) {
        if let Some(mut child) = self.unikey_child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.unikey_process_kind = None;
        self.unikey_status = "NOVA stopped the UniKey process it started.".to_owned();
    }

    fn launch_chrome_in_sandbox(&mut self) {
        let Some(chrome) = self.chrome_path.clone() else {
            self.status_message = "Install Google Chrome on the Windows host first; the isolated launcher maps only its read-only application folder.".to_owned();
            return;
        };
        let url = match apps::normalized_browser_target(&self.chrome_url) {
            Ok(url) => url,
            Err(error) => { self.status_message = format!("Invalid browser address: {error}"); return; }
        };
        match isolation::launch_isolated_chrome(&self.paths, &chrome, &url) {
            Ok(_) => self.status_message = "Started isolated Chrome. Its profile and downloads live only inside Windows Sandbox; close Sandbox to discard them.".to_owned(),
            Err(error) => self.status_message = format!("Could not start isolated Chrome: {error}"),
        }
    }

    fn security_window(&mut self, ctx: &egui::Context) {
        if self.security_open {
            self.load_quarantine_count_if_needed();
            self.load_security_detection_if_needed();
        }
        let sandbox_available = self.sandbox_available.unwrap_or(false);
        let installed_count = security_tools::TOOLS.iter()
            .filter(|info| info.requires_local_executable && self.security_tool_installed[info.tool.index()])
            .count();
        let mut scope_ready = security_tools::validate_authorized_scope(
            &self.security_scope,
            self.security_scope_confirmed,
        ).is_ok();
        let mut open = self.security_open;
        let mut clear_requested = false;
        let mut refresh_detection = false;
        let mut selected_tool: Option<SecurityTool> = None;

        egui::Window::new("NOVA Security Center")
            .open(&mut open)
            .fade_in(!self.settings.reduce_motion)
            .fade_out(!self.settings.reduce_motion)
            .default_size([820.0, 690.0])
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.heading(RichText::new("Security Center").size(25.0).strong());
                        ui.label(RichText::new("A guided control room for authorized security work.").weak());
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new("NOVA SECURITY").small().strong().color(Color32::from_rgb(117, 169, 250)));
                    });
                });
                ui.add_space(8.0);
                if ui.small_button("↻ Refresh app detection").clicked() {
                    refresh_detection = true;
                }

                egui::Grid::new("nova-security-overview-cards")
                    .num_columns(3)
                    .spacing(egui::vec2(10.0, 8.0))
                    .show(ui, |ui| {
                        egui::Frame::group(ui.style()).show(ui, |ui| {
                            ui.set_min_width(175.0);
                            ui.label(RichText::new("GUI TOOLS").small().weak().strong());
                            ui.heading(format!("{installed_count}/4"));
                            ui.label(RichText::new("local desktop engines detected").small().weak());
                        });
                        egui::Frame::group(ui.style()).show(ui, |ui| {
                            ui.set_min_width(175.0);
                            ui.label(RichText::new("WINDOWS SANDBOX").small().weak().strong());
                            ui.heading(if sandbox_available { "Detected" } else { "Unavailable" });
                            ui.label(RichText::new("feature + virtualization required").small().weak());
                        });
                        egui::Frame::group(ui.style()).show(ui, |ui| {
                            ui.set_min_width(175.0);
                            ui.label(RichText::new("TEST SCOPE").small().weak().strong());
                            ui.heading(if scope_ready { "Confirmed" } else { "Not confirmed" });
                            ui.label(RichText::new("operator acknowledgement").small().weak());
                        });
                        ui.end_row();
                    });

                ui.add_space(8.0);
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.heading("1. Authorized test scope");
                    ui.label("Enter only assets you own or are explicitly authorized to assess. Example: lab.example.test; 192.0.2.10/32");
                    let scope_response = ui.add(TextEdit::singleline(&mut self.security_scope)
                        .hint_text("Authorized domains, IPs or lab range")
                        .desired_width(f32::INFINITY));
                    if scope_response.changed() {
                        // Re-acknowledgement is required whenever the target scope changes.
                        // scope_ready is recomputed immediately below from this updated state.
                        self.security_scope_confirmed = false;
                    }
                    ui.checkbox(&mut self.security_scope_confirmed, "I own these assets or have written permission to test them.");
                    scope_ready = security_tools::validate_authorized_scope(
                        &self.security_scope,
                        self.security_scope_confirmed,
                    ).is_ok();
                    if scope_ready {
                        ui.colored_label(Color32::from_rgb(87, 190, 140), "Scope acknowledged. Keep every external tool's target list inside this scope.");
                    } else {
                        ui.colored_label(Color32::from_rgb(225, 166, 86), "Opening a testing tool is disabled until a scope is entered and acknowledged.");
                    }
                    ui.label(RichText::new("Important: this acknowledgement is a workflow guard, not a technical firewall. The external tools still have their own target controls; NOVA does not enforce their scan scope.").small().weak());
                });

                ui.add_space(8.0);
                ui.heading("2. GUI security applications");
                ui.label(RichText::new("Open a dedicated NOVA workspace for each tool. Engines are installed separately and are not bundled in the USB image.").small().weak());
                egui::Grid::new("nova-security-tool-cards")
                    .num_columns(2)
                    .spacing(egui::vec2(12.0, 10.0))
                    .show(ui, |ui| {
                        for (position, info) in security_tools::TOOLS.iter().enumerate() {
                            let installed = self.security_tool_installed[info.tool.index()];
                            egui::Frame::group(ui.style()).show(ui, |ui| {
                                ui.set_min_width(310.0);
                                ui.set_min_height(112.0);
                                ui.horizontal(|ui| {
                                    ui.heading(RichText::new(match position {
                                        0 => "◎",
                                        1 => "◈",
                                        2 => "⌁",
                                        3 => "▥",
                                        _ => "⇄",
                                    }).size(24.0).color(Color32::from_rgb(123, 164, 245)));
                                    ui.vertical(|ui| {
                                        ui.label(RichText::new(info.name).strong());
                                        ui.label(RichText::new(info.executable).small().weak());
                                    });
                                });
                                ui.label(RichText::new(info.description).small());
                                ui.horizontal(|ui| {
                                    if info.requires_local_executable && installed {
                                        ui.colored_label(Color32::from_rgb(87, 190, 140), "Installed");
                                    } else if info.requires_local_executable {
                                        ui.colored_label(Color32::from_rgb(225, 166, 86), "Not detected");
                                    } else {
                                        ui.colored_label(Color32::from_rgb(225, 166, 86), "Separate GSA server required");
                                    }
                                    if ui.add_enabled(scope_ready, egui::Button::new("Open workspace")).clicked() {
                                        selected_tool = Some(info.tool);
                                    }
                                });
                            });
                            if position % 2 == 1 { ui.end_row(); }
                        }
                    });

                ui.add_space(8.0);
                egui::CollapsingHeader::new("3. Isolation and disposable analysis")
                    .default_open(false)
                    .show(ui, |ui| {
                        if sandbox_available {
                            ui.colored_label(Color32::from_rgb(87, 190, 140), "Windows Sandbox launcher detected. NOVA will fail closed if the optional feature cannot start.");
                        } else {
                            ui.colored_label(Color32::from_rgb(225, 166, 86), "Windows Sandbox is not available on this Windows installation.");
                        }
                        ui.label("Requires Windows Pro, Enterprise or Education (not Home), hardware virtualization enabled, at least 4 GB RAM (8 GB recommended), 1 GB free disk space and 2 CPU cores.");
                        ui.label("Inspect in Sandbox stages a copy of one file, maps it read-only, disables networking and clipboard, then opens Explorer. Isolated Chrome uses a new disposable profile and networking; it may reach local-network services.");
                        ui.label("NOVA, Terminal/CMD, UniKey, Wi-Fi and ordinary host applications still run on Windows itself. No isolation feature can promise protection against every Windows, hypervisor or firmware vulnerability.");
                        ui.horizontal(|ui| {
                            ui.label(format!("Staged sample/config items: {}", self.quarantine_count));
                            if ui.add_enabled(self.quarantine_count > 0, egui::Button::new("Clear staged copies…")).clicked() {
                                clear_requested = true;
                            }
                        });
                    });
            });

        self.security_open = open;
        if refresh_detection {
            self.security_tools_checked = false;
            self.load_security_detection_if_needed();
        }
        if let Some(tool) = selected_tool {
            self.security_tool_open[tool.index()] = true;
        }
        if clear_requested {
            self.security_confirm_clear_quarantine = true;
        }
        if self.security_confirm_clear_quarantine {
            egui::Window::new("Clear NOVA quarantine staging?")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label("Close every Windows Sandbox window before removing staged copies and NOVA .wsb launch files. Original workspace files are not deleted.");
                    ui.horizontal(|ui| {
                        if ui.button("Clear staged copies").clicked() {
                            match isolation::clear_quarantine(&self.paths) {
                                Ok(count) => self.status_message = format!("Removed {count} staged sample/config item(s). Original workspace files were not changed."),
                                Err(error) => self.status_message = format!("Could not clear staged samples: {error}"),
                            }
                            self.quarantine_count = isolation::quarantine_item_count(&self.paths).unwrap_or(0);
                            self.quarantine_count_loaded = true;
                            self.security_confirm_clear_quarantine = false;
                        }
                        if ui.button("Cancel").clicked() { self.security_confirm_clear_quarantine = false; }
                    });
                });
        }
    }

    fn security_tool_window(&mut self, ctx: &egui::Context, tool: SecurityTool) {
        let index = tool.index();
        if !self.security_tool_open[index] { return; }
        self.load_security_detection_if_needed();

        let info = tool.info();
        let detected = self.security_tool_paths[index].clone();
        let mut open = self.security_tool_open[index];
        let mut launch_requested = false;
        let mut requested_url: Option<&'static str> = None;
        let mut status = self.security_tool_status[index].clone();
        let scope_ready = security_tools::validate_authorized_scope(&self.security_scope, self.security_scope_confirmed).is_ok();

        egui::Window::new(format!("NOVA · {}", info.name))
            .open(&mut open)
            .fade_in(!self.settings.reduce_motion)
            .fade_out(!self.settings.reduce_motion)
            .default_size([590.0, 410.0])
            .show(ctx, |ui| {
                ui.heading(info.name);
                ui.label(RichText::new(info.description).weak());
                ui.separator();
                ui.label(RichText::new("AUTHORIZED SCOPE").small().strong().weak());
                ui.label(if self.security_scope.trim().is_empty() { "No target scope configured in Security Center.".to_owned() } else { self.security_scope.clone() });
                if !scope_ready {
                    ui.colored_label(Color32::from_rgb(225, 166, 86), "Return to Security Center, enter the authorized scope and confirm permission before launching this tool.");
                }

                if tool == SecurityTool::VulnerabilityManagement {
                    ui.separator();
                    ui.label("Greenbone is a separate scanner/server. NOVA opens its Greenbone Security Assistant (GSA) web console; it does not install the scanner/database/feed on this USB.");
                    ui.add(TextEdit::singleline(&mut self.greenbone_url).hint_text("https://greenbone.example.test:9392").desired_width(f32::INFINITY));
                    ui.colored_label(Color32::from_rgb(225, 166, 86), "The default-browser action runs on the Windows host and is not sandboxed. Do not use it for an untrusted GSA server.");
                    if ui.add_enabled(scope_ready, egui::Button::new("Open Greenbone console in default browser…")).clicked() { launch_requested = true; }
                } else {
                    ui.separator();
                    if let Some(path) = &detected {
                        ui.colored_label(Color32::from_rgb(87, 190, 140), "Application detected");
                        ui.label(RichText::new(path.display().to_string()).small().weak());
                    } else {
                        ui.colored_label(Color32::from_rgb(225, 166, 86), "Application not detected on this PC or NOVA USB.");
                        ui.label("Install the official application first. NOVA does not download or silently install third-party scanner binaries.");
                    }
                    if ui.add_enabled(scope_ready && detected.is_some(), egui::Button::new(format!("Launch {}", info.executable))).clicked() {
                        launch_requested = true;
                    }
                }

                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("Official download / project page").clicked() { requested_url = Some(info.website); }
                    if ui.button("Documentation").clicked() { requested_url = Some(info.documentation); }
                });
                ui.label(RichText::new(&status).small().weak());
                ui.separator();
                ui.label(RichText::new("Safety: these third-party apps run on the Windows host, not inside Windows Sandbox. The scope acknowledgement is a reminder, not an enforced network boundary. Use only approved assets; start with passive or non-invasive features and use active testing only with explicit authorization.").small().weak());
            });

        self.security_tool_open[index] = open;
        if launch_requested {
            if let Err(error) = security_tools::validate_authorized_scope(&self.security_scope, self.security_scope_confirmed) {
                status = error.to_string();
            } else if tool == SecurityTool::VulnerabilityManagement {
                match security_tools::open_web_console(&self.greenbone_url) {
                    Ok(url) => status = format!("Opened Greenbone console: {url}"),
                    Err(error) => status = format!("Could not open Greenbone console: {error}"),
                }
            } else {
                match tool.launch(&self.paths.app_dir) {
                    Ok(path) => status = format!("Launched {} from {}", info.name, path.display()),
                    Err(error) => status = error.to_string(),
                }
            }
            self.status_message = status.clone();
        }
        if let Some(url) = requested_url {
            match security_tools::open_web_console(url) {
                Ok(url) => status = format!("Opened official project page: {url}"),
                Err(error) => status = format!("Could not open project page: {error}"),
            }
        }
        self.security_tool_status[index] = status;
    }

    fn unikey_window(&mut self, ctx: &egui::Context) {
        if self.unikey_open { self.detect_unikey_if_needed(); }
        let mut download_requested = false;
        let mut start_requested = false;
        let mut stop_requested = false;
        let snap = self.take_window_snap("UniKey", ctx);
        let mut window = egui::Window::new("UniKey — Vietnamese Input")
            .fade_in(!self.settings.reduce_motion)
            .fade_out(!self.settings.reduce_motion)
            .resizable(false)
            .default_width(540.0);
        if let Some((position, size)) = snap { window = window.current_pos(position).fixed_size(size); }
        window.open(&mut self.unikey_open).show(ctx, |ui| {
                ui.heading("Vietnamese typing for NOVA and Windows");
                ui.label("NOVA uses Unicode text fields and system-font fallback for installed glyphs. UniKey adds Vietnamese Telex/VNI input on the Windows host, including NOVA Editor and Terminal.");
                ui.separator();
                if let Some(path) = &self.unikey_path {
                    ui.colored_label(Color32::from_rgb(87, 190, 140), "Portable UniKey executable found on this USB");
                    ui.label(RichText::new(path.display().to_string()).small().weak());
                } else {
                    ui.colored_label(Color32::from_rgb(225, 166, 86), "UniKey is not on this USB yet");
                    ui.label("Download the official 64-bit UniKey build. NOVA verifies the Authenticode signature publisher before launching it.");
                }
                ui.horizontal(|ui| {
                    if ui.add_enabled(self.unikey_child.is_none(), egui::Button::new("Download UniKey to USB")).clicked() {
                        download_requested = true;
                    }
                    if ui.add_enabled(self.unikey_path.is_some() && self.unikey_child.is_none(), egui::Button::new("Start UniKey")).clicked() {
                        start_requested = true;
                    }
                    if self.unikey_process_kind == Some(UniKeyProcessKind::Running)
                        && ui.button("Stop UniKey").clicked()
                    {
                        stop_requested = true;
                    }
                });
                if self.unikey_process_kind == Some(UniKeyProcessKind::Download) {
                    ui.label(RichText::new("Download is running. Wait for signature verification to finish; NOVA keeps the temporary download cleanup handler active.").small().weak());
                }
                ui.label(RichText::new(&self.unikey_status).small().weak());
                ui.separator();
                ui.heading("Recommended UniKey settings");
                ui.label("1. Open the UniKey tray menu.");
                ui.label("2. Character set: Unicode.");
                ui.label("3. Input method: Telex or VNI.");
                ui.label("4. Keep UniKey running while typing. UniKey is a separate Windows-host process, not part of the disposable Sandbox. Closing NOVA stops the UniKey process that NOVA started when it can do so cleanly.");
                ui.label(RichText::new("Official source: unikey.org · Publisher must be PHAM KIM LONG · NOVA verifies Authenticode before launch · No Windows service or startup task is installed.").small().weak());
                if ui.button("Open official UniKey download page").clicked() {
                    match unikey::open_official_download_page() {
                        Ok(()) => self.unikey_status = "Opened the official UniKey download page.".to_owned(),
                        Err(error) => self.unikey_status = format!("Could not open UniKey website: {error}"),
                    }
                }
            });
        // Defer whole-self methods until egui releases the `.open(&mut self.unikey_open)` borrow.
        if download_requested {
            self.download_unikey();
        } else if start_requested {
            self.start_unikey();
        } else if stop_requested {
            self.stop_unikey();
        }
    }

    /// Paint a compact Windows-style desktop/Start shortcut using the shared app catalog.
    fn app_icon_button(&mut self, ui: &mut egui::Ui, app: StartApp, size: egui::Vec2, desktop: bool) -> bool {
        let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
        let hover = if self.settings.reduce_motion {
            if response.hovered() { 1.0 } else { 0.0 }
        } else {
            ui.ctx().animate_bool_with_time(
                ui.scope_id().with(("app-shortcut-hover", app.name)),
                response.hovered(),
                0.12,
            )
        };
        let dark = self.is_dark_mode(ui.ctx());
        let base_alpha = if desktop { 0 } else if dark { 16 } else { 24 };
        let hover_alpha = if desktop { 42 } else { 42 };
        let fill_alpha = (base_alpha as f32 + hover * hover_alpha as f32).round() as u8;
        if fill_alpha > 0 {
            ui.painter().rect_filled(
                rect,
                egui::CornerRadius::same(if desktop { 8 } else { 11 }),
                Color32::from_rgba_unmultiplied(255, 255, 255, fill_alpha),
            );
        }
        if response.hovered() && !desktop {
            ui.painter().rect_stroke(
                rect,
                egui::CornerRadius::same(11),
                egui::Stroke::new(1.0, Color32::from_rgba_unmultiplied(145, 178, 245, 95)),
                egui::StrokeKind::Inside,
            );
        }
        let icon_size: f32 = if desktop { 31.0 } else { 27.0 };
        let text_color = if dark { Color32::from_rgb(238, 243, 252) } else { Color32::from_rgb(35, 45, 63) };
        let icon_color = if dark { Color32::from_rgb(225, 234, 252) } else { Color32::from_rgb(45, 66, 100) };
        let cache_key = (app.icon, icon_size.to_bits(), ui.ctx().pixels_per_point().to_bits(), dark);
        let icon_galley = if let Some(cached) = self.icon_galleys.get(&cache_key) {
            std::sync::Arc::clone(cached)
        } else {
            let galley = ui.painter().layout_no_wrap(
                app.icon.to_owned(),
                FontId::proportional(icon_size),
                icon_color,
            );
            self.icon_galleys.insert(cache_key, std::sync::Arc::clone(&galley));
            galley
        };
        ui.painter().galley(
            egui::pos2(rect.center().x - icon_galley.size().x * 0.5, rect.top() + if desktop { 8.0 } else { 7.0 }),
            icon_galley,
            icon_color,
        );
        ui.painter().text(
            egui::pos2(rect.center().x, rect.bottom() - 7.0),
            egui::Align2::CENTER_BOTTOM,
            app.name,
            FontId::proportional(if desktop { 11.5 } else { 11.0 }),
            text_color,
        );
        let mut open_from_menu = false;
        let mut pins_changed = false;
        response.context_menu(|ui| {
            if ui.button(format!("Open {}", app.name)).clicked() {
                open_from_menu = true;
                ui.close();
            }
            let is_pinned = self.settings.pinned_apps.iter().any(|name| name.as_str() == app.name);
            if is_pinned {
                if ui.button("Unpin from Start").clicked() {
                    self.settings.pinned_apps.retain(|name| name.as_str() != app.name);
                    pins_changed = true;
                    ui.close();
                }
            } else if ui.button("Pin to Start").clicked() {
                self.settings.pinned_apps.push(app.name.to_owned());
                pins_changed = true;
                ui.close();
            }
            ui.separator();
            ui.label(RichText::new(app.description).small().weak());
        });
        if pins_changed {
            self.status_message = match self.paths.save_settings(&self.settings) {
                Ok(()) => format!("Start pins updated: {}", app.name),
                Err(error) => format!("Start pins changed but could not be saved: {error}"),
            };
        }
        response.clicked() || open_from_menu
    }

    fn take_window_snap(&mut self, app: &str, ctx: &egui::Context) -> Option<(egui::Pos2, egui::Vec2)> {
        let side = self.window_snap_requests.remove(app)?;
        let rect = ctx.content_rect();
        let half_width = (rect.width() * 0.5).max(220.0);
        let (x, width) = match side {
            SnapSide::Left => (rect.left(), half_width),
            SnapSide::Right => (rect.right() - half_width, half_width),
            SnapSide::Maximize => (rect.left(), (rect.width() - 8.0).max(220.0)),
        };
        let height = (rect.height() - 20.0).max(220.0);
        Some((egui::pos2(x, rect.top() + 4.0), egui::vec2(width, height)))
    }

    fn taskbar(&mut self, ui: &mut egui::Ui) {
        let dark = self.is_dark_mode(ui.ctx());
        let taskbar_fill = if dark {
            Color32::from_rgba_unmultiplied(20, 27, 43, 238)
        } else {
            Color32::from_rgba_unmultiplied(248, 250, 255, 244)
        };
        egui::Frame::new()
            .fill(taskbar_fill)
            .stroke(egui::Stroke::new(1.0, if dark { Color32::from_white_alpha(24) } else { Color32::from_black_alpha(18) }))
            .corner_radius(egui::CornerRadius::same(14))
            .inner_margin(egui::Margin::symmetric(12, 6))
            .show(ui, |ui| {
                ui.set_min_height(48.0);
                ui.horizontal_centered(|ui| {
                    if ui.add_sized(
                        [38.0, 38.0],
                        egui::Button::new(RichText::new("⊞").size(27.0).color(Color32::from_rgb(105, 157, 245)))
                            .corner_radius(egui::CornerRadius::same(10)),
                    ).on_hover_text("Start").clicked() {
                        self.start_open = !self.start_open;
                        self.start_show_all = false;
                    }

                    let search_width = ui.available_width().clamp(78.0, 132.0);
                    if ui.add_sized(
                        [search_width, 34.0],
                        egui::Button::new(RichText::new("⌕   Search").size(13.0))
                            .corner_radius(egui::CornerRadius::same(18)),
                    ).clicked() {
                        self.start_open = true;
                        self.start_show_all = false;
                    }
                    ui.add_space(5.0);
                    ui.separator();
                    ui.add_space(3.0);

                    let task_apps = [
                        ("▦", "Files", self.files_open),
                        ("⌘", "Terminal", self.terminal_open),
                        ("✎", "Editor", self.editor_open),
                        ("∑", "Calculator", self.calculator_open),
                        ("◉", "Monitor", self.monitor_open),
                        ("▤", "Trash", self.trash_open),
                        ("🛡", "Security Center", self.security_open),
                        ("◎", "Network Discovery", self.security_tool_open[SecurityTool::NetworkDiscovery.index()]),
                        ("◈", "Vulnerability Management", self.security_tool_open[SecurityTool::VulnerabilityManagement.index()]),
                        ("⌁", "Web Security", self.security_tool_open[SecurityTool::WebSecurity.index()]),
                        ("▥", "Packet Analyzer", self.security_tool_open[SecurityTool::PacketAnalyzer.index()]),
                        ("⇄", "Web / API Inspector", self.security_tool_open[SecurityTool::WebApiInspector.index()]),
                        ("🌐", "Google Chrome", self.chrome_open),
                        ("文", "UniKey", self.unikey_open),
                        ("⚙", "Settings", self.settings_open),
                    ];
                    let app_strip_width = (ui.available_width() - 108.0).max(112.0);
                    let task_icon_width = if app_strip_width < 420.0 { 30.0 } else { 34.0 };
                    egui::ScrollArea::horizontal()
                        .id_salt("nova-taskbar-running-apps")
                        .max_width(app_strip_width)
                        .max_height(42.0)
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                for (glyph, name, active) in task_apps {
                                    let active_fill = if dark { Color32::from_rgb(47, 61, 88) } else { Color32::from_rgb(224, 234, 250) };
                                    let button = egui::Button::new(RichText::new(glyph).size(if task_icon_width < 34.0 { 17.0 } else { 20.0 }))
                                        .min_size(egui::vec2(task_icon_width, 36.0))
                                        .corner_radius(egui::CornerRadius::same(9))
                                        .fill(if active { active_fill } else { Color32::TRANSPARENT });
                                    let response = ui.add(button).on_hover_text(name);
                                    if response.clicked() { self.toggle_app(name); }
                                    response.context_menu(|ui| {
                                        ui.label(RichText::new(format!("Arrange {name}")).small().weak());
                                        if ui.button("Snap to left half").clicked() {
                                            self.open_app(name);
                                            self.window_snap_requests.insert(name.to_owned(), SnapSide::Left);
                                            ui.close();
                                        }
                                        if ui.button("Snap to right half").clicked() {
                                            self.open_app(name);
                                            self.window_snap_requests.insert(name.to_owned(), SnapSide::Right);
                                            ui.close();
                                        }
                                        if ui.button("Fill desktop").clicked() {
                                            self.open_app(name);
                                            self.window_snap_requests.insert(name.to_owned(), SnapSide::Maximize);
                                            ui.close();
                                        }
                                    });
                                }
                            });
                        });

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add_sized([34.0, 36.0], egui::Button::new("☰").corner_radius(egui::CornerRadius::same(9)))
                            .on_hover_text("Quick Settings and recent NOVA activity").clicked() {
                            self.notification_open = !self.notification_open;
                        }
                        if ui.add_sized([34.0, 36.0], egui::Button::new("◉").corner_radius(egui::CornerRadius::same(9)))
                            .on_hover_text("System Monitor").clicked() {
                            self.monitor_open = true;
                        }
                        let mode = if self.paths.portable { "USB" } else { "LOCAL" };
                        ui.label(RichText::new(format!("● {mode}")).small().color(Color32::from_rgb(105, 190, 158)));
                    });
                });
            });
    }

    fn notification_center(&mut self, ctx: &egui::Context) {
        if !self.notification_open { return; }
        let dark = self.is_dark_mode(ctx);
        let fill = if dark { Color32::from_rgb(25, 32, 49) } else { Color32::from_rgb(249, 251, 255) };
        egui::Area::new(egui::Id::unique("nova-quick-settings-panel"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-12.0, -66.0))
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).fill(fill).corner_radius(egui::CornerRadius::same(16))
                    .inner_margin(egui::Margin::same(16)).show(ui, |ui| {
                        ui.set_min_width(270.0);
                        ui.set_max_width(320.0);
                        ui.horizontal(|ui| {
                            ui.heading("Quick settings");
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.small_button("✕").clicked() { self.notification_open = false; }
                            });
                        });
                        ui.label(RichText::new(if self.paths.portable { "USB portable workspace" } else { "Local workspace" }).small().weak());
                        ui.separator();
                        ui.horizontal(|ui| {
                            if ui.button("⚙ Settings").clicked() { self.settings_open = true; self.notification_open = false; }
                            if ui.button("◉ Monitor").clicked() { self.monitor_open = true; self.notification_open = false; }
                        });
                        ui.horizontal(|ui| {
                            if ui.button("▦ Files").clicked() { self.files_open = true; self.ensure_files_loaded(); self.notification_open = false; }
                            if ui.button("⌘ Terminal").clicked() { self.terminal_open = true; self.notification_open = false; }
                        });
                        ui.separator();
                        ui.label(RichText::new("Recent activity").strong());
                        ui.label(RichText::new(&self.status_message).small().weak());
                        ui.add_space(5.0);
                        ui.label(RichText::new("Shortcuts: Ctrl+Space · Ctrl+E · Ctrl+Shift+Esc").small().weak());
                    });
            });
    }

    fn start_menu(&mut self, ctx: &egui::Context) {
        if self.start_open && ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
            self.start_open = false;
        }
        let visibility = if self.settings.reduce_motion {
            if self.start_open { 1.0 } else { 0.0 }
        } else {
            ctx.animate_bool_with_time(egui::Id::unique("nova-windows-start-menu"), self.start_open, 0.20)
        };
        if visibility <= 0.001 {
            return;
        }

        let mut selected_app: Option<&'static str> = None;
        let mut show_pinned = false;
        let mut show_all = false;
        let content_rect = ctx.content_rect();
        let screen_width = content_rect.width();
        let menu_height = (content_rect.height() - 190.0).clamp(160.0, 390.0);
        let menu_width = (screen_width - 20.0).clamp(300.0, 680.0);
        let pinned_columns = (((menu_width - 40.0) / 91.0).floor() as usize).clamp(2, 6);
        let slide_up = if self.settings.reduce_motion { 0.0 } else { (1.0 - visibility) * 16.0 };
        let dark = self.is_dark_mode(ctx);
        let surface = if dark {
            Color32::from_rgba_unmultiplied(25, 32, 49, 246)
        } else {
            Color32::from_rgba_unmultiplied(249, 251, 255, 248)
        };
        let border = if dark { Color32::from_white_alpha(30) } else { Color32::from_black_alpha(22) };

        egui::Area::new(egui::Id::unique("nova-start-menu-panel"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -66.0 + slide_up))
            .fade_in(!self.settings.reduce_motion)
            .show(ctx, |ui| {
                ui.set_opacity(visibility);
                egui::Frame::new()
                    .fill(surface)
                    .stroke(egui::Stroke::new(1.0, border))
                    .shadow(egui::Shadow {
                        offset: [0, 14],
                        blur: 30,
                        spread: 0,
                        color: Color32::from_black_alpha(90),
                    })
                    .corner_radius(egui::CornerRadius::same(18))
                    .inner_margin(egui::Margin::same(19))
                    .show(ui, |ui| {
                        ui.set_min_width(menu_width - 38.0);
                        ui.set_max_width(menu_width - 38.0);

                        ui.add(
                            TextEdit::singleline(&mut self.start_search)
                                .hint_text("Search apps and settings")
                                .desired_width(f32::INFINITY),
                        );
                        let query = self.start_search.trim().to_lowercase();
                        ui.add_space(13.0);

                        if self.start_show_all {
                            ui.horizontal(|ui| {
                                ui.heading(RichText::new("All apps").size(19.0).strong());
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if ui.button("← Pinned").clicked() { show_pinned = true; }
                                });
                            });
                            ui.add_space(7.0);
                            egui::ScrollArea::vertical().max_height(menu_height).show(ui, |ui| {
                                let categories = ["Workspace", "Productivity", "Internet", "System", "Security"];
                                for category in categories {
                                    let items: Vec<StartApp> = START_APPS.iter().copied()
                                        .filter(|app| app.category == category)
                                        .filter(|app| query.is_empty() || app.name.to_lowercase().contains(&query)
                                            || app.description.to_lowercase().contains(&query)
                                            || app.category.to_lowercase().contains(&query))
                                        .collect();
                                    if items.is_empty() { continue; }
                                    ui.label(RichText::new(category.to_uppercase()).small().strong().weak());
                                    ui.add_space(4.0);
                                    egui::Grid::new(("nova-start-category", category))
                                        .num_columns(2)
                                        .spacing(egui::vec2(8.0, 5.0))
                                        .show(ui, |ui| {
                                            for (index, app) in items.iter().enumerate() {
                                                let label = format!("{}   {}\n       {}", app.icon, app.name, app.description);
                                                if ui.add_sized([menu_width * 0.5 - 39.0, 47.0], egui::Button::new(RichText::new(label).size(12.0)).corner_radius(egui::CornerRadius::same(9))).clicked() {
                                                    selected_app = Some(app.name);
                                                }
                                                if index % 2 == 1 { ui.end_row(); }
                                            }
                                        });
                                    ui.add_space(9.0);
                                }
                                if !START_APPS.iter().any(|app| query.is_empty() || app.name.to_lowercase().contains(&query)
                                    || app.description.to_lowercase().contains(&query)
                                    || app.category.to_lowercase().contains(&query)) {
                                    ui.label(RichText::new("No applications match your search.").weak());
                                }
                            });
                        } else {
                            ui.horizontal(|ui| {
                                ui.heading(RichText::new(if query.is_empty() { "Pinned" } else { "Search results" }).size(19.0).strong());
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if ui.button("All apps  →").clicked() { show_all = true; }
                                });
                            });
                            ui.add_space(7.0);
                            let matching: Vec<StartApp> = if query.is_empty() {
                                self.settings.pinned_apps.iter().filter_map(|name| START_APPS.iter().find(|app| app.name == name.as_str()).copied()).collect()
                            } else {
                                START_APPS.iter().copied().filter(|app| app.name.to_lowercase().contains(&query)
                                    || app.description.to_lowercase().contains(&query)
                                    || app.category.to_lowercase().contains(&query)).collect()
                            };
                            egui::Grid::new("nova-start-pinned-grid")
                                .num_columns(pinned_columns)
                                .spacing(egui::vec2(5.0, 5.0))
                                .show(ui, |ui| {
                                    for (index, app) in matching.iter().enumerate() {
                                        if self.app_icon_button(ui, *app, egui::vec2(91.0, 76.0), false) {
                                            selected_app = Some(app.name);
                                        }
                                        if index % pinned_columns == pinned_columns - 1 { ui.end_row(); }
                                    }
                                });
                            if matching.is_empty() {
                                ui.label(RichText::new(if query.is_empty() { "No pinned apps yet. Right-click a desktop shortcut and choose Pin to Start." } else { "No applications match your search." }).weak());
                            }

                            if query.is_empty() {
                                ui.add_space(11.0);
                                ui.separator();
                                ui.add_space(7.0);
                                ui.horizontal(|ui| {
                                    ui.heading(RichText::new("Recommended").size(17.0).strong());
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        ui.label(RichText::new("Quick access").small().weak());
                                    });
                                });
                                ui.add_space(5.0);
                                let quick = ["Files", "Terminal", "Settings"];
                                ui.horizontal(|ui| {
                                    for name in quick {
                                        if let Some(app) = START_APPS.iter().find(|app| app.name == name).copied() {
                                            if ui.add_sized([menu_width / 3.0 - 34.0, 43.0], egui::Button::new(format!("{}  {}\n      {}", app.icon, app.name, app.description)).corner_radius(egui::CornerRadius::same(9))).clicked() {
                                                selected_app = Some(app.name);
                                            }
                                        }
                                    }
                                });
                            }
                        }

                        ui.add_space(9.0);
                        ui.separator();
                        ui.add_space(5.0);
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("◉  NOVA User").size(12.0).strong());
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.label(RichText::new(if self.paths.portable { "Portable workspace" } else { "Local workspace" }).small().weak());
                            });
                        });
                    });
            });

        if show_all { self.start_show_all = true; }
        if show_pinned { self.start_show_all = false; }
        if let Some(app) = selected_app { self.open_app(app); }
    }

    fn start_terminal_task(&mut self, task: TerminalTask) {
        if self.terminal_busy {
            self.terminal.line("A terminal command is already running. Please wait for it to finish.");
            return;
        }

        let (sender, receiver) = mpsc::channel();
        match thread::Builder::new()
            .name("nova-terminal-worker".to_owned())
            .spawn(move || {
                let result = task.run();
                let _ = sender.send(result);
            })
        {
            Ok(_) => {
                self.terminal_job_receiver = Some(receiver);
                self.terminal_busy = true;
            }
            Err(error) => self.terminal.line(format!("Could not start terminal worker: {error}")),
        }
    }

    fn poll_terminal_task(&mut self, ctx: &egui::Context) {
        let result = match self.terminal_job_receiver.as_ref() {
            Some(receiver) => match receiver.try_recv() {
                Ok(result) => Some(Ok(result)),
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => Some(Err("Terminal worker stopped unexpectedly.".to_owned())),
            },
            None => None,
        };

        if let Some(result) = result {
            match result {
                Ok(output) if !output.trim().is_empty() => {
                    for line in output.lines() {
                        self.terminal.line(line.to_owned());
                    }
                }
                Ok(_) => {}
                Err(error) => self.terminal.line(error),
            }
            self.terminal_job_receiver = None;
            self.terminal_busy = false;
        } else if self.terminal_busy {
            // Worker polling is intentionally throttled; input events still repaint immediately.
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }

    fn terminal_window(&mut self, ctx: &egui::Context) {
        self.poll_terminal_task(ctx);
        let mut submit = false;
        let mut clear_output = false;
        let snap = self.take_window_snap("Terminal", ctx);
        let mut window = egui::Window::new("NOVA Terminal — Windows CMD")
            .fade_in(!self.settings.reduce_motion)
            .fade_out(!self.settings.reduce_motion)
            .default_size([760.0, 490.0])
            .min_width(420.0);
        if let Some((position, size)) = snap { window = window.current_pos(position).fixed_size(size); }
        window.open(&mut self.terminal_open).show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(self.terminal.prompt()).monospace().color(Color32::from_rgb(117, 203, 172)));
                    if self.terminal.wifi_mode.is_some() {
                        ui.label(RichText::new("Wi-Fi setup mode · input hidden").small().weak());
                    }
                    if self.terminal_busy {
                        ui.spinner();
                        ui.label(RichText::new("Running in background…").small().weak());
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("Clear output").clicked() { clear_output = true; }
                    });
                });
                egui::ScrollArea::vertical()
                    .stick_to_bottom(true)
                    .max_height(ui.available_height() - 82.0)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for line in &self.terminal.output {
                            ui.label(RichText::new(line).font(FontId::monospace(13.0)));
                        }
                    });
                ui.separator();
                ui.horizontal(|ui| {
                    let prompt = self.terminal.prompt();
                    ui.label(RichText::new(prompt).monospace().color(Color32::from_rgb(117, 203, 172)));
                    let hide_input = self.terminal.password_entry();
                    let response = ui.add_enabled(
                        !self.terminal_busy,
                        TextEdit::singleline(&mut self.terminal.input)
                            .font(FontId::monospace(13.0))
                            .password(hide_input)
                            .hint_text(if self.terminal_busy { "Command running…" } else { "Enter command…" })
                            .desired_width(f32::INFINITY),
                    );
                    if !self.terminal_busy && response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)) {
                        submit = true;
                    }
                    if ui.add_enabled(!self.terminal_busy, egui::Button::new("Run")).clicked() {
                        submit = true;
                    }
                });
                ui.label(RichText::new("CMD runs on the Windows host with your account permissions. It is NOT isolated; never run unknown commands here.").small().weak());
            });
        if clear_output { self.terminal.clear(); }
        if submit && !self.terminal_busy {
            let line = std::mem::take(&mut self.terminal.input);
            if !line.trim().is_empty() {
                if let Some(task) = self.terminal.submit(&self.paths, &line) {
                    self.start_terminal_task(task);
                }
            }
        }
    }

    fn files_window(&mut self, ctx: &egui::Context) {
        let mut navigate_to: Option<PathBuf> = None;
        let mut open_file: Option<PathBuf> = None;
        let mut create_folder = false;
        let mut move_to_trash: Option<PathBuf> = None;
        let mut sandbox_sample: Option<PathBuf> = None;
        let mut refresh = false;
        let snap = self.take_window_snap("Files", ctx);
        let mut window = egui::Window::new("▦  NOVA Files")
            .fade_in(!self.settings.reduce_motion)
            .fade_out(!self.settings.reduce_motion)
            .default_size([760.0, 520.0])
            .min_width(420.0);
        if let Some((position, size)) = snap { window = window.current_pos(position).fixed_size(size); }
        window.open(&mut self.files_open).show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if ui.button("↑  Up").clicked() && self.files_dir != self.paths.user_dir {
                        navigate_to = self.files_dir.parent().map(Path::to_path_buf);
                    }
                    if ui.button("↻  Refresh").clicked() { refresh = true; }
                    ui.separator();
                    let relative = self.files_dir.strip_prefix(&self.paths.user_dir).unwrap_or(Path::new("."));
                    ui.label(RichText::new(format!("Home  /  {}", relative.display())).weak());
                });
                ui.separator();
                ui.horizontal(|ui| {
                    ui.add(TextEdit::singleline(&mut self.files_new_folder).hint_text("New folder name").desired_width(170.0));
                    if ui.button("＋ New folder").clicked() { create_folder = true; }
                    ui.separator();
                    ui.add(TextEdit::singleline(&mut self.files_search).hint_text("Filter this folder…").desired_width(150.0));
                    ui.selectable_value(&mut self.files_grid_view, false, "☷ List");
                    ui.selectable_value(&mut self.files_grid_view, true, "▦ Grid");
                });
                if self.files_selected.is_some() {
                    ui.horizontal(|ui| {
                        if ui.button("Open selected").clicked() {
                            if let Some(selected) = self.files_selected.as_ref() {
                                if selected.is_dir() { navigate_to = Some(selected.clone()); }
                                else { open_file = Some(selected.clone()); }
                            }
                        }
                        if ui.button("Move to Trash").clicked() { move_to_trash = self.files_selected.clone(); }
                        if self.files_selected.as_ref().is_some_and(|path| path.is_file()) && ui.button("Inspect in Sandbox").clicked() {
                            sandbox_sample = self.files_selected.clone();
                        }
                    });
                }
                ui.separator();
                let filter = self.files_search.trim().to_lowercase();
                let visible_entries: Vec<&CachedFileEntry> = self.files_entries.iter()
                    .filter(|entry| filter.is_empty() || entry.name.to_lowercase().contains(&filter))
                    .collect();
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    if let Some(error) = &self.files_error {
                        ui.colored_label(Color32::LIGHT_RED, error);
                    } else if visible_entries.is_empty() {
                        ui.add_space(18.0);
                        ui.heading(if filter.is_empty() { "This folder is empty" } else { "No matching items" });
                        ui.label(RichText::new(if filter.is_empty() { "Create a folder or copy files into your NOVA workspace." } else { "Try a different search term." }).weak());
                    } else if self.files_grid_view {
                        let columns = ((ui.available_width() / 150.0).floor() as usize).clamp(1, 5);
                        egui::Grid::new("nova-files-grid-view").num_columns(columns).spacing(egui::vec2(8.0, 8.0)).show(ui, |ui| {
                            for (index, entry) in visible_entries.iter().enumerate() {
                                let path = entry.path.clone();
                                let selected = self.files_selected.as_ref() == Some(&path);
                                let label = format!("{}\n{}\n{}", if entry.is_dir { "▰" } else { "▤" }, entry.name, entry.display_size);
                                let response = ui.add_sized([136.0, 82.0], egui::Button::selectable(selected, label).corner_radius(egui::CornerRadius::same(10)));
                                if response.clicked() { self.files_selected = Some(path.clone()); }
                                if response.double_clicked() {
                                    if entry.is_dir { navigate_to = Some(path.clone()); } else { open_file = Some(path.clone()); }
                                }
                                response.context_menu(|ui| {
                                    if ui.button("Open").clicked() {
                                        if entry.is_dir { navigate_to = Some(path.clone()); } else { open_file = Some(path.clone()); }
                                        ui.close();
                                    }
                                    if ui.button("Move to Trash").clicked() { move_to_trash = Some(path.clone()); ui.close(); }
                                });
                                if index % columns == columns - 1 { ui.end_row(); }
                            }
                        });
                    } else {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("NAME").small().strong().weak());
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| { ui.label(RichText::new("SIZE").small().strong().weak()); });
                        });
                        for entry in &visible_entries {
                            let path = entry.path.clone();
                            let selected = self.files_selected.as_ref() == Some(&path);
                            ui.horizontal(|ui| {
                                let response = ui.selectable_label(selected, format!("{}  {}", if entry.is_dir { "▰" } else { "▤" }, entry.name));
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    ui.label(RichText::new(&entry.display_size).small().weak());
                                });
                                if response.clicked() { self.files_selected = Some(path.clone()); }
                                if response.double_clicked() {
                                    if entry.is_dir { navigate_to = Some(path.clone()); } else { open_file = Some(path.clone()); }
                                }
                                response.context_menu(|ui| {
                                    if ui.button("Open").clicked() {
                                        if entry.is_dir { navigate_to = Some(path.clone()); } else { open_file = Some(path.clone()); }
                                        ui.close();
                                    }
                                    if ui.button("Move to Trash").clicked() { move_to_trash = Some(path.clone()); ui.close(); }
                                });
                            });
                        }
                    }
                });
                ui.separator();
                ui.label(RichText::new("Single-click to select · Double-click to open · Deleted items stay in NOVA Trash until permanently removed").small().weak());
            });

        if refresh {
            self.files_selected = None;
            self.refresh_files();
        }
        if let Some(path) = navigate_to {
            match canonical_user_path(&self.paths.user_dir, &path) {
                Ok(folder) if folder.is_dir() => {
                    self.files_dir = folder;
                    self.files_selected = None;
                    self.refresh_files();
                }
                Ok(_) => self.status_message = "That item is not a folder.".to_owned(),
                Err(error) => self.status_message = format!("Navigation blocked: {error}"),
            }
        }
        if create_folder {
            let name = self.files_new_folder.clone();
            if !name.is_empty() && !name.contains('/') && !name.contains('\\') {
                let validated_name = safe_user_path(&self.paths.user_dir, &name);
                let safe_parent = canonical_user_path(&self.paths.user_dir, &self.files_dir);
                match (validated_name, safe_parent) {
                    (Ok(_), Ok(parent)) => match fs::create_dir(parent.join(&name)) {
                        Ok(()) => {
                            self.files_new_folder.clear();
                            self.status_message = format!("Created folder: {name}");
                            self.refresh_files();
                        }
                        Err(error) => self.status_message = format!("Create folder failed: {error}"),
                    },
                    (Err(error), _) | (_, Err(error)) => self.status_message = format!("Create folder blocked: {error}"),
                }
            }
        }
        if let Some(path) = move_to_trash {
            match trash::move_to_trash(&self.paths, &path) {
                Ok(entry) => {
                    self.status_message = format!("Moved '{}' to NOVA Trash", entry.name);
                    self.files_selected = None;
                    self.refresh_files();
                    self.refresh_trash();
                }
                Err(error) => self.status_message = format!("Could not move item to Trash: {error}"),
            }
        }
        if let Some(path) = sandbox_sample {
            match isolation::launch_sample_in_sandbox(&self.paths, &path) {
                Ok(_) => {
                    self.quarantine_count = isolation::quarantine_item_count(&self.paths).unwrap_or(self.quarantine_count + 1);
                    self.quarantine_count_loaded = true;
                    self.status_message = "Opened a read-only copy in offline Windows Sandbox. Close Sandbox to discard the test session.".to_owned();
                }
                Err(error) => self.status_message = format!("Could not open file in Windows Sandbox: {error}"),
            }
        }
        if let Some(path) = open_file { self.open_file_in_editor(path); }
    }

    fn open_file_in_editor(&mut self, path: PathBuf) {
        let canonical_file = match canonical_user_path(&self.paths.user_dir, &path) {
            Ok(file) => file,
            Err(error) => {
                self.editor_status = format!("Cannot open file: {error}");
                self.editor_open = true;
                return;
            }
        };
        match fs::metadata(&canonical_file) {
            Ok(meta) if meta.len() <= 2 * 1024 * 1024 => match fs::read(&canonical_file).and_then(|bytes| unicode::decode_text_file(&bytes)) {
                Ok(text) => {
                    self.editor_text = text;
                    let canonical_root = self.paths.user_dir.canonicalize().ok();
                    self.editor_path = canonical_root
                        .as_deref()
                        .and_then(|root| canonical_file.strip_prefix(root).ok())
                        .unwrap_or(&canonical_file)
                        .to_string_lossy()
                        .to_string();
                    self.editor_status = "File opened".to_owned();
                }
                Err(error) => self.editor_status = format!("Text file could not be decoded: {error}"),
            },
            Ok(_) => self.editor_status = "Files larger than 2 MiB are not opened in the built-in editor.".to_owned(),
            Err(error) => self.editor_status = format!("Cannot read file metadata: {error}"),
        }
        self.editor_open = true;
    }

    fn editor_window(&mut self, ctx: &egui::Context) {
        let mut save = false;
        let mut new_document = false;
        let snap = self.take_window_snap("Editor", ctx);
        let mut window = egui::Window::new("NOVA Text Editor")
            .fade_in(!self.settings.reduce_motion)
            .fade_out(!self.settings.reduce_motion)
            .default_size([700.0, 520.0]);
        if let Some((position, size)) = snap { window = window.current_pos(position).fixed_size(size); }
        window.open(&mut self.editor_open).show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Path (inside NOVA user folder):");
                    ui.add(TextEdit::singleline(&mut self.editor_path).desired_width(330.0));
                    if ui.button("New").clicked() { new_document = true; }
                    if ui.button("Save").clicked() { save = true; }
                });
                ui.add(
                    TextEdit::multiline(&mut self.editor_text)
                        .font(FontId::monospace(13.0))
                        .desired_rows(20)
                        .desired_width(f32::INFINITY),
                );
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&self.editor_status).small().weak());
                    ui.label(RichText::new("Unicode: UTF-8 / UTF-16 files · Vietnamese input via UniKey (Unicode mode)").small().weak());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new(format!("{} chars · {} words", self.editor_text.chars().count(), self.editor_text.split_whitespace().count())).small().weak());
                    });
                });
            });
        if new_document {
            self.editor_path = "Documents/Untitled.txt".to_owned();
            self.editor_text.clear();
            self.editor_status = "New document — not saved".to_owned();
        }
        if save {
            if self.editor_text.len() > 2 * 1024 * 1024 {
                self.editor_status = "Files larger than 2 MiB cannot be saved in the built-in editor.".to_owned();
                return;
            }
            match safe_user_path(&self.paths.user_dir, &self.editor_path) {
                Ok(path) => {
                    let parent = path
                        .parent()
                        .filter(|folder| !folder.as_os_str().is_empty())
                        .unwrap_or(&self.paths.user_dir);
                    let canonical_parent = match create_user_directories(&self.paths.user_dir, parent) {
                        Ok(folder) => folder,
                        Err(error) => {
                            self.editor_status = format!("Cannot create or resolve target folder: {error}");
                            return;
                        }
                    };
                    let Some(file_name) = path.file_name() else {
                        self.editor_status = "A file name is required.".to_owned();
                        return;
                    };
                    let safe_target = canonical_parent.join(file_name);
                    match fs::symlink_metadata(&safe_target) {
                        Ok(metadata) if metadata.file_type().is_symlink() => {
                            self.editor_status = "Saving through a symbolic link is blocked.".to_owned();
                            return;
                        }
                        Ok(metadata) if metadata.is_dir() => {
                            self.editor_status = "The target path is a directory, not a file.".to_owned();
                            return;
                        }
                        Ok(_) => {
                            if let Err(error) = canonical_user_path(&self.paths.user_dir, &safe_target) {
                                self.editor_status = format!("Saving outside NOVA data is blocked: {error}");
                                return;
                            }
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                        Err(error) => {
                            self.editor_status = format!("Cannot inspect target file: {error}");
                            return;
                        }
                    }
                    match fs::write(&safe_target, &self.editor_text) {
                        Ok(()) => self.editor_status = "Saved to NOVA USB data".to_owned(),
                        Err(error) => self.editor_status = format!("Save failed: {error}"),
                    }
                }
                Err(error) => self.editor_status = error.to_string(),
            }
        }
    }

    fn settings_window(&mut self, ctx: &egui::Context) {
        let mut changed = false;
        let mut launch_app: Option<&'static str> = None;
        let mut open_files_requested = false;
        let snap = self.take_window_snap("Settings", ctx);
        let mut window = egui::Window::new("⚙  NOVA Settings")
            .fade_in(!self.settings.reduce_motion)
            .fade_out(!self.settings.reduce_motion)
            .default_size([620.0, 460.0])
            .min_width(480.0);
        if let Some((position, size)) = snap { window = window.current_pos(position).fixed_size(size); }
        window.open(&mut self.settings_open).show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.heading("Settings");
                        ui.label(RichText::new("Personalize your NOVA workspace").small().weak());
                    });
                });
                ui.separator();
                egui::ScrollArea::horizontal().show(ui, |ui| {
                    ui.horizontal(|ui| {
                        for (index, label) in [
                            (0, "Appearance"), (1, "System"), (2, "Apps"),
                            (3, "Network"), (4, "Security"), (5, "About"),
                        ] {
                            ui.selectable_value(&mut self.settings_page, index, label);
                        }
                    });
                });
                ui.separator();
                match self.settings_page {
                    0 => {
                        ui.columns(2, |columns| {
                            columns[0].heading("Theme");
                            columns[0].horizontal(|ui| {
                                changed |= ui.selectable_value(&mut self.settings.theme, "system".to_owned(), "System").changed();
                                changed |= ui.selectable_value(&mut self.settings.theme, "dark".to_owned(), "Dark").changed();
                                changed |= ui.selectable_value(&mut self.settings.theme, "light".to_owned(), "Light").changed();
                            });
                            columns[0].add_space(10.0);
                            columns[0].heading("Accent color");
                            egui::ComboBox::from_id_salt("nova-accent")
                                .selected_text(self.settings.accent.to_uppercase())
                                .show_ui(&mut columns[0], |ui| {
                                    for accent in ["blue", "violet", "teal", "rose", "amber"] {
                                        changed |= ui.selectable_value(&mut self.settings.accent, accent.to_owned(), accent.to_uppercase()).changed();
                                    }
                                });
                            columns[1].heading("Preview");
                            columns[1].group(|ui| {
                                ui.label(RichText::new("NOVA OS").size(20.0).strong());
                                ui.label("Portable desktop workspace");
                                ui.add_space(8.0);
                                ui.horizontal(|ui| {
                                    ui.label("●  Accent");
                                    ui.label(RichText::new("Active").color(match self.settings.accent.as_str() {
                                        "violet" => Color32::from_rgb(151, 120, 255),
                                        "teal" => Color32::from_rgb(50, 190, 174),
                                        "rose" => Color32::from_rgb(238, 112, 151),
                                        "amber" => Color32::from_rgb(232, 171, 76),
                                        _ => Color32::from_rgb(92, 139, 245),
                                    }));
                                });
                            });
                        });
                        ui.separator();
                        changed |= ui.checkbox(&mut self.settings.reduce_motion, "Reduce animations and visual effects").changed();
                    }
                    1 => {
                        ui.heading("System & accessibility");
                        ui.label("Choose how NOVA behaves on this Windows computer.");
                        changed |= ui.checkbox(&mut self.settings.reduce_motion, "Reduce animations and visual effects").changed();
                        ui.separator();
                        ui.horizontal(|ui| {
                            if ui.button("Open System Monitor").clicked() { self.monitor_open = true; }
                            if ui.button("Open File Manager").clicked() { open_files_requested = true; }
                        });
                        ui.horizontal(|ui| {
                            if ui.button("Open UniKey input").clicked() { self.unikey_open = true; }
                            if ui.button("Open Security Center").clicked() { self.security_open = true; }
                        });
                    }
                    2 => {
                        ui.heading("Installed NOVA workspaces");
                        ui.label(RichText::new("Choose an app to open. Optional host applications are detected only when needed.").small().weak());
                        egui::ScrollArea::vertical().max_height(285.0).show(ui, |ui| {
                            for app in START_APPS {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(app.icon).size(20.0));
                                    ui.vertical(|ui| {
                                        ui.label(RichText::new(app.name).strong());
                                        ui.label(RichText::new(app.description).small().weak());
                                    });
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        if ui.button("Open").clicked() { launch_app = Some(app.name); }
                                    });
                                });
                                ui.separator();
                            }
                        });
                    }
                    3 => {
                        ui.heading("Network & connectivity");
                        ui.label("NOVA provides tools for inspecting and configuring the Windows host network. It does not replace Windows network settings.");
                        ui.horizontal(|ui| {
                            if ui.button("Open Terminal / Wi-Fi tools").clicked() { self.terminal_open = true; }
                            if ui.button("Network Discovery").clicked() { launch_app = Some("Network Discovery"); }
                        });
                        ui.label(RichText::new("Network Discovery requires its optional external tool. Network commands run with your Windows account permissions.").small().weak());
                    }
                    4 => {
                        ui.heading("Security & isolation");
                        ui.label("Manage optional security workspaces and Windows Sandbox integration.");
                        if ui.button("Open Security Center").clicked() { self.security_open = true; }
                        ui.label(RichText::new("External security tools run on the Windows host unless explicitly opened in Windows Sandbox. Scope acknowledgement is not a technical network boundary. Use only authorized targets.").small().weak());
                    }
                    _ => {
                        ui.heading("NOVA OS Native");
                        ui.label("Portable Windows desktop workspace");
                        ui.label(format!("Mode: {}", if self.paths.portable { "USB portable" } else { "Local" }));
                        ui.label("NOVA is an application running on Windows, not a separate operating-system kernel.");
                        ui.separator();
                        ui.label("Keyboard shortcuts");
                        ui.monospace("Ctrl + Space    Start menu");
                        ui.monospace("Ctrl + E         File Manager");
                        ui.monospace("Ctrl + Shift + Esc    System Monitor");
                        ui.monospace("Esc             Close Start / quick panel");
                    }
                }
                ui.separator();
                ui.label(RichText::new("Settings are stored in the NOVA user data folder. NOVA does not install Windows services or startup tasks.").small().weak());
            });
        if open_files_requested { self.files_open = true; self.ensure_files_loaded(); }
        if let Some(app) = launch_app { self.open_app(app); }
        if changed {
            self.apply_theme(ctx);
            match self.paths.save_settings(&self.settings) {
                Ok(()) => self.status_message = "Settings saved.".to_owned(),
                Err(error) => self.status_message = format!("Settings changed but could not be saved: {error}"),
            }
        }
    }

    fn trash_window(&mut self, ctx: &egui::Context) {
        let mut restore_id: Option<String> = None;
        let mut delete_id: Option<String> = None;
        let mut refresh_requested = false;
        let snap = self.take_window_snap("Trash", ctx);
        let mut window = egui::Window::new("▤  NOVA Trash")
            .fade_in(!self.settings.reduce_motion)
            .fade_out(!self.settings.reduce_motion)
            .default_size([690.0, 450.0]);
        if let Some((position, size)) = snap { window = window.current_pos(position).fixed_size(size); }
        window.open(&mut self.trash_open).show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(format!("{} item(s)", self.trash_entries.len()));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add_enabled(!self.trash_entries.is_empty(), egui::Button::new("Empty Trash…")).clicked() {
                            self.trash_confirm_empty = true;
                        }
                        if let Some(id) = self.trash_selected.as_ref() {
                            if ui.button("Delete permanently…").clicked() { delete_id = Some(id.clone()); }
                            if ui.button("Restore").clicked() { restore_id = Some(id.clone()); }
                        }
                        if ui.button("↻").on_hover_text("Refresh Trash").clicked() { refresh_requested = true; }
                    });
                });
                ui.separator();
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    if self.trash_entries.is_empty() {
                        ui.add_space(22.0);
                        ui.vertical_centered(|ui| {
                            ui.label(RichText::new("▤").size(42.0).color(Color32::from_rgb(130, 155, 205)));
                            ui.heading("Your Trash is empty");
                            ui.label(RichText::new("Items you delete in NOVA Files will appear here.").weak());
                        });
                    } else {
                        for entry in &self.trash_entries {
                            let selected = self.trash_selected.as_ref() == Some(&entry.id);
                            let size = entry.size_bytes.map(format_size).unwrap_or_else(|| "Folder".to_owned());
                            let age = relative_age(entry.deleted_at);
                            let label = format!("{}  {}   ·   {}   ·   {}", if entry.is_directory { "▰" } else { "▤" }, entry.name, size, age);
                            if ui.selectable_label(selected, label).clicked() { self.trash_selected = Some(entry.id.clone()); }
                            if selected {
                                ui.label(RichText::new(format!("Original location: {}", entry.original_path)).small().weak());
                            }
                        }
                    }
                });
                ui.separator();
                ui.label(RichText::new("NOVA Trash is stored in the Data folder on this USB. Emptying Trash permanently deletes these items.").small().weak());
            });

        if refresh_requested { self.refresh_trash(); }
        if let Some(id) = restore_id {
            match trash::restore_from_trash(&self.paths, &id) {
                Ok(path) => { self.status_message = format!("Restored to {}", path.display()); self.refresh_trash(); }
                Err(error) => self.status_message = format!("Restore failed: {error}"),
            }
        }
        if let Some(id) = delete_id {
            self.trash_selected = Some(id);
            self.trash_confirm_delete = true;
        }

        if self.trash_confirm_delete {
            egui::Window::new("Delete permanently?")
                .collapsible(false)
                .resizable(false)
                .fade_in(!self.settings.reduce_motion)
                .show(ctx, |ui| {
                    ui.label("This item will be permanently deleted from NOVA Trash.");
                    ui.horizontal(|ui| {
                        if ui.button("Delete permanently").clicked() {
                            if let Some(id) = self.trash_selected.clone() {
                                match trash::delete_permanently(&self.paths, &id) {
                                    Ok(()) => self.status_message = "Item permanently deleted.".to_owned(),
                                    Err(error) => self.status_message = format!("Permanent delete failed: {error}"),
                                }
                                self.refresh_trash();
                            }
                            self.trash_confirm_delete = false;
                        }
                        if ui.button("Cancel").clicked() { self.trash_confirm_delete = false; }
                    });
                });
        }

        if self.trash_confirm_empty {
            egui::Window::new("Empty NOVA Trash?")
                .collapsible(false)
                .resizable(false)
                .fade_in(!self.settings.reduce_motion)
                .show(ctx, |ui| {
                    ui.label("All items in NOVA Trash will be permanently deleted. This cannot be undone.");
                    ui.horizontal(|ui| {
                        if ui.button("Empty Trash").clicked() {
                            match trash::empty_trash(&self.paths) {
                                Ok(count) => self.status_message = format!("Permanently deleted {count} item(s) from NOVA Trash."),
                                Err(error) => self.status_message = format!("Could not empty Trash: {error}"),
                            }
                            self.refresh_trash();
                            self.trash_confirm_empty = false;
                        }
                        if ui.button("Cancel").clicked() { self.trash_confirm_empty = false; }
                    });
                });
        }
    }

    fn chrome_window(&mut self, ctx: &egui::Context) {
        if self.chrome_open { self.detect_chrome_if_needed(); }
        let mut open_host_now = false;
        let mut open_isolated_now = false;
        let snap = self.take_window_snap("Google Chrome", ctx);
        let mut window = egui::Window::new("🌐  Google Chrome")
            .fade_in(!self.settings.reduce_motion)
            .fade_out(!self.settings.reduce_motion)
            .resizable(false)
            .default_width(540.0);
        if let Some((position, size)) = snap { window = window.current_pos(position).fixed_size(size); }
        window.open(&mut self.chrome_open).show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Open a website in Google Chrome").strong());
                    if ui.small_button("↻ Detect").on_hover_text("Search common Chrome installation paths again").clicked() {
                        self.chrome_path = apps::find_chrome();
                        self.chrome_checked = true;
                    }
                });
                ui.add(TextEdit::singleline(&mut self.chrome_url)
                    .hint_text("https://example.com")
                    .desired_width(f32::INFINITY));
                ui.horizontal(|ui| {
                    if ui.add_enabled(self.chrome_path.is_some(), egui::Button::new("Open isolated (recommended)")).clicked() {
                        open_isolated_now = true;
                    }
                    if ui.add_enabled(self.chrome_path.is_some(), egui::Button::new("Open on Windows host…")).clicked() {
                        self.chrome_confirm_host = true;
                    }
                });
                if self.chrome_confirm_host {
                    ui.separator();
                    ui.colored_label(Color32::from_rgb(225, 166, 86), "This opens normal Chrome on the real Windows host. Downloads, extensions, cookies and browser exploits are NOT isolated by NOVA.");
                    ui.horizontal(|ui| {
                        if ui.button("I understand — open on host").clicked() {
                            self.chrome_confirm_host = false;
                            open_host_now = true;
                        }
                        if ui.button("Cancel").clicked() {
                            self.chrome_confirm_host = false;
                        }
                    });
                }
                ui.separator();
                if let Some(path) = &self.chrome_path {
                    ui.colored_label(Color32::from_rgb(91, 190, 145), "Google Chrome detected on the Windows host");
                    ui.label(RichText::new(path.display().to_string()).small().weak());
                    ui.label("Open isolated uses a fresh Chrome profile in Windows Sandbox. Its downloaded files disappear when Sandbox closes. Use the offline Inspect in Sandbox action for a file you already saved to NOVA Downloads.");
                } else {
                    ui.colored_label(Color32::from_rgb(225, 166, 86), "Google Chrome was not found on this PC.");
                    ui.label("Chrome is not bundled or installed by NOVA. Download it from Google, then choose Detect.");
                    if ui.button("Open official Google Chrome download page").clicked() {
                        match apps::open_chrome_download_page() {
                            Ok(()) => self.status_message = "Opened Google's official Chrome download page.".to_owned(),
                            Err(error) => self.status_message = format!("Could not open download page: {error}"),
                        }
                    }
                }
                ui.label(RichText::new("The isolated browser requires Windows Sandbox (Pro/Enterprise/Education + hardware virtualization). Network is enabled for browsing and may reach local-network services; vGPU, clipboard and writable host folders are disabled.").small().weak());
            });
        if open_isolated_now { self.launch_chrome_in_sandbox(); }
        if open_host_now { self.launch_chrome(); }
    }

    fn calculator_window(&mut self, ctx: &egui::Context) {
        let mut calculate = false;
        let snap = self.take_window_snap("Calculator", ctx);
        let mut window = egui::Window::new("NOVA Calculator")
            .fade_in(!self.settings.reduce_motion)
            .fade_out(!self.settings.reduce_motion)
            .resizable(false)
            .default_width(340.0);
        if let Some((position, size)) = snap { window = window.current_pos(position).fixed_size(size); }
        window.open(&mut self.calculator_open).show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.add(TextEdit::singleline(&mut self.calc_left).desired_width(90.0));
                    egui::ComboBox::from_label("")
                        .selected_text(&self.calc_operator)
                        .show_ui(ui, |ui| {
                            for op in ["+", "−", "×", "÷"] {
                                ui.selectable_value(&mut self.calc_operator, op.to_owned(), op);
                            }
                        });
                    ui.add(TextEdit::singleline(&mut self.calc_right).desired_width(90.0));
                });
                ui.horizontal(|ui| {
                    if ui.button("Calculate").clicked() { calculate = true; }
                    if ui.button("Clear").clicked() {
                        self.calc_left.clear();
                        self.calc_right.clear();
                        self.calc_result = "0".to_owned();
                    }
                    if ui.button("Swap").clicked() { std::mem::swap(&mut self.calc_left, &mut self.calc_right); }
                });
                ui.heading(RichText::new(&self.calc_result).size(26.0).color(Color32::from_rgb(126, 184, 255)));
            });
        if calculate {
            self.calculate();
        }
    }

    fn calculate(&mut self) {
        let left = self.calc_left.trim().parse::<f64>();
        let right = self.calc_right.trim().parse::<f64>();
        self.calc_result = match (left, right) {
            (Ok(a), Ok(b)) => match self.calc_operator.as_str() {
                "+" => format_number(a + b),
                "−" => format_number(a - b),
                "×" => format_number(a * b),
                "÷" if b != 0.0 => format_number(a / b),
                "÷" => "Cannot divide by zero".to_owned(),
                _ => "Unknown operator".to_owned(),
            },
            _ => "Enter valid numbers".to_owned(),
        };
    }

    fn monitor_window(&mut self, ctx: &egui::Context) {
        if self.monitor_open && self.monitor.is_none() && self.monitor_error.is_none() {
            match System::new() {
                Ok(system) => self.monitor = Some(system),
                Err(error) => self.monitor_error = Some(format!("System Monitor unavailable: {error:?}")),
            }
        }
        let metrics = if self.monitor_open {
            if let Some(monitor) = self.monitor.as_mut() {
                if self.last_monitor_refresh.elapsed() >= Duration::from_secs(1) {
                    // Refresh only the metrics shown here. refresh_all() also walks every
                    // process and gathers expensive details that the monitor never displays.
                    monitor.refresh_cpu_usage();
                    monitor.refresh_memory();
                    self.last_monitor_refresh = Instant::now();
                }
                if self.last_process_refresh.elapsed() >= Duration::from_secs(10) {
                    monitor.refresh_processes_specifics(
                        ProcessesToUpdate::All,
                        true,
                        ProcessRefreshKind::nothing().without_tasks(),
                    );
                    self.last_process_refresh = Instant::now();
                }
                let cpu = monitor.global_cpu_usage();
                let used = monitor.used_memory() as f64 / 1024.0 / 1024.0 / 1024.0;
                let total = monitor.total_memory() as f64 / 1024.0 / 1024.0 / 1024.0;
                let processes = monitor.processes().len();
                Some((cpu, used, total, processes))
            } else {
                None
            }
        } else {
            None
        };
        let monitor_error = self.monitor_error.clone();

        let snap = self.take_window_snap("Monitor", ctx);
        let mut window = egui::Window::new("NOVA System Monitor")
            .fade_in(!self.settings.reduce_motion)
            .fade_out(!self.settings.reduce_motion)
            .default_size([430.0, 300.0]);
        if let Some((position, size)) = snap { window = window.current_pos(position).fixed_size(size); }
        window.open(&mut self.monitor_open).show(ctx, |ui| {
                if let Some((cpu, used, total, processes)) = metrics {
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.label(RichText::new("CPU LOAD").small().weak().strong());
                            ui.heading(format!("{cpu:.1}%"));
                        });
                        ui.separator();
                        ui.vertical(|ui| {
                            ui.label(RichText::new("RUNNING PROCESSES").small().weak().strong());
                            ui.heading(processes.to_string());
                        });
                    });
                    ui.add(egui::ProgressBar::new((cpu / 100.0).clamp(0.0, 1.0)).show_percentage());
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("MEMORY").small().weak().strong());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(format!("{used:.2} / {total:.2} GB"));
                        });
                    });
                    ui.add(egui::ProgressBar::new(if total > 0.0 { (used / total) as f32 } else { 0.0 }).show_percentage());
                } else {
                    ui.colored_label(
                        Color32::LIGHT_RED,
                        monitor_error.as_deref().unwrap_or("System information is unavailable on this device."),
                    );
                }
                ui.separator();
                ui.label(RichText::new("Metrics are provided by the Windows host; NOVA is a desktop environment, not a separate kernel.").small().weak());
            });
    }
}

impl eframe::App for NovaApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.poll_unikey_child();
        let ctx = ui.ctx().clone();
        if self.unikey_child.is_some() {
            // Poll child liveness at 2 Hz instead of waking the renderer five times per second.
            ctx.request_repaint_after(Duration::from_millis(500));
        }
        self.apply_theme(&ctx);
        let (toggle_start, open_files, open_monitor, close_panels) = ctx.input(|input| (
            input.modifiers.ctrl && input.key_pressed(egui::Key::Space),
            input.modifiers.ctrl && input.key_pressed(egui::Key::E),
            input.modifiers.ctrl && input.modifiers.shift && input.key_pressed(egui::Key::Escape),
            input.key_pressed(egui::Key::Escape),
        ));
        if toggle_start { self.start_open = !self.start_open; self.notification_open = false; }
        if open_files { self.files_open = true; self.ensure_files_loaded(); }
        if open_monitor { self.monitor_open = true; }
        if close_panels { self.start_open = false; self.notification_open = false; }

        let dark = self.is_dark_mode(&ctx);
        let desktop_fill = if dark { Color32::from_rgb(12, 20, 36) } else { Color32::from_rgb(222, 232, 247) };
        egui::Frame::new()
            .fill(if dark { Color32::from_rgb(9, 14, 25) } else { Color32::from_rgb(231, 237, 247) })
            .inner_margin(egui::Margin::same(8))
            .show(ui, |ui| {
                ui.vertical(|ui| {
                    let taskbar_height = 60.0;
                    let desktop_outer_height = (ui.available_height() - taskbar_height - 8.0).max(270.0);
                    // The desktop frame adds 18 pt of top and bottom inner margin.
                    let desktop_content_height = (desktop_outer_height - 36.0).max(220.0);
                    egui::Frame::new()
                        .fill(desktop_fill)
                        .stroke(egui::Stroke::new(1.0, if dark { Color32::from_white_alpha(18) } else { Color32::from_black_alpha(14) }))
                        .corner_radius(egui::CornerRadius::same(16))
                        .inner_margin(egui::Margin::same(18))
                        .show(ui, |ui| {
                            ui.set_min_size(egui::vec2(ui.available_width(), desktop_content_height));
                            let rect = ui.max_rect();
                            if !self.settings.reduce_motion {
                                ui.painter().circle_filled(
                                    rect.right_top() + egui::vec2(-155.0, 165.0),
                                    265.0,
                                    Color32::from_rgba_unmultiplied(77, 119, 213, if dark { 24 } else { 34 }),
                                );
                                ui.painter().circle_filled(
                                    rect.left_bottom() + egui::vec2(130.0, -45.0),
                                    205.0,
                                    Color32::from_rgba_unmultiplied(38, 158, 151, if dark { 18 } else { 25 }),
                                );
                            }

                            ui.vertical(|ui| {
                                ui.horizontal(|ui| {
                                    ui.vertical(|ui| {
                                        ui.label(RichText::new("NOVA OS").size(22.0).strong().color(if dark { Color32::WHITE } else { Color32::from_rgb(30, 48, 76) }));
                                        ui.label(RichText::new("Portable desktop workspace").small().color(if dark { Color32::from_rgb(173, 190, 218) } else { Color32::from_rgb(82, 101, 132) }));
                                    });
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        let mode = if self.paths.portable { "● USB PORTABLE" } else { "● LOCAL MODE" };
                                        ui.label(RichText::new(mode).small().color(Color32::from_rgb(105, 190, 158)));
                                    });
                                });
                                ui.add_space(17.0);
                                ui.label(RichText::new("Desktop").small().strong().color(if dark { Color32::from_rgb(194, 207, 229) } else { Color32::from_rgb(74, 91, 119) }));
                                ui.add_space(7.0);

                                let desktop_apps = [
                                    "Files", "Terminal", "Editor",
                                    "Calculator", "Google Chrome", "UniKey",
                                    "Trash", "Security Center", "Settings",
                                ];
                                let desktop_columns = ((ui.available_width() / 108.0).floor() as usize).clamp(2, 7);
                                egui::Grid::new("nova-desktop-shortcuts")
                                    .num_columns(desktop_columns)
                                    .spacing(egui::vec2(5.0, 6.0))
                                    .show(ui, |ui| {
                                        for (index, name) in desktop_apps.iter().enumerate() {
                                            if let Some(app) = START_APPS.iter().find(|app| app.name == name).copied() {
                                                if self.app_icon_button(ui, app, egui::vec2(94.0, 88.0), true) {
                                                    self.open_app(app.name);
                                                }
                                            }
                                            if index % desktop_columns == desktop_columns - 1 { ui.end_row(); }
                                        }
                                    });

                                ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                                    egui::Frame::new()
                                        .fill(Color32::from_rgba_unmultiplied(255, 255, 255, if dark { 12 } else { 125 }))
                                        .corner_radius(egui::CornerRadius::same(9))
                                        .inner_margin(egui::Margin::symmetric(11, 7))
                                        .show(ui, |ui| {
                                            ui.label(RichText::new(&self.status_message).small().color(if dark { Color32::from_rgb(188, 202, 225) } else { Color32::from_rgb(63, 78, 102) }));
                                        });
                                });
                            });
                        });

                    ui.add_space(8.0);
                    self.taskbar(ui);
                });
            });

        self.start_menu(&ctx);
        self.notification_center(&ctx);
        self.terminal_window(&ctx);
        self.files_window(&ctx);
        self.editor_window(&ctx);
        self.settings_window(&ctx);
        self.calculator_window(&ctx);
        self.monitor_window(&ctx);
        self.trash_window(&ctx);
        self.chrome_window(&ctx);
        self.unikey_window(&ctx);
        self.security_window(&ctx);
        for info in &security_tools::TOOLS {
            self.security_tool_window(&ctx, info.tool);
        }
        if self.monitor_open {
            // The monitor data is refreshed once per second; faster frames add no information.
            ctx.request_repaint_after(Duration::from_secs(1));
        }
    }
}

fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 { size /= 1024.0; unit += 1; }
    if unit == 0 { format!("{bytes} B") } else { format!("{size:.1} {}", UNITS[unit]) }
}

fn relative_age(timestamp: u64) -> String {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
    let age = now.saturating_sub(timestamp);
    if age < 60 { "just now".to_owned() }
    else if age < 3600 { format!("{}m ago", age / 60) }
    else if age < 86400 { format!("{}h ago", age / 3600) }
    else { format!("{}d ago", age / 86400) }
}

fn format_number(value: f64) -> String {
    if !value.is_finite() {
        return "Result is not finite".to_owned();
    }
    if value == 0.0 {
        return "0".to_owned();
    }
    value.to_string()
}

#[cfg(test)]
mod tests {
    use super::{format_number, START_APPS, START_PINS};
    use crate::storage::Settings;

    #[test]
    fn start_app_catalog_has_unique_names_and_valid_categories() {
        let allowed = ["Workspace", "Productivity", "System", "Internet", "Security"];
        for (index, app) in START_APPS.iter().enumerate() {
            assert!(!app.name.trim().is_empty());
            assert!(!app.icon.trim().is_empty());
            assert!(allowed.contains(&app.category));
            assert!(!START_APPS[..index].iter().any(|previous| previous.name == app.name));
        }
    }

    #[test]
    fn start_pins_reference_only_catalog_apps_without_duplicates() {
        for (index, name) in START_PINS.iter().enumerate() {
            assert!(START_APPS.iter().any(|app| app.name == *name));
            assert!(!START_PINS[..index].contains(name));
        }
    }

    #[test]
    fn default_start_pins_match_the_validated_app_catalog() {
        let settings = Settings::default();
        let expected: Vec<String> = START_PINS.iter().map(|name| (*name).to_owned()).collect();
        assert_eq!(settings.pinned_apps, expected);
    }

    #[test]
    fn legacy_settings_json_receives_default_start_pins() {
        let legacy = r#"{"theme":"dark","reduce_motion":false,"accent":"blue"}"#;
        let settings: Settings = serde_json::from_str(legacy).expect("legacy settings should remain readable");
        assert_eq!(settings.pinned_apps, Settings::default().pinned_apps);
    }

    #[test]
    fn calculator_format_does_not_round_small_values_to_zero() {
        assert_eq!(format_number(0.000000001), "0.000000001");
        assert_eq!(format_number(15.0), "15");
        assert_eq!(format_number(-0.0), "0");
    }

    #[test]
    fn calculator_rejects_non_finite_results() {
        assert_eq!(format_number(f64::INFINITY), "Result is not finite");
        assert_eq!(format_number(f64::NAN), "Result is not finite");
    }
}

impl Drop for NovaApp {
    fn drop(&mut self) {
        // UniKey is a host process that NOVA explicitly started; stop it when
        // NOVA closes. The downloader is a short-lived PowerShell helper with
        // a `finally` cleanup block, so detach it and let cleanup finish rather
        // than hard-killing it and leaving staging files in the host TEMP.
        if self.unikey_process_kind == Some(UniKeyProcessKind::Running) {
            if let Some(mut child) = self.unikey_child.take() {
                if child.try_wait().ok().flatten().is_none() {
                    let _ = child.kill();
                    let _ = child.wait();
                }
            }
        } else {
            self.unikey_child.take();
        }
    }
}
