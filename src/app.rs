use crate::storage::{canonical_user_path, create_user_directories, safe_user_path, AppPaths, Settings};
use crate::terminal::{TerminalState, TerminalTask};
use crate::trash::{self, TrashEntry};
use crate::apps;
use crate::{isolation, unikey, unicode};
use eframe::egui::{self, Color32, FontId, RichText, TextEdit};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};
use std::process::Child;
use sysinfo::System;

#[derive(Clone, Copy, Debug)]
enum UniKeyProcessKind { Download, Running }

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
    launcher_open: bool,
    trash_open: bool,
    chrome_open: bool,
    unikey_open: bool,
    security_open: bool,
    security_confirm_clear_quarantine: bool,
    chrome_confirm_host: bool,
    trash_confirm_empty: bool,
    trash_confirm_delete: bool,
    trash_entries: Vec<TrashEntry>,
    trash_selected: Option<String>,
    files_selected: Option<PathBuf>,
    launcher_search: String,
    chrome_path: Option<PathBuf>,
    unikey_path: Option<PathBuf>,
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
    status_message: String,
}

impl NovaApp {
    pub fn new(cc: &eframe::CreationContext<'_>, paths: AppPaths) -> Self {
        let (settings, settings_warning) = match paths.load_settings() {
            Ok(settings) => (settings, None),
            Err(error) => (Settings::default(), Some(format!("Settings could not be loaded; defaults are active: {error}"))),
        };
        let portable = paths.portable;
        let unikey_path = unikey::find_portable(&paths.app_dir);
        let quarantine_count = isolation::quarantine_item_count(&paths).unwrap_or(0);
        let (monitor, monitor_error) = match System::new() {
            Ok(system) => (Some(system), None),
            Err(error) => (None, Some(format!("System Monitor unavailable: {error:?}"))),
        };
        let app = Self {
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
            launcher_open: false,
            trash_open: false,
            chrome_open: false,
            unikey_open: false,
            security_open: false,
            security_confirm_clear_quarantine: false,
            chrome_confirm_host: false,
            trash_confirm_empty: false,
            trash_confirm_delete: false,
            trash_entries: Vec::new(),
            trash_selected: None,
            files_selected: None,
            launcher_search: String::new(),
            chrome_path: apps::find_chrome(),
            chrome_url: "https://www.google.com".to_owned(),
            unikey_path,
            unikey_child: None,
            unikey_process_kind: None,
            unikey_status: "UniKey is not running.".to_owned(),
            quarantine_count,
            files_new_folder: String::new(),
            editor_path: "Documents/Note.txt".to_owned(),
            editor_text: String::new(),
            editor_status: "Ready".to_owned(),
            calc_left: "12".to_owned(),
            calc_right: "3".to_owned(),
            calc_operator: "+".to_owned(),
            calc_result: "15".to_owned(),
            monitor,
            monitor_error,
            last_monitor_refresh: Instant::now() - Duration::from_secs(2),
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

    fn apply_theme(&self, ctx: &egui::Context) {
        let mut visuals = if self.settings.theme == "light" {
            egui::Visuals::light()
        } else {
            egui::Visuals::dark()
        };
        if self.settings.theme == "light" {
            visuals.panel_fill = Color32::from_rgb(242, 245, 250);
            visuals.window_fill = Color32::from_rgb(255, 255, 255);
            visuals.extreme_bg_color = Color32::from_rgb(233, 237, 244);
        } else {
            visuals.panel_fill = Color32::from_rgb(17, 23, 38);
            visuals.window_fill = Color32::from_rgb(23, 31, 49);
            visuals.extreme_bg_color = Color32::from_rgb(10, 15, 27);
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
    }

    fn open_app(&mut self, app: &str) {
        match app {
            "Files" => self.files_open = true,
            "Terminal" => self.terminal_open = true,
            "Editor" => self.editor_open = true,
            "Settings" => self.settings_open = true,
            "Calculator" => self.calculator_open = true,
            "Monitor" => self.monitor_open = true,
            "Trash" => {
                self.trash_open = true;
                self.refresh_trash();
            }
            "Google Chrome" => self.chrome_open = true,
            "UniKey" => self.unikey_open = true,
            "Security Center" => self.security_open = true,
            _ => {}
        }
        self.launcher_open = false;
    }

    /// Clicking an active taskbar item hides its workspace window; clicking it
    /// again from the launcher or desktop brings it back.
    fn toggle_app(&mut self, app: &str) {
        match app {
            "Files" => self.files_open = !self.files_open,
            "Terminal" => self.terminal_open = !self.terminal_open,
            "Editor" => self.editor_open = !self.editor_open,
            "Settings" => self.settings_open = !self.settings_open,
            "Calculator" => self.calculator_open = !self.calculator_open,
            "Monitor" => self.monitor_open = !self.monitor_open,
            "Trash" => {
                self.trash_open = !self.trash_open;
                if self.trash_open { self.refresh_trash(); }
            }
            "Google Chrome" => self.chrome_open = !self.chrome_open,
            "UniKey" => self.unikey_open = !self.unikey_open,
            "Security Center" => self.security_open = !self.security_open,
            _ => {}
        }
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
        let available = isolation::is_available();
        egui::Window::new("NOVA Security Center")
            .open(&mut self.security_open)
            .fade_in(!self.settings.reduce_motion)
            .fade_out(!self.settings.reduce_motion)
            .default_size([620.0, 510.0])
            .show(ctx, |ui| {
                ui.heading("Isolation status");
                if available {
                    ui.colored_label(Color32::from_rgb(87, 190, 140), "WindowsSandbox.exe found — feature launch still depends on Windows edition and virtualization settings");
                } else {
                    ui.colored_label(Color32::from_rgb(225, 166, 86), "Windows Sandbox launcher was not found");
                }
                ui.label("Requires Windows Pro, Enterprise or Education (not Home), hardware virtualization enabled, at least 4 GB RAM (8 GB recommended), 1 GB free disk space and 2 CPU cores.");
                ui.label("Windows Sandbox allows one running instance at a time. NOVA will fail closed if Sandbox is unavailable; it will not run suspicious files on the host as a fallback.");
                ui.separator();
                ui.heading("What is isolated?");
                ui.label("• Inspect file in Sandbox: NOVA copies one selected file into Data/Security/Quarantine, maps only that folder read-only, disables networking, vGPU and clipboard, and opens Explorer. You must manually run the sample inside Sandbox.");
                ui.label("• Isolated Chrome: starts with a fresh temporary profile inside Windows Sandbox. The installed Chrome program folder is read-only; profile, cookies, extensions, cache and downloads stay inside the disposable VM. Networking is enabled so websites load and can expose the VM to your local network.");
                ui.label("• Close Windows Sandbox and confirm its deletion to discard the VM's software, downloads and state. The staged sample copy and NOVA launch-config files remain on the USB until you clear them below.");
                ui.separator();
                ui.heading("Important security boundary");
                ui.colored_label(Color32::from_rgb(225, 166, 86), "NOVA is a normal Windows process, not a virtual machine. Terminal/CMD, UniKey, Wi-Fi, Open on host and ordinary Files operations still run on or modify the Windows host.");
                ui.label("Never execute unknown files or commands on the host. Use Inspect in Sandbox for untrusted files and Open isolated for browsing. If Windows Sandbox is unavailable, do not use the host launch buttons as a substitute.");
                ui.label("No software can promise absolute protection against every hypervisor, Windows or firmware vulnerability. Keep Windows and Defender updated; do not map writable host folders into Sandbox.");
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label(format!("Staged sample/config items: {}", self.quarantine_count));
                    if ui.add_enabled(self.quarantine_count > 0, egui::Button::new("Clear staged copies…")).clicked() {
                        self.security_confirm_clear_quarantine = true;
                    }
                });
                if self.security_confirm_clear_quarantine {
                    ui.colored_label(Color32::from_rgb(225, 166, 86), "Close all Windows Sandbox sessions before clearing staged files.");
                    ui.horizontal(|ui| {
                        if ui.button("Clear staged copies").clicked() {
                            match isolation::clear_quarantine(&self.paths) {
                                Ok(count) => self.status_message = format!("Removed {count} staged sample/config item(s). Original workspace files were not changed."),
                                Err(error) => self.status_message = format!("Could not clear staged samples: {error}"),
                            }
                            self.quarantine_count = isolation::quarantine_item_count(&self.paths).unwrap_or(0);
                            self.security_confirm_clear_quarantine = false;
                        }
                        if ui.button("Cancel").clicked() { self.security_confirm_clear_quarantine = false; }
                    });
                }
                if !available {
                    ui.separator();
                    ui.label("Enable Windows Sandbox from Turn Windows features on or off, then restart Windows. On supported Pro/Enterprise/Education editions, enable the Windows Sandbox optional feature and CPU virtualization. Windows Home does not include this feature.");
                }
            });
    }

    fn unikey_window(&mut self, ctx: &egui::Context) {
        egui::Window::new("UniKey — Vietnamese Input")
            .open(&mut self.unikey_open)
            .fade_in(!self.settings.reduce_motion)
            .fade_out(!self.settings.reduce_motion)
            .resizable(false)
            .default_width(540.0)
            .show(ctx, |ui| {
                ui.heading("Vietnamese typing for NOVA and Windows");
                ui.label("NOVA uses Unicode text fields and system-font fallback for installed glyphs. UniKey adds Vietnamese Telex/VNI input on the Windows host, including NOVA Editor and Terminal.");
                ui.separator();
                self.unikey_path = unikey::find_portable(&self.paths.app_dir);
                if let Some(path) = &self.unikey_path {
                    ui.colored_label(Color32::from_rgb(87, 190, 140), "Portable UniKey executable found on this USB");
                    ui.label(RichText::new(path.display().to_string()).small().weak());
                } else {
                    ui.colored_label(Color32::from_rgb(225, 166, 86), "UniKey is not on this USB yet");
                    ui.label("Download the official 64-bit UniKey build. NOVA verifies the Authenticode signature publisher before launching it.");
                }
                ui.horizontal(|ui| {
                    if ui.add_enabled(self.unikey_child.is_none(), egui::Button::new("Download UniKey to USB")).clicked() {
                        self.download_unikey();
                    }
                    if ui.add_enabled(self.unikey_path.is_some() && self.unikey_child.is_none(), egui::Button::new("Start UniKey")).clicked() {
                        self.start_unikey();
                    }
                    if self.unikey_process_kind == Some(UniKeyProcessKind::Running)
                        && ui.button("Stop UniKey").clicked()
                    {
                        self.stop_unikey();
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
    }

    fn sidebar_button(&mut self, ui: &mut egui::Ui, glyph: &str, app: &str) {
        if ui
            .add_sized(
                [ui.available_width(), 38.0],
                egui::Button::new(RichText::new(format!("  {glyph}     {app}")).size(14.0))
                    .corner_radius(egui::CornerRadius::same(8)),
            )
            .clicked()
        {
            self.open_app(app);
        }
    }

    fn desktop_tile(&mut self, ui: &mut egui::Ui, glyph: &str, app: &str, description: &str) {
        let button = egui::Frame::new()
            .fill(Color32::from_rgba_unmultiplied(255, 255, 255, 16))
            .stroke(egui::Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 30)))
            .corner_radius(egui::CornerRadius::same(15))
            .inner_margin(egui::Margin::same(15))
            .show(ui, |ui| {
                ui.set_min_size(egui::vec2(174.0, 112.0));
                ui.vertical(|ui| {
                    ui.label(RichText::new(glyph).size(27.0).color(Color32::from_rgb(151, 179, 255)));
                    ui.label(RichText::new(app).strong().color(Color32::WHITE));
                    ui.label(RichText::new(description).small().color(Color32::from_rgb(188, 201, 224)));
                });
            });
        let response = ui.interact(button.response.rect, ui.scope_id().with(app), egui::Sense::click());
        let hover = if self.settings.reduce_motion {
            if response.hovered() { 1.0 } else { 0.0 }
        } else {
            ui.ctx().animate_bool_with_time(ui.scope_id().with(("tile-hover", app)), response.hovered(), 0.15)
        };
        if hover > 0.01 {
            ui.painter().rect_filled(
                response.rect,
                egui::CornerRadius::same(15),
                Color32::from_rgba_unmultiplied(112, 147, 236, (hover * 18.0) as u8),
            );
        }
        if response.clicked() {
            self.open_app(app);
        }
        ui.add_space(8.0);
    }

    fn launcher_window(&mut self, ctx: &egui::Context) {
        let visibility = if self.settings.reduce_motion {
            if self.launcher_open { 1.0 } else { 0.0 }
        } else {
            ctx.animate_bool_with_time(egui::Id::new("nova-start-menu-visibility"), self.launcher_open, 0.18)
        };
        if visibility <= 0.001 { return; }
        let mut selected_app: Option<&'static str> = None;
        egui::Area::new(egui::Id::new("nova-start-menu"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::LEFT_BOTTOM, egui::vec2(14.0, -66.0))
            .fade_in(!self.settings.reduce_motion)
            .show(ctx, |ui| {
                ui.set_opacity(if self.settings.reduce_motion { 1.0 } else { visibility });
                egui::Frame::popup(ui.style())
                    .corner_radius(egui::CornerRadius::same(18))
                    .inner_margin(egui::Margin::same(18))
                    .show(ui, |ui| {
                        ui.set_min_width(390.0);
                        ui.horizontal(|ui| {
                            ui.heading(RichText::new("NOVA Launcher").strong());
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.small_button("×").clicked() { selected_app = Some("__close__"); }
                            });
                        });
                        ui.label(RichText::new("Find and open your workspace tools").small().weak());
                        ui.add_space(8.0);
                        ui.add(TextEdit::singleline(&mut self.launcher_search)
                            .hint_text("Search applications…")
                            .desired_width(f32::INFINITY));
                        ui.add_space(10.0);
                        let apps = [
                            ("Files", "▦", "Browse your NOVA workspace"),
                            ("Terminal", "⌘", "Windows CMD and Wi-Fi tools"),
                            ("Editor", "✎", "Write and edit text files"),
                            ("Calculator", "∑", "Quick calculations"),
                            ("Monitor", "◉", "CPU, memory and process stats"),
                            ("Trash", "▤", "Restore deleted NOVA files"),
                            ("Google Chrome", "🌐", "Open Chrome normally or isolated"),
                            ("UniKey", "文", "Vietnamese Unicode Telex / VNI input"),
                            ("Security Center", "🛡", "Windows Sandbox and file isolation"),
                            ("Settings", "⚙", "Theme, accent and motion"),
                        ];
                        let query = self.launcher_search.to_lowercase();
                        egui::Grid::new("nova-launcher-grid")
                            .num_columns(2)
                            .spacing(egui::vec2(10.0, 8.0))
                            .show(ui, |ui| {
                                let mut column = 0;
                                for (name, icon, description) in apps {
                                    if !query.is_empty() && !name.to_lowercase().contains(&query) && !description.to_lowercase().contains(&query) {
                                        continue;
                                    }
                                    let label = format!("{icon}  {name}\n{description}");
                                    if ui.add_sized([178.0, 54.0], egui::Button::new(RichText::new(label).size(12.5))).clicked() {
                                        selected_app = Some(name);
                                    }
                                    column += 1;
                                    if column % 2 == 0 { ui.end_row(); }
                                }
                            });
                        if self.chrome_path.is_none() {
                            ui.add_space(5.0);
                            ui.label(RichText::new("Chrome is optional and is not bundled with NOVA.").small().weak());
                        }
                        let visible_count = apps.iter().filter(|(name, _, description)| {
                            query.is_empty() || name.to_lowercase().contains(&query) || description.to_lowercase().contains(&query)
                        }).count();
                        if visible_count == 0 {
                            ui.add_space(8.0);
                            ui.label(RichText::new("No applications match your search.").weak());
                        }
                    });
            });
        if let Some(app) = selected_app {
            if app == "__close__" {
                self.launcher_open = false;
            } else {
                self.open_app(app);
            }
        }
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
            ctx.request_repaint_after(Duration::from_millis(50));
        }
    }

    fn terminal_window(&mut self, ctx: &egui::Context) {
        self.poll_terminal_task(ctx);
        let mut submit = false;
        let mut clear_output = false;
        egui::Window::new("NOVA Terminal — Windows CMD")
            .fade_in(!self.settings.reduce_motion)
            .fade_out(!self.settings.reduce_motion)
            .open(&mut self.terminal_open)
            .default_size([760.0, 490.0])
            .min_width(540.0)
            .show(ctx, |ui| {
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
        egui::Window::new("▦  NOVA Files")
            .open(&mut self.files_open)
            .fade_in(!self.settings.reduce_motion)
            .fade_out(!self.settings.reduce_motion)
            .default_size([760.0, 520.0])
            .min_width(540.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if ui.button("↑  Up").clicked() && self.files_dir != self.paths.user_dir {
                        navigate_to = self.files_dir.parent().map(Path::to_path_buf);
                    }
                    if ui.button("↻  Refresh").clicked() { refresh = true; }
                    ui.separator();
                    ui.label(RichText::new(self.files_dir.strip_prefix(&self.paths.user_dir).unwrap_or(Path::new(".")).display().to_string()).weak());
                });
                ui.separator();
                ui.horizontal(|ui| {
                    ui.add(TextEdit::singleline(&mut self.files_new_folder).hint_text("New folder name").desired_width(210.0));
                    if ui.button("＋ New folder").clicked() { create_folder = true; }
                    if let Some(selected) = self.files_selected.as_ref() {
                        if ui.button("Open").clicked() {
                            if selected.is_dir() { navigate_to = Some(selected.clone()); }
                            else { open_file = Some(selected.clone()); }
                        }
                        if ui.button("Move to Trash").clicked() { move_to_trash = Some(selected.clone()); }
                        if selected.is_file() && ui.button("Inspect in Sandbox").clicked() { sandbox_sample = Some(selected.clone()); }
                    }
                });
                ui.separator();
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    let safe_current = canonical_user_path(&self.paths.user_dir, &self.files_dir);
                    match safe_current.and_then(|folder| fs::read_dir(folder)) {
                        Ok(entries) => {
                            let mut rows = entries.filter_map(Result::ok).collect::<Vec<_>>();
                            rows.sort_by_key(|entry| (!entry.file_type().map(|t| t.is_dir()).unwrap_or(false), entry.file_name()));
                            if rows.is_empty() {
                                ui.add_space(18.0);
                                ui.label(RichText::new("This folder is empty").weak());
                            }
                            for entry in rows {
                                let Ok(kind) = entry.file_type() else { continue };
                                if kind.is_symlink() { continue; }
                                if canonical_user_path(&self.paths.user_dir, &entry.path()).is_err() { continue; }
                                let path = entry.path();
                                let name = entry.file_name().to_string_lossy().to_string();
                                let is_dir = kind.is_dir();
                                let size = entry.metadata().ok().filter(|meta| meta.is_file()).map(|meta| format_size(meta.len())).unwrap_or_else(|| if is_dir { "Folder".to_owned() } else { "File".to_owned() });
                                let selected = self.files_selected.as_ref() == Some(&path);
                                let response = ui.selectable_label(selected, format!("{}  {:<34}  {}", if is_dir { "▰" } else { "▤" }, name, size));
                                if response.clicked() { self.files_selected = Some(path.clone()); }
                                if response.double_clicked() {
                                    if is_dir { navigate_to = Some(path); } else { open_file = Some(path); }
                                }
                            }
                        }
                        Err(error) => { ui.colored_label(Color32::LIGHT_RED, format!("Cannot read folder: {error}")); }
                    }
                });
                ui.separator();
                ui.label(RichText::new("Single-click to select · Double-click to open · Deleted items stay in NOVA Trash until permanently removed").small().weak());
            });

