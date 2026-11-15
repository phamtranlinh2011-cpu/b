use crate::storage::{canonical_user_path, create_user_directories, safe_user_path, AppPaths, Settings};
use crate::terminal::{TerminalState, TerminalTask};
use eframe::egui::{self, Color32, FontId, RichText, TextEdit};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};
use sysinfo::System;

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
        visuals.selection.bg_fill = Color32::from_rgb(77, 105, 190);
        ctx.set_visuals(visuals);
        ctx.all_styles_mut(|style| {
            style.spacing.item_spacing = egui::vec2(10.0, 8.0);
            style.spacing.button_padding = egui::vec2(12.0, 8.0);
            style.visuals.window_corner_radius = egui::CornerRadius::same(14);
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
            _ => {}
        }
        self.launcher_open = false;
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
            .fill(Color32::from_rgba_unmultiplied(255, 255, 255, 14))
            .stroke(egui::Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 24)))
            .corner_radius(egui::CornerRadius::same(12))
            .inner_margin(egui::Margin::same(13))
            .show(ui, |ui| {
                ui.set_min_size(egui::vec2(170.0, 104.0));
                ui.vertical(|ui| {
                    ui.label(RichText::new(glyph).size(25.0).color(Color32::from_rgb(151, 179, 255)));
                    ui.label(RichText::new(app).strong().color(Color32::WHITE));
                    ui.label(RichText::new(description).small().color(Color32::from_rgb(180, 194, 217)));
                });
            });
        if ui.interact(button.response.rect, ui.scope_id().with(app), egui::Sense::click()).clicked() {
            self.open_app(app);
        }
        ui.add_space(8.0);
    }

    fn launcher_window(&mut self, ctx: &egui::Context) {
        let mut selected_app: Option<&'static str> = None;
        egui::Window::new("NOVA Launcher")
            .open(&mut self.launcher_open)
            .resizable(false)
            .default_width(300.0)
            .show(ctx, |ui| {
                ui.label(RichText::new("Launch an application").weak());
                ui.separator();
                for app in ["Files", "Terminal", "Editor", "Calculator", "Monitor", "Settings"] {
                    if ui.button(app).clicked() {
                        selected_app = Some(app);
                    }
                }
            });
        if let Some(app) = selected_app {
            self.open_app(app);
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
        egui::Window::new("NOVA Terminal — Windows CMD")
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
                ui.label(RichText::new("CMD runs with your Windows account permissions. This is not a sandbox.").small().weak());
            });
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
        egui::Window::new("NOVA Files")
            .open(&mut self.files_open)
            .default_size([650.0, 460.0])
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if ui.button("↑ Up").clicked() && self.files_dir != self.paths.user_dir {
                        navigate_to = self.files_dir.parent().map(Path::to_path_buf);
                    }
                    ui.label(RichText::new(self.files_dir.strip_prefix(&self.paths.user_dir).unwrap_or(Path::new(".")).display().to_string()).weak());
                });
                ui.separator();
                ui.horizontal(|ui| {
                    ui.add(TextEdit::singleline(&mut self.files_new_folder).hint_text("New folder name").desired_width(220.0));
                    if ui.button("Create folder").clicked() {
                        create_folder = true;
                    }
                });
                ui.separator();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    let safe_current = canonical_user_path(&self.paths.user_dir, &self.files_dir);
                    match safe_current.and_then(|folder| fs::read_dir(folder)) {
                        Ok(entries) => {
                            let mut rows = entries.filter_map(Result::ok).collect::<Vec<_>>();
                            rows.sort_by_key(|entry| (!entry.file_type().map(|t| t.is_dir()).unwrap_or(false), entry.file_name()));
                            for entry in rows {
                                let Ok(kind) = entry.file_type() else { continue };
                                if kind.is_symlink() { continue; }
                                // Canonicalize each entry as a second defense against
                                // Windows junctions/reparse points escaping the user root.
                                if canonical_user_path(&self.paths.user_dir, &entry.path()).is_err() { continue; }
                                let name = entry.file_name().to_string_lossy().to_string();
                                let is_dir = kind.is_dir();
                                let icon = if is_dir { "▰" } else { "▤" };
                                if ui.selectable_label(false, format!("{icon}  {name}")).clicked() {
                                    if is_dir {
                                        navigate_to = Some(entry.path());
                                    } else {
                                        open_file = Some(entry.path());
                                    }
                                }
                            }
                        }
                        Err(error) => {
                            ui.colored_label(Color32::LIGHT_RED, format!("Cannot read folder: {error}"));
                        }
                    }
                });
                ui.separator();
                ui.label(RichText::new("Files are restricted to NOVA's Data/Users/Default folder. Select a file to open it in Editor.").small().weak());
            });

        if let Some(path) = navigate_to {
            match canonical_user_path(&self.paths.user_dir, &path) {
                Ok(folder) if folder.is_dir() => self.files_dir = folder,
                Ok(_) => self.status_message = "That item is not a folder.".to_owned(),
                Err(error) => self.status_message = format!("Navigation blocked: {error}"),
            }
        }
        if create_folder {
            let name = self.files_new_folder.trim().to_owned();
            if !name.is_empty() && !name.contains('/') && !name.contains('\\') {
                let validated_name = safe_user_path(&self.paths.user_dir, &name);
                let safe_parent = canonical_user_path(&self.paths.user_dir, &self.files_dir);
                match (validated_name, safe_parent) {
                    (Ok(_), Ok(parent)) => match fs::create_dir(parent.join(&name)) {
                        Ok(()) => {
                            self.files_new_folder.clear();
                            self.status_message = format!("Created folder: {name}");
                        }
                        Err(error) => self.status_message = format!("Create folder failed: {error}"),
                    },
                    (Err(error), _) | (_, Err(error)) => {
                        self.status_message = format!("Create folder blocked: {error}")
                    }
                }
            }
        }
        if let Some(path) = open_file {
            self.open_file_in_editor(path);
        }
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
            Ok(meta) if meta.len() <= 2 * 1024 * 1024 => match fs::read_to_string(&canonical_file) {
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
        egui::Window::new("NOVA Text Editor")
            .open(&mut self.editor_open)
            .default_size([700.0, 520.0])
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Path (inside NOVA user folder):");
                    ui.add(TextEdit::singleline(&mut self.editor_path).desired_width(330.0));
                    if ui.button("Save").clicked() {
                        save = true;
                    }
                });
                ui.add(
                    TextEdit::multiline(&mut self.editor_text)
                        .font(FontId::monospace(13.0))
                        .desired_rows(20)
                        .desired_width(f32::INFINITY),
                );
                ui.label(RichText::new(&self.editor_status).small().weak());
            });
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
            .open(&mut self.settings_open)
            .resizable(false)
            .default_width(400.0)
            .show(ctx, |ui| {
                ui.heading("Appearance");
                ui.horizontal(|ui| {
                    changed |= ui.selectable_value(&mut self.settings.theme, "dark".to_owned(), "Dark").changed();
                    changed |= ui.selectable_value(&mut self.settings.theme, "light".to_owned(), "Light").changed();
                });
                ui.separator();
                changed |= ui.checkbox(&mut self.settings.reduce_motion, "Reduce visual effects").changed();
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

    fn calculator_window(&mut self, ctx: &egui::Context) {
        let mut calculate = false;
        egui::Window::new("NOVA Calculator")
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
                if ui.button("Calculate").clicked() {
                    calculate = true;
                }
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
        if !self.monitor_open {
            return;
        }

        let metrics = if let Some(monitor) = self.monitor.as_mut() {
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
        };
        let monitor_error = self.monitor_error.clone();

        egui::Window::new("NOVA System Monitor")
            .open(&mut self.monitor_open)
            .default_size([430.0, 300.0])
            .show(ctx, |ui| {
                if let Some((cpu, used, total, processes)) = metrics {
                    ui.label(format!("CPU usage: {cpu:.1}%"));
                    ui.add(egui::ProgressBar::new((cpu / 100.0).clamp(0.0, 1.0)).show_percentage());
                    ui.add_space(8.0);
                    ui.label(format!("Memory: {used:.2} / {total:.2} GB"));
                    ui.add(egui::ProgressBar::new(if total > 0.0 { (used / total) as f32 } else { 0.0 }).show_percentage());
                    ui.add_space(8.0);
                    ui.label(format!("Processes: {processes}"));
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
        let ctx = ui.ctx().clone();
        self.apply_theme(&ctx);

        ui.vertical(|ui| {
            // Top bar
            ui.horizontal_centered(|ui| {
                if ui.button(RichText::new("✦ NOVA").size(18.0).strong().color(Color32::from_rgb(133, 164, 255))).clicked() {
                    self.launcher_open = !self.launcher_open;
                }
                ui.separator();
                ui.label(RichText::new("NATIVE DESKTOP").small().weak());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new("NOVA 0.3.5").small().weak());
                    ui.separator();
                    let mode = if self.paths.portable { "USB PORTABLE" } else { "LOCAL MODE" };
                    ui.label(RichText::new(mode).small().color(Color32::from_rgb(105, 207, 170)));
                });
            });
            ui.separator();

            let content_height = (ui.available_height() - 78.0).max(300.0);
            ui.horizontal(|ui| {
                // Sidebar
                ui.allocate_ui_with_layout(
                    egui::vec2(184.0, content_height),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.label(RichText::new("WORKSPACE").small().weak().strong());
                        ui.add_space(8.0);
                        self.sidebar_button(ui, "▦", "Files");
                        self.sidebar_button(ui, "⌘", "Terminal");
                        self.sidebar_button(ui, "✎", "Editor");
                        self.sidebar_button(ui, "∑", "Calculator");
                        self.sidebar_button(ui, "◉", "Monitor");
                        ui.add_space(18.0);
                        ui.separator();
                        self.sidebar_button(ui, "⚙", "Settings");
                        ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                            ui.add_space(12.0);
                            ui.label(RichText::new("Portable by design").small().weak());
                            ui.label(RichText::new("Windows 10 / 11 · x64").small().weak());
                        });
                    },
                );
                ui.separator();

                // Desktop area
                let desktop_width = ui.available_width().max(420.0);
                ui.allocate_ui_with_layout(
                    egui::vec2(desktop_width, content_height),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        let rect = ui.max_rect();
                        let painter = ui.painter();
                        painter.rect_filled(rect, egui::CornerRadius::ZERO, Color32::from_rgb(12, 22, 43));
                        if !self.settings.reduce_motion {
                            painter.circle_filled(
                                rect.right_top() + egui::vec2(-170.0, 180.0),
                                260.0,
                                Color32::from_rgba_unmultiplied(58, 93, 170, 34),
                            );
                            painter.circle_filled(
                                rect.left_bottom() + egui::vec2(190.0, -80.0),
                                210.0,
                                Color32::from_rgba_unmultiplied(42, 159, 153, 25),
                            );
                        }
                        ui.vertical(|ui| {
                            ui.add_space(30.0);
                            ui.label(RichText::new("A workspace that travels with you.").size(27.0).strong().color(Color32::WHITE));
                            ui.label(RichText::new("Your files, tools and preferences stay on this USB drive.").size(14.0).color(Color32::from_rgb(186, 199, 222)));
                            ui.add_space(26.0);
                            ui.horizontal_wrapped(|ui| {
                                self.desktop_tile(ui, "▦", "Files", "Browse your workspace");
                                self.desktop_tile(ui, "⌘", "Terminal", "Windows command line + Wi-Fi");
                                self.desktop_tile(ui, "✎", "Editor", "Edit portable notes");
                                self.desktop_tile(ui, "∑", "Calculator", "Quick calculations");
                                self.desktop_tile(ui, "◉", "Monitor", "CPU, memory and processes");
                                self.desktop_tile(ui, "⚙", "Settings", "Personalize NOVA");
                            });
                            ui.add_space(20.0);
                            ui.label(RichText::new(&self.status_message).small().color(Color32::from_rgb(168, 188, 215)));
                        });
                    },
                );
            });

            ui.separator();
            // Taskbar
            ui.horizontal_centered(|ui| {
                let apps = [
                    ("▦", "Files", self.files_open),
                    ("⌘", "Terminal", self.terminal_open),
                    ("✎", "Editor", self.editor_open),
                    ("∑", "Calculator", self.calculator_open),
                    ("◉", "Monitor", self.monitor_open),
                    ("⚙", "Settings", self.settings_open),
                ];
                for (glyph, name, active) in apps {
                    let label = if active { format!("● {glyph} {name}") } else { format!("{glyph} {name}") };
                    if ui.button(label).clicked() {
                        self.open_app(name);
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new("● READY").small().color(Color32::from_rgb(105, 207, 170)));
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
        ctx.request_repaint_after(Duration::from_millis(500));
    }
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