        if refresh { self.files_selected = None; }
        if let Some(path) = navigate_to {
            match canonical_user_path(&self.paths.user_dir, &path) {
                Ok(folder) if folder.is_dir() => { self.files_dir = folder; self.files_selected = None; }
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
                        Ok(()) => { self.files_new_folder.clear(); self.status_message = format!("Created folder: {name}"); }
                        Err(error) => self.status_message = format!("Create folder failed: {error}"),
                    },
                    (Err(error), _) | (_, Err(error)) => self.status_message = format!("Create folder blocked: {error}"),
                }
            }
        }
        if let Some(path) = move_to_trash {
            match trash::move_to_trash(&self.paths, &path) {
                Ok(entry) => { self.status_message = format!("Moved '{}' to NOVA Trash", entry.name); self.files_selected = None; self.refresh_trash(); }
                Err(error) => self.status_message = format!("Could not move item to Trash: {error}"),
            }
        }
        if let Some(path) = sandbox_sample {
            match isolation::launch_sample_in_sandbox(&self.paths, &path) {
                Ok(_) => {
                    self.quarantine_count = isolation::quarantine_item_count(&self.paths).unwrap_or(self.quarantine_count + 1);
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
        egui::Window::new("NOVA Text Editor")
            .fade_in(!self.settings.reduce_motion)
            .fade_out(!self.settings.reduce_motion)
            .open(&mut self.editor_open)
            .default_size([700.0, 520.0])
            .show(ctx, |ui| {
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
        egui::Window::new("NOVA Settings")
            .fade_in(!self.settings.reduce_motion)
            .fade_out(!self.settings.reduce_motion)
            .open(&mut self.settings_open)
            .resizable(false)
            .default_width(400.0)
            .show(ctx, |ui| {
                ui.heading("Appearance");
                ui.horizontal(|ui| {
                    changed |= ui.selectable_value(&mut self.settings.theme, "dark".to_owned(), "Dark").changed();
                    changed |= ui.selectable_value(&mut self.settings.theme, "light".to_owned(), "Light").changed();
                });
                ui.horizontal(|ui| {
                    ui.label("Accent color");
                    egui::ComboBox::from_id_salt("nova-accent")
                        .selected_text(self.settings.accent.to_uppercase())
                        .show_ui(ui, |ui| {
                            for accent in ["blue", "violet", "teal", "rose", "amber"] {
                                changed |= ui.selectable_value(&mut self.settings.accent, accent.to_owned(), accent.to_uppercase()).changed();
                            }
                        });
                });
                ui.separator();
                changed |= ui.checkbox(&mut self.settings.reduce_motion, "Reduce visual effects").changed();
                ui.separator();
                ui.heading("Language & Security");
                if ui.button("Open UniKey Vietnamese input").clicked() { self.unikey_open = true; }
                if ui.button("Open Security Center").clicked() { self.security_open = true; }
                ui.label(RichText::new("NOVA Editor supports UTF-8 and UTF-16 text files. System-wide Vietnamese typing is provided by the verified portable UniKey companion.").small().weak());
                ui.separator();
                ui.label(RichText::new("Portable mode stores settings and user data in Data next to NOVA.exe.").small().weak());
                ui.label(RichText::new("NOVA does not install services or register startup tasks.").small().weak());
            });
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
        egui::Window::new("▤  NOVA Trash")
            .open(&mut self.trash_open)
            .fade_in(!self.settings.reduce_motion)
            .fade_out(!self.settings.reduce_motion)
            .default_size([690.0, 450.0])
            .show(ctx, |ui| {
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
                        if ui.button("↻").on_hover_text("Refresh Trash").clicked() { self.refresh_trash(); }
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
        let mut open_host_now = false;
        egui::Window::new("🌐  Google Chrome")
            .open(&mut self.chrome_open)
            .fade_in(!self.settings.reduce_motion)
            .fade_out(!self.settings.reduce_motion)
            .resizable(false)
            .default_width(540.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Open a website in Google Chrome").strong());
                    if ui.small_button("↻ Detect").on_hover_text("Search common Chrome installation paths again").clicked() {
                        self.chrome_path = apps::find_chrome();
                    }
                });
                ui.add(TextEdit::singleline(&mut self.chrome_url)
                    .hint_text("https://example.com")
                    .desired_width(f32::INFINITY));
                ui.horizontal(|ui| {
                    if ui.add_enabled(self.chrome_path.is_some(), egui::Button::new("Open isolated (recommended)")).clicked() {
                        self.launch_chrome_in_sandbox();
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
        if open_host_now {
            self.launch_chrome();
        }
    }

    fn calculator_window(&mut self, ctx: &egui::Context) {
        let mut calculate = false;
        egui::Window::new("NOVA Calculator")
            .fade_in(!self.settings.reduce_motion)
            .fade_out(!self.settings.reduce_motion)
            .open(&mut self.calculator_open)
            .resizable(false)
            .default_width(340.0)
            .show(ctx, |ui| {
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
        let metrics = if self.monitor_open {
            if let Some(monitor) = self.monitor.as_mut() {
                if self.last_monitor_refresh.elapsed() >= Duration::from_secs(1) {
                    monitor.refresh_all();
                    self.last_monitor_refresh = Instant::now();
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

        egui::Window::new("NOVA System Monitor")
            .fade_in(!self.settings.reduce_motion)
            .fade_out(!self.settings.reduce_motion)
            .open(&mut self.monitor_open)
            .default_size([430.0, 300.0])
            .show(ctx, |ui| {
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
            ctx.request_repaint_after(Duration::from_millis(200));
        }
        self.apply_theme(&ctx);

        egui::Frame::new().fill(if self.settings.theme == "light" {
            Color32::from_rgb(232, 237, 246)
        } else {
            Color32::from_rgb(10, 16, 29)
        }).inner_margin(egui::Margin::same(10)).show(ui, |ui| {
            ui.vertical(|ui| {
                // Compact system bar
                egui::Frame::new()
                    .fill(if self.settings.theme == "light" { Color32::from_rgb(249, 251, 255) } else { Color32::from_rgb(19, 27, 44) })
                    .corner_radius(egui::CornerRadius::same(14))
                    .inner_margin(egui::Margin::symmetric(14, 9))
                    .show(ui, |ui| {
                        ui.horizontal_centered(|ui| {
                            if ui.button(RichText::new("✦  NOVA").size(18.0).strong().color(Color32::from_rgb(133, 164, 255))).clicked() {
                                self.launcher_open = !self.launcher_open;
                            }
                            ui.separator();
                            ui.label(RichText::new("DESKTOP WORKSPACE").small().weak().strong());
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.label(RichText::new("NOVA 0.5.0").small().weak());
                                ui.separator();
                                let mode = if self.paths.portable { "● USB PORTABLE" } else { "● LOCAL MODE" };
                                ui.label(RichText::new(mode).small().color(Color32::from_rgb(105, 207, 170)));
                            });
                        });
                    });

                ui.add_space(9.0);
                let content_height = (ui.available_height() - 64.0).max(310.0);
                ui.horizontal(|ui| {
                    // App navigation rail
                    egui::Frame::new()
                        .fill(if self.settings.theme == "light" { Color32::from_rgb(244, 247, 252) } else { Color32::from_rgb(15, 23, 39) })
                        .corner_radius(egui::CornerRadius::same(15))
                        .inner_margin(egui::Margin::same(12))
                        .show(ui, |ui| {
                            ui.set_min_width(156.0);
                            ui.set_min_height(content_height);
                            ui.label(RichText::new("WORKSPACE").small().weak().strong());
                            ui.add_space(10.0);
                            self.sidebar_button(ui, "▦", "Files");
                            self.sidebar_button(ui, "⌘", "Terminal");
                            self.sidebar_button(ui, "✎", "Editor");
                            self.sidebar_button(ui, "∑", "Calculator");
                            self.sidebar_button(ui, "◉", "Monitor");
                            self.sidebar_button(ui, "▤", "Trash");
                            self.sidebar_button(ui, "🌐", "Google Chrome");
                            self.sidebar_button(ui, "文", "UniKey");
                            self.sidebar_button(ui, "🛡", "Security Center");
                            ui.add_space(16.0);
                            ui.separator();
                            self.sidebar_button(ui, "⚙", "Settings");
                            ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                                ui.add_space(8.0);
                                ui.label(RichText::new("Portable by design").small().weak());
                                ui.label(RichText::new("Windows 10 / 11 · x64").small().weak());
                            });
                        });
                    ui.add_space(9.0);

                    // Desktop canvas
                    let desktop_width = ui.available_width().max(420.0);
                    egui::Frame::new()
                        .fill(if self.settings.theme == "light" { Color32::from_rgb(222, 231, 245) } else { Color32::from_rgb(12, 22, 42) })
                        .corner_radius(egui::CornerRadius::same(18))
                        .inner_margin(egui::Margin::same(24))
                        .show(ui, |ui| {
                            ui.set_min_size(egui::vec2(desktop_width, content_height));
                            let rect = ui.max_rect();
                            if !self.settings.reduce_motion {
                                ui.painter().circle_filled(
                                    rect.right_top() + egui::vec2(-150.0, 155.0),
                                    250.0,
                                    Color32::from_rgba_unmultiplied(66, 103, 190, 28),
                                );
                                ui.painter().circle_filled(
                                    rect.left_bottom() + egui::vec2(120.0, -70.0),
                                    190.0,
                                    Color32::from_rgba_unmultiplied(40, 159, 153, 22),
                                );
                            }
                            ui.vertical(|ui| {
                                ui.label(RichText::new("Your workspace, ready to go.").size(28.0).strong().color(if self.settings.theme == "light" { Color32::from_rgb(25, 39, 64) } else { Color32::WHITE }));
                                ui.label(RichText::new("A portable desktop for files, tools and everyday work.").size(14.0).color(if self.settings.theme == "light" { Color32::from_rgb(75, 91, 118) } else { Color32::from_rgb(186, 199, 222) }));
                                ui.add_space(22.0);
                                ui.label(RichText::new("PINNED APPS").small().strong().weak());
                                ui.add_space(8.0);
                                ui.horizontal_wrapped(|ui| {
                                    self.desktop_tile(ui, "▦", "Files", "Browse your workspace");
                                    self.desktop_tile(ui, "⌘", "Terminal", "CMD and Wi-Fi tools");
                                    self.desktop_tile(ui, "✎", "Editor", "Write portable notes");
                                    self.desktop_tile(ui, "∑", "Calculator", "Quick calculations");
                                    self.desktop_tile(ui, "◉", "Monitor", "CPU and memory");
                                    self.desktop_tile(ui, "▤", "Trash", "Restore deleted items");
                                    self.desktop_tile(ui, "🌐", "Google Chrome", "Open normal or isolated browser");
                                    self.desktop_tile(ui, "文", "UniKey", "Vietnamese Unicode typing");
                                    self.desktop_tile(ui, "🛡", "Security Center", "Windows Sandbox protection");
                                    self.desktop_tile(ui, "⚙", "Settings", "Personalize NOVA");
                                });
                                ui.add_space(18.0);
                                egui::Frame::new()
                                    .fill(Color32::from_rgba_unmultiplied(255, 255, 255, if self.settings.theme == "light" { 180 } else { 12 }))
                                    .corner_radius(egui::CornerRadius::same(12))
                                    .inner_margin(egui::Margin::same(12))
                                    .show(ui, |ui| {
                                        ui.label(RichText::new(&self.status_message).small().color(if self.settings.theme == "light" { Color32::from_rgb(63, 78, 102) } else { Color32::from_rgb(180, 197, 222) }));
                                    });
                            });
                        });
                });

                ui.add_space(9.0);
                // Bottom dock/taskbar. Active apps are highlighted; click again to hide.
                egui::Frame::new()
                    .fill(if self.settings.theme == "light" { Color32::from_rgb(249, 251, 255) } else { Color32::from_rgb(19, 27, 44) })
                    .corner_radius(egui::CornerRadius::same(14))
                    .inner_margin(egui::Margin::symmetric(12, 7))
                    .show(ui, |ui| {
                        ui.horizontal_centered(|ui| {
                            let apps = [
                                ("▦", "Files", self.files_open),
                                ("⌘", "Terminal", self.terminal_open),
                                ("✎", "Editor", self.editor_open),
                                ("∑", "Calculator", self.calculator_open),
                                ("◉", "Monitor", self.monitor_open),
                                ("▤", "Trash", self.trash_open),
                                ("🌐", "Google Chrome", self.chrome_open),
                                ("文", "UniKey", self.unikey_open),
                                ("🛡", "Security Center", self.security_open),
                                ("⚙", "Settings", self.settings_open),
                            ];
                            for (glyph, name, active) in apps {
                                let label = if active { format!("● {glyph} {name}") } else { format!("{glyph} {name}") };
                                if ui.button(label).clicked() { self.toggle_app(name); }
                            }
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.label(RichText::new("● READY").small().color(Color32::from_rgb(105, 207, 170)));
                            });
                        });
                    });
            });
        });

        self.launcher_window(&ctx);
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
        // Keep periodic repaints only while the live monitor is visible.
        // egui animations and user input schedule their own frames otherwise.
        if self.monitor_open {
            ctx.request_repaint_after(Duration::from_millis(250));
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
    use super::format_number;

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
