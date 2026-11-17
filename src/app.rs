use crate::storage::{atomic_write_user_file, canonical_user_path, create_user_directories, is_link_or_reparse_point, safe_user_path, AppPaths, Settings};
use crate::terminal::{TerminalState, TerminalTask};
use crate::trash::{self, TrashEntry};
use crate::apps;
use crate::calculator::{self, AngleUnit, EvalContext};
use crate::{isolation, unikey, unicode, hardware::{self, HardwareTelemetry, HardwareSnapshot, HostControl}, security_tools::{self, SecurityTool}};
use eframe::egui::{self, Color32, FontId, RichText, TextEdit};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};
use std::process::Child;
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum UniKeyProcessKind { Running }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SnapSide { Left, Right, Maximize, Restore }

#[derive(Clone)]
enum PendingEditorAction {
    NewDocument,
    OpenFile(PathBuf),
    Close,
    ExitApplication,
}

#[derive(Clone, Copy)]
enum EditorDecision { Save, Discard, Cancel }

#[derive(Clone)]
struct ProcessMetric {
    pid: String,
    name: String,
    cpu_percent: f32,
    memory_bytes: u64,
}

#[derive(Clone)]
struct CpuCoreMetric {
    name: String,
    usage_percent: f32,
    frequency_mhz: u64,
}

#[derive(Clone, Copy)]
struct WindowMotion {
    from_pos: egui::Pos2,
    from_size: egui::Vec2,
    to_pos: egui::Pos2,
    to_size: egui::Vec2,
    started: Instant,
}

/// Single source of truth for desktop shortcuts, Start pins and the All apps view.
#[derive(Clone)]
struct CachedFileEntry {
    path: PathBuf,
    name: String,
    is_dir: bool,
    display_size: String,
}

enum FilesWorkerResult {
    Operation { message: String, clear_clipboard: bool, retry_sources: Vec<PathBuf> },
    Search {
        query: String,
        result: Result<file_ops::SearchResult, String>,
    },
}

enum NetworkWorkerResult {
    Scan(Result<Vec<String>, String>),
    Connect(Result<String, String>),
    Disconnect(Result<String, String>),
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
    StartApp { name: "Calculator", icon: "∑", description: "Scientific calculator and Casio-style calculation modes", category: "Productivity" },
    StartApp { name: "Monitor", icon: "◉", description: "CPU, RAM, processes, storage, network, GPU and power", category: "System" },
    StartApp { name: "Trash", icon: "▤", description: "Restore deleted NOVA files", category: "System" },
    StartApp { name: "Google Chrome", icon: "🌐", description: "Open the real Google Chrome Windows app", category: "Internet" },
    StartApp { name: "UniKey", icon: "文", description: "Bundled Vietnamese Telex and VNI input", category: "Productivity" },
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

const DESKTOP_SHORTCUTS: [&str; 9] = [
    "Files", "Terminal", "Editor", "Calculator", "Google Chrome", "UniKey",
    "Trash", "Security Center", "Settings",
];

/// Compute balanced taskbar rails without ever allocating more than the viewport.
fn taskbar_rail_widths(available_width: f32) -> (f32, f32, f32, bool, bool) {
    let available = available_width.max(0.0);
    let ultra_compact = available < 440.0;
    let medium = available < 720.0;
    // Equal rails keep the middle app group geometrically centered.
    let side_width = if ultra_compact { 38.0 } else if medium { 136.0 } else { 170.0 };
    let desired_left = side_width;
    let desired_right = side_width;
    let scale = ((available - 72.0).max(0.0) / (desired_left + desired_right)).clamp(0.0, 1.0);
    let left = desired_left * scale;
    let right = desired_right * scale;
    let center = (available - left - right).max(0.0);
    (left, center, right, ultra_compact, medium)
}

/// Compute the Start surface from the actual viewport, including small windows.
fn start_menu_dimensions(screen_width: f32, screen_height: f32) -> (f32, f32, usize) {
    let width = screen_width.max(1.0);
    let height = screen_height.max(1.0);
    let menu_width = (width - 24.0).clamp(1.0, 680.0).min(width);
    let scroll_height = (height - 240.0).clamp(80.0, 330.0).min((height - 16.0).max(1.0));
    let menu_margin = ((menu_width - 1.0) * 0.5).clamp(0.0, 19.0);
    let inner_width = (menu_width - menu_margin * 2.0).max(1.0);
    let pinned_columns = ((inner_width / 96.0).floor() as usize).clamp(1, 6);
    (menu_width, scroll_height, pinned_columns)
}

/// Keep long filenames readable in the compact List view without pushing the size column off-screen.
fn normalize_pinned_apps(pins: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    pins.iter()
        .filter(|name| START_APPS.iter().any(|app| app.name == name.as_str()))
        .filter(|name| seen.insert(name.as_str()))
        .cloned()
        .collect()
}

fn normalize_desktop_positions(positions: &BTreeMap<String, [f32; 2]>) -> BTreeMap<String, [f32; 2]> {
    positions.iter()
        .filter(|(name, point)| DESKTOP_SHORTCUTS.contains(&name.as_str())
            && point[0].is_finite() && point[1].is_finite())
        .map(|(name, point)| (name.clone(), [point[0].clamp(0.0, 1.0), point[1].clamp(0.0, 1.0)]))
        .collect()
}

fn workspace_is_active(active_workspace: Option<&str>, app_name: &str, is_open: bool) -> bool {
    is_open && active_workspace == Some(app_name)
}

fn move_pinned_app(pins: &mut [String], name: &str, direction: isize) -> bool {
    let Some(index) = pins.iter().position(|item| item == name) else { return false; };
    let target = index as isize + direction;
    if target < 0 || target >= pins.len() as isize { return false; }
    pins.swap(index, target as usize);
    true
}

fn desktop_default_position(index: usize, rect: egui::Rect, columns: usize) -> egui::Pos2 {
    let columns = columns.max(1);
    let col = index % columns;
    let row = index / columns;
    egui::pos2(rect.left() + 14.0 + col as f32 * 102.0, rect.top() + 66.0 + row as f32 * 94.0)
}

fn desktop_layout_changed(previous: Option<egui::Vec2>, current: egui::Vec2) -> bool {
    previous.is_some_and(|previous| {
        (previous.x - current.x).abs() > 1.0 || (previous.y - current.y).abs() > 1.0
    })
}

fn desktop_shortcut_travel(rect: egui::Rect) -> (f32, f32) {
    // Desktop shortcut Areas are 94x88 px, constrained inside a 4 px inset.
    // Their normalized positions start at (12, 58), so subtract both offsets
    // and the actual Area size from the available desktop dimensions.
    ((rect.width() - 110.0).max(1.0), (rect.height() - 150.0).max(1.0))
}

fn app_icon_colors(name: &str) -> (Color32, Color32) {
    let accent = match name {
        "Files" => Color32::from_rgb(62, 151, 238),
        "Terminal" => Color32::from_rgb(57, 185, 145),
        "Editor" => Color32::from_rgb(111, 143, 231),
        "Calculator" => Color32::from_rgb(112, 126, 157),
        "Monitor" => Color32::from_rgb(40, 181, 190),
        "Trash" => Color32::from_rgb(137, 151, 174),
        "Google Chrome" => Color32::from_rgb(231, 111, 81),
        "UniKey" => Color32::from_rgb(171, 125, 224),
        "Security Center" => Color32::from_rgb(61, 177, 129),
        "Network Discovery" => Color32::from_rgb(70, 158, 218),
        "Vulnerability Management" => Color32::from_rgb(219, 160, 66),
        "Web Security" => Color32::from_rgb(201, 111, 143),
        "Packet Analyzer" => Color32::from_rgb(93, 132, 203),
        "Web / API Inspector" => Color32::from_rgb(147, 126, 205),
        _ => Color32::from_rgb(92, 139, 245),
    };
    let background = Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 34);
    (accent, background)
}

/// Draw a small application icon using egui vector primitives, avoiding emoji/font fallback differences.
fn paint_vector_app_icon(painter: &egui::Painter, center: egui::Pos2, name: &str, size: f32, dark: bool) {
    let (accent, background) = app_icon_colors(name);
    let rect = egui::Rect::from_center_size(center, egui::vec2(size, size));
    let radius = (size * 0.23).clamp(4.0, 12.0) as u8;
    painter.rect_filled(rect, egui::CornerRadius::same(radius), background);
    painter.rect_stroke(rect, egui::CornerRadius::same(radius), egui::Stroke::new(1.0, accent.gamma_multiply(0.55)), egui::StrokeKind::Inside);
    let unit = size / 32.0;
    let point = |x: f32, y: f32| egui::pos2(rect.left() + x * unit, rect.top() + y * unit);
    let stroke = egui::Stroke::new((size * 0.055).clamp(1.2, 2.3), if dark { Color32::from_rgb(239, 245, 255) } else { Color32::from_rgb(36, 50, 72) });
    let line = |a: (f32, f32), b: (f32, f32)| painter.line_segment([point(a.0, a.1), point(b.0, b.1)], stroke);
    match name {
        "Files" => {
            let pts = vec![point(5.0, 11.0), point(13.0, 11.0), point(16.0, 14.0), point(27.0, 14.0), point(25.0, 24.0), point(6.0, 24.0)];
            painter.add(egui::Shape::convex_polygon(pts, accent, stroke));
            painter.line_segment([point(5.5, 12.5), point(26.0, 15.0)], egui::Stroke::new(1.0, Color32::from_white_alpha(150)));
        }
        "Terminal" => {
            let inner = egui::Rect::from_min_max(point(5.0, 7.0), point(27.0, 25.0));
            painter.rect_stroke(inner, egui::CornerRadius::same(4), stroke, egui::StrokeKind::Inside);
            line((9.0, 12.0), (13.0, 16.0)); line((13.0, 16.0), (9.0, 20.0)); line((16.0, 20.0), (22.0, 20.0));
        }
        "Editor" => {
            let page = egui::Rect::from_min_max(point(8.0, 5.0), point(24.0, 27.0));
            painter.rect_stroke(page, egui::CornerRadius::same(2), stroke, egui::StrokeKind::Inside);
            line((19.0, 5.5), (19.0, 11.0)); line((19.0, 11.0), (23.5, 11.0));
            line((11.0, 15.0), (21.0, 15.0)); line((11.0, 19.0), (21.0, 19.0)); line((11.0, 23.0), (18.0, 23.0));
        }
        "Calculator" => {
            let body = egui::Rect::from_min_max(point(8.0, 4.5), point(24.0, 27.5));
            painter.rect_stroke(body, egui::CornerRadius::same(3), stroke, egui::StrokeKind::Inside);
            painter.rect_filled(egui::Rect::from_min_max(point(11.0, 8.0), point(21.0, 12.0)), egui::CornerRadius::same(1), accent);
            for (x, y) in [(12.0, 16.0), (19.0, 16.0), (12.0, 22.0), (19.0, 22.0)] { painter.circle_filled(point(x, y), unit * 1.2, accent); }
        }
        "Monitor" => {
            let screen = egui::Rect::from_min_max(point(4.5, 7.0), point(27.5, 22.0));
            painter.rect_stroke(screen, egui::CornerRadius::same(3), stroke, egui::StrokeKind::Inside);
            line((16.0, 22.0), (16.0, 26.0)); line((11.0, 27.0), (21.0, 27.0));
            line((8.0, 18.0), (13.0, 13.0)); line((13.0, 13.0), (17.0, 16.0)); line((17.0, 16.0), (23.0, 10.0));
        }
        "Trash" => {
            line((8.0, 9.0), (24.0, 9.0)); line((12.0, 6.0), (20.0, 6.0));
            let bin = egui::Rect::from_min_max(point(10.0, 11.0), point(22.0, 26.0));
            painter.rect_stroke(bin, egui::CornerRadius::same(2), stroke, egui::StrokeKind::Inside);
            line((14.0, 14.0), (14.0, 23.0)); line((18.0, 14.0), (18.0, 23.0));
        }
        "Google Chrome" => {
            painter.circle_stroke(center, size * 0.34, egui::Stroke::new((size * 0.10).clamp(2.0, 4.0), accent));
            painter.circle_stroke(center, size * 0.19, egui::Stroke::new((size * 0.08).clamp(1.8, 3.0), if dark { Color32::WHITE } else { Color32::from_rgb(35, 51, 75) }));
            painter.circle_filled(center, size * 0.105, Color32::from_rgb(74, 143, 223));
        }
        "UniKey" => {
            let keyboard = egui::Rect::from_min_max(point(4.0, 9.0), point(28.0, 23.0));
            painter.rect_stroke(keyboard, egui::CornerRadius::same(3), stroke, egui::StrokeKind::Inside);
            for y in [13.0, 17.0] { for x in [8.0, 13.0, 18.0, 23.0] { painter.circle_filled(point(x, y), unit * 0.85, accent); } }
            line((11.0, 20.0), (21.0, 20.0));
        }
        "Security Center" | "Network Discovery" | "Vulnerability Management" | "Web Security" | "Packet Analyzer" | "Web / API Inspector" => {
            if name == "Security Center" {
                let shield = vec![point(16.0, 4.5), point(26.0, 8.0), point(25.0, 19.0), point(16.0, 27.0), point(7.0, 19.0), point(6.0, 8.0)];
                painter.add(egui::Shape::convex_polygon(shield, background, stroke));
                line((11.0, 15.0), (15.0, 19.0)); line((15.0, 19.0), (22.0, 11.0));
            } else if name == "Network Discovery" {
                painter.circle_stroke(center, size * 0.31, stroke); painter.circle_stroke(center, size * 0.19, stroke);
                painter.line_segment(
                    [center - egui::vec2(size * 0.31, 0.0), center + egui::vec2(size * 0.31, 0.0)],
                    stroke,
                );
                painter.line_segment(
                    [center - egui::vec2(0.0, size * 0.31), center + egui::vec2(0.0, size * 0.31)],
                    stroke,
                );
                painter.circle_filled(point(22.0, 10.0), unit * 2.0, accent);
            } else if name == "Vulnerability Management" {
                for (i, h) in [(0, 9.0), (1, 15.0), (2, 21.0)] { let x = 8.0 + i as f32 * 7.0; painter.rect_filled(egui::Rect::from_min_max(point(x, 25.0-h), point(x+4.0, 25.0)), egui::CornerRadius::same(1), accent); }
                line((6.0, 25.5), (26.0, 25.5));
            } else if name == "Web Security" {
                line((12.0, 7.0), (7.0, 16.0)); line((7.0, 16.0), (12.0, 25.0));
                line((20.0, 7.0), (25.0, 16.0)); line((25.0, 16.0), (20.0, 25.0)); line((18.0, 6.0), (14.0, 26.0));
            } else if name == "Packet Analyzer" {
                line((5.0, 17.0), (9.0, 17.0)); line((9.0, 17.0), (12.0, 10.0)); line((12.0, 10.0), (16.0, 23.0)); line((16.0, 23.0), (20.0, 13.0)); line((20.0, 13.0), (23.0, 17.0)); line((23.0, 17.0), (27.0, 17.0));
            } else {
                line((7.0, 12.0), (12.0, 16.0)); line((12.0, 16.0), (7.0, 20.0)); line((25.0, 12.0), (20.0, 16.0)); line((20.0, 16.0), (25.0, 20.0)); line((14.0, 23.0), (18.0, 9.0));
            }
        }
        _ => {
            painter.circle_stroke(center, size * 0.28, stroke); painter.circle_filled(center, size * 0.10, accent);
            for angle in [0.0_f32, 1.57, 3.14, 4.71] {
                let direction = egui::vec2(angle.cos(), angle.sin());
                painter.line_segment([center + direction * size * 0.31, center + direction * size * 0.39], stroke);
            }
        }
    }
}

fn discard_unsaved_editor_text(current: &mut String, saved_snapshot: &str) {
    current.clear();
    current.push_str(saved_snapshot);
}

fn next_untitled_file_name(directory: &Path) -> std::io::Result<String> {
    for index in 1..=9_999_u32 {
        let name = if index == 1 {
            "Untitled.txt".to_owned()
        } else {
            format!("Untitled ({index}).txt")
        };
        match fs::symlink_metadata(directory.join(&name)) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(name),
            Ok(_) => continue,
            Err(error) => return Err(error),
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "Could not find an unused Untitled document name.",
    ))
}

fn compact_file_name(name: &str, max_chars: usize) -> String {
    if name.chars().count() <= max_chars {
        return name.to_owned();
    }
    if max_chars <= 1 {
        return "…".to_owned();
    }
    let prefix: String = name.chars().take(max_chars - 1).collect();
    format!("{prefix}…")
}

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
    unikey_open: bool,
    security_open: bool,
    security_tool_open: [bool; 5],
    security_tool_status: [String; 5],
    security_tool_installed: [bool; 5],
    security_tool_paths: [Option<PathBuf>; 5],
    security_tools_checked: bool,
    security_unsigned_zap_acknowledged: bool,
    security_launch_receiver: Option<Receiver<(usize, Result<PathBuf, String>)>>,
    security_launching: [bool; 5],
    sandbox_available: Option<bool>,
    security_scope: String,
    security_scope_confirmed: bool,
    greenbone_url: String,
    security_confirm_clear_quarantine: bool,
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
    files_multi_selected: HashSet<PathBuf>,
    files_clipboard: Vec<PathBuf>,
    files_clipboard_cut: bool,
    files_preview: Option<(PathBuf, String)>,
    files_rename_target: Option<PathBuf>,
    files_rename_input: String,
    files_recursive_search_active: bool,
    files_search_truncated: bool,
    files_worker_receiver: Option<Receiver<FilesWorkerResult>>,
    files_worker_busy: bool,
    network_worker_receiver: Option<Receiver<NetworkWorkerResult>>,
    network_busy: bool,
    network_networks: Vec<String>,
    network_selected_ssid: Option<String>,
    network_password: String,
    network_status: String,
    notification_open: bool,
    task_view_open: bool,
    activity_log: Vec<String>,
    last_activity_status: String,
    active_workspace: Option<String>,
    minimized_apps: HashSet<String>,
    window_restore_rects: HashMap<String, egui::Rect>,
    desktop_rect: Option<egui::Rect>,
    desktop_last_size: Option<egui::Vec2>,
    desktop_layout_generation: u64,
    settings_page: usize,
    window_snap_requests: HashMap<String, SnapSide>,
    window_motions: HashMap<String, WindowMotion>,
    window_rects: HashMap<String, egui::Rect>,
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
    files_dir: PathBuf,
    files_new_folder: String,
    editor_path: String,
    editor_text: String,
    editor_saved_text: String,
    editor_saved_path: Option<PathBuf>,
    editor_dirty: bool,
    editor_pending_action: Option<PendingEditorAction>,
    editor_overwrite_pending: Option<PathBuf>,
    exit_after_editor: bool,
    editor_status: String,
    editor_find_open: bool,
    editor_find_query: String,
    editor_replace_text: String,
    calc_mode: String,
    calc_expression: String,
    calc_result: String,
    calc_angle: AngleUnit,
    calc_answer: f64,
    calc_memory: f64,
    calc_memory_present: bool,
    calc_history: Vec<String>,
    calc_stats_data: String,
    calc_stats_y: String,
    calc_equation_a: String,
    calc_equation_b: String,
    calc_equation_c: String,
    calc_table_start: String,
    calc_table_end: String,
    calc_table_step: String,
    calc_calculus_x: String,
    calc_integral_start: String,
    calc_integral_end: String,
    calc_integral_steps: String,
    calc_inequality_operator: String,
    calc_base_input: String,
    calc_base_from: u32,
    calc_matrix_a: String,
    calc_matrix_b: String,
    calc_vector_a: String,
    calc_vector_b: String,
    calc_ratio_a: String,
    calc_ratio_b: String,
    calc_ratio_c: String,
    monitor: Option<System>,
    hardware: Option<HardwareTelemetry>,
    hardware_snapshot: HardwareSnapshot,
    monitor_tab: usize,
    monitor_process_rows: Vec<ProcessMetric>,
    monitor_cpu_baseline_captured: bool,
    monitor_cpu_sample_ready: bool,
    monitor_process_baseline_captured: bool,
    monitor_process_cpu_ready: bool,
    monitor_error: Option<String>,
    last_monitor_refresh: Instant,
    last_process_refresh: Instant,
    last_cpu_frequency_refresh: Instant,
    applied_theme: String,
    applied_effective_theme: String,
    applied_accent: String,
    status_message: String,
}

fn replace_all_bounded(
    original: &str,
    needle: &str,
    replacement: &str,
    max_bytes: usize,
) -> Result<(String, usize), String> {
    if needle.is_empty() {
        return Ok((original.to_owned(), 0));
    }
    let matches = original.matches(needle).count();
    if matches == 0 {
        return Ok((original.to_owned(), 0));
    }
    let removed = needle.len().checked_mul(matches)
        .ok_or_else(|| "the replacement size could not be calculated safely".to_owned())?;
    let added = replacement.len().checked_mul(matches)
        .ok_or_else(|| "the replacement size could not be calculated safely".to_owned())?;
    let result_len = original.len().checked_sub(removed)
        .and_then(|base| base.checked_add(added))
        .ok_or_else(|| "the replacement size could not be calculated safely".to_owned())?;
    if result_len > max_bytes {
        return Err(format!("the result would exceed the {max_bytes} byte limit"));
    }
    Ok((original.replace(needle, replacement), matches))
}

fn top_level_selected_paths(mut paths: Vec<PathBuf>) -> Vec<PathBuf> {
    paths.sort_by_key(|path| (path.components().count(), path.clone()));
    let mut roots: Vec<PathBuf> = Vec::new();
    for path in paths {
        if roots.iter().any(|parent| path.starts_with(parent)) { continue; }
        roots.push(path);
    }
    roots
}

impl NovaApp {
    pub fn new(cc: &eframe::CreationContext<'_>, paths: AppPaths) -> Self {
        let (mut settings, mut settings_warning) = match paths.load_settings() {
            Ok(settings) => (settings, None),
            Err(error) => (Settings::default(), Some(format!("Settings could not be loaded; defaults are active: {error}"))),
        };
        // Settings are user-controlled data: accept only catalog app names and
        // remove duplicates before Start/Taskbar code consumes the list.
        let original_pins = settings.pinned_apps.clone();
        settings.pinned_apps = normalize_pinned_apps(&settings.pinned_apps);
        if settings.pinned_apps != original_pins {
            if let Err(error) = paths.save_settings(&settings) {
                let warning = format!("Invalid Start pins were ignored, but normalized settings could not be saved: {error}");
                settings_warning = Some(settings_warning.map_or(warning.clone(), |existing| format!("{existing} · {warning}")));
            }
        }
        let original_desktop_positions = settings.desktop_icon_positions.clone();
        settings.desktop_icon_positions = normalize_desktop_positions(&settings.desktop_icon_positions);
        if settings.desktop_icon_positions != original_desktop_positions {
            if let Err(error) = paths.save_settings(&settings) {
                let warning = format!("Invalid desktop icon positions were reset in memory but could not be saved: {error}");
                settings_warning = Some(settings_warning.map_or(warning.clone(), |existing| format!("{existing} · {warning}")));
            }
        }
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
            unikey_open: false,
            security_open: false,
            security_tool_open: [false; 5],
            security_tool_status: std::array::from_fn(|_| "Ready. Tool is not started.".to_owned()),
            security_tool_installed: [false; 5],
            security_tool_paths: std::array::from_fn(|_| None),
            security_tools_checked: false,
            security_unsigned_zap_acknowledged: false,
            security_launch_receiver: None,
            security_launching: [false; 5],
            sandbox_available: None,
            security_scope: String::new(),
            security_scope_confirmed: false,
            greenbone_url: "https://127.0.0.1:9392".to_owned(),
            security_confirm_clear_quarantine: false,
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
            files_multi_selected: HashSet::new(),
            files_clipboard: Vec::new(),
            files_clipboard_cut: false,
            files_preview: None,
            files_rename_target: None,
            files_rename_input: String::new(),
            files_recursive_search_active: false,
            files_search_truncated: false,
            files_worker_receiver: None,
            files_worker_busy: false,
            network_worker_receiver: None,
            network_busy: false,
            network_networks: Vec::new(),
            network_selected_ssid: None,
            network_password: String::new(),
            network_status: "Choose Scan to list nearby Wi-Fi networks.".to_owned(),
            notification_open: false,
            task_view_open: false,
            activity_log: Vec::new(),
            last_activity_status: String::new(),
            active_workspace: None,
            minimized_apps: HashSet::new(),
            window_restore_rects: HashMap::new(),
            desktop_rect: None,
            desktop_last_size: None,
            desktop_layout_generation: 0,
            settings_page: 0,
            window_snap_requests: HashMap::new(),
            window_motions: HashMap::new(),
            window_rects: HashMap::new(),
            start_search: String::new(),
            chrome_path: None,
            chrome_checked: false,
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
            editor_saved_text: String::new(),
            editor_saved_path: None,
            editor_dirty: false,
            editor_pending_action: None,
            editor_overwrite_pending: None,
            exit_after_editor: false,
            editor_status: "Ready".to_owned(),
            editor_find_open: false,
            editor_find_query: String::new(),
            editor_replace_text: String::new(),
            calc_mode: "Calculate".to_owned(),
            calc_expression: "sin(30)+sqrt(16)".to_owned(),
            calc_result: "Ready".to_owned(),
            calc_angle: AngleUnit::Degree,
            calc_answer: 0.0,
            calc_memory: 0.0,
            calc_memory_present: false,
            calc_history: Vec::new(),
            calc_stats_data: "12, 15, 18, 20, 25".to_owned(),
            calc_stats_y: String::new(),
            calc_equation_a: "1".to_owned(),
            calc_equation_b: "-3".to_owned(),
            calc_equation_c: "2".to_owned(),
            calc_table_start: "-5".to_owned(),
            calc_table_end: "5".to_owned(),
            calc_table_step: "1".to_owned(),
            calc_calculus_x: "1".to_owned(),
            calc_integral_start: "0".to_owned(),
            calc_integral_end: "1".to_owned(),
            calc_integral_steps: "100".to_owned(),
            calc_inequality_operator: ">".to_owned(),
            calc_base_input: "255".to_owned(),
            calc_base_from: 10,
            calc_matrix_a: "1,2,3;4,5,6;7,8,9".to_owned(),
            calc_matrix_b: "9,8,7;6,5,4;3,2,1".to_owned(),
            calc_vector_a: "1,2,3".to_owned(),
            calc_vector_b: "4,5,6".to_owned(),
            calc_ratio_a: "2".to_owned(),
            calc_ratio_b: "3".to_owned(),
            calc_ratio_c: "8".to_owned(),
            monitor: None,
            hardware: None,
            hardware_snapshot: HardwareSnapshot::default(),
            monitor_tab: 0,
            monitor_process_rows: Vec::new(),
            monitor_cpu_baseline_captured: false,
            monitor_cpu_sample_ready: false,
            monitor_process_baseline_captured: false,
            monitor_process_cpu_ready: false,
            monitor_error: None,
            last_monitor_refresh: Instant::now().checked_sub(Duration::from_secs(2)).unwrap_or_else(Instant::now),
            last_process_refresh: Instant::now().checked_sub(Duration::from_secs(10)).unwrap_or_else(Instant::now),
            last_cpu_frequency_refresh: Instant::now().checked_sub(Duration::from_secs(10)).unwrap_or_else(Instant::now),
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
        // Do not show the initial startup banner as a new recent-activity event.
        app.last_activity_status.clone_from(&app.status_message);
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
            visuals.faint_bg_color = Color32::from_rgb(27, 35, 53);
            visuals.code_bg_color = Color32::from_rgb(13, 19, 31);
        } else {
            visuals.panel_fill = Color32::from_rgb(242, 245, 250);
            visuals.window_fill = Color32::from_rgb(255, 255, 255);
            visuals.extreme_bg_color = Color32::from_rgb(233, 237, 244);
            visuals.faint_bg_color = Color32::from_rgb(247, 249, 253);
            visuals.code_bg_color = Color32::from_rgb(239, 243, 250);
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
        visuals.window_corner_radius = egui::CornerRadius::same(17);
        visuals.menu_corner_radius = egui::CornerRadius::same(13);
        visuals.window_stroke = egui::Stroke::new(
            1.0,
            if effective_dark { Color32::from_white_alpha(26) } else { Color32::from_black_alpha(20) },
        );
        visuals.window_highlight_topmost = true;
        visuals.window_shadow = egui::Shadow {
            offset: [0, 14],
            blur: 34,
            spread: 0,
            color: Color32::from_black_alpha(92),
        };
        visuals.popup_shadow = egui::Shadow {
            offset: [0, 8],
            blur: 22,
            spread: 0,
            color: Color32::from_black_alpha(70),
        };
        let (surface, surface_hover, surface_active, border) = if effective_dark {
            (
                Color32::from_rgb(30, 39, 59),
                Color32::from_rgb(43, 55, 80),
                Color32::from_rgb(53, 68, 98),
                Color32::from_white_alpha(22),
            )
        } else {
            (
                Color32::from_rgb(247, 249, 253),
                Color32::from_rgb(235, 241, 251),
                Color32::from_rgb(222, 233, 250),
                Color32::from_black_alpha(18),
            )
        };
        visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, border);
        visuals.widgets.inactive.bg_fill = surface;
        visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, border);
        visuals.widgets.hovered.bg_fill = surface_hover;
        visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, accent.gamma_multiply(0.55));
        visuals.widgets.active.bg_fill = surface_active;
        visuals.widgets.active.bg_stroke = egui::Stroke::new(1.0, accent);
        visuals.widgets.open.bg_fill = surface_hover;
        visuals.widgets.open.bg_stroke = egui::Stroke::new(1.0, accent.gamma_multiply(0.7));
        visuals.widgets.inactive.corner_radius = egui::CornerRadius::same(10);
        visuals.widgets.hovered.corner_radius = egui::CornerRadius::same(10);
        visuals.widgets.active.corner_radius = egui::CornerRadius::same(10);
        visuals.widgets.open.corner_radius = egui::CornerRadius::same(10);
        ctx.set_visuals(visuals);
        ctx.all_styles_mut(|style| {
            style.spacing.item_spacing = egui::vec2(10.0, 8.0);
            style.spacing.button_padding = egui::vec2(12.0, 8.0);
            style.spacing.interact_size = egui::vec2(38.0, 32.0);
            style.spacing.window_margin = egui::Margin::same(16);
            style.visuals.window_corner_radius = egui::CornerRadius::same(17);
        });
        self.applied_theme.clone_from(&self.settings.theme);
        self.applied_effective_theme = effective_theme.to_owned();
        self.applied_accent.clone_from(&self.settings.accent);
    }

    fn window_order(&self, name: &str) -> egui::Order {
        if self.active_workspace.as_deref() == Some(name) { egui::Order::Foreground } else { egui::Order::Middle }
    }

    fn mark_window_active(&mut self, ctx: &egui::Context, name: &str) {
        if self.active_workspace.as_deref() != Some(name) {
            self.active_workspace = Some(name.to_owned());
            ctx.request_repaint();
        }
    }

    fn record_activity(&mut self, message: impl AsRef<str>) {
        let message = message.as_ref().trim();
        if message.is_empty() { return; }
        let (clock, _) = local_clock_strings();
        self.activity_log.insert(0, format!("{clock}  {message}"));
        self.activity_log.truncate(10);
    }

    fn capture_status_activity(&mut self) {
        if self.status_message.trim().is_empty() || self.status_message == self.last_activity_status { return; }
        let message = self.status_message.clone();
        self.record_activity(&message);
        self.last_activity_status = message;
    }

    /// Remove stale window-manager state after the user closes a window with its title-bar X.
    /// Minimized windows retain their rectangle so they can be restored later.
    fn prune_closed_window_state(&mut self) {
        const WORKSPACES: [&str; 14] = [
            "Files", "Terminal", "Editor", "Settings", "Calculator", "Monitor", "Trash",
            "UniKey", "Security Center", "Network Discovery", "Vulnerability Management",
            "Web Security", "Packet Analyzer", "Web / API Inspector",
        ];
        for name in WORKSPACES {
            if !self.app_is_open(name) && !self.minimized_apps.contains(name) {
                let closed_window = self.window_rects.remove(name).is_some();
                self.window_restore_rects.remove(name);
                self.window_motions.remove(name);
                self.window_snap_requests.remove(name);
                if self.active_workspace.as_deref() == Some(name) { self.active_workspace = None; }
                if closed_window { self.record_activity(format!("Closed {name}")); }
            }
        }
    }

    fn set_app_window_open(&mut self, app: &str, open: bool) {
        match app {
            "Files" => self.files_open = open,
            "Terminal" => self.terminal_open = open,
            "Editor" => self.editor_open = open,
            "Settings" => self.settings_open = open,
            "Calculator" => self.calculator_open = open,
            "Monitor" => self.monitor_open = open,
            "Trash" => self.trash_open = open,
            "UniKey" => self.unikey_open = open,
            "Security Center" => self.security_open = open,
            "Network Discovery" => self.security_tool_open[SecurityTool::NetworkDiscovery.index()] = open,
            "Vulnerability Management" => self.security_tool_open[SecurityTool::VulnerabilityManagement.index()] = open,
            "Web Security" => self.security_tool_open[SecurityTool::WebSecurity.index()] = open,
            "Packet Analyzer" => self.security_tool_open[SecurityTool::PacketAnalyzer.index()] = open,
            "Web / API Inspector" => self.security_tool_open[SecurityTool::WebApiInspector.index()] = open,
            _ => {}
        }
    }

    fn minimize_app(&mut self, app: &str) {
        let was_open = self.app_is_open(app);
        self.set_app_window_open(app, false);
        if was_open {
            self.minimized_apps.insert(app.to_owned());
            self.record_activity(format!("Minimized {app}"));
        }
        if self.active_workspace.as_deref() == Some(app) { self.active_workspace = None; }
    }

    fn close_app(&mut self, app: &str) {
        if app == "Editor" && self.editor_dirty {
            self.request_editor_action(PendingEditorAction::Close);
            return;
        }
        let had_window = self.app_is_open(app) || self.minimized_apps.contains(app);
        self.set_app_window_open(app, false);
        self.minimized_apps.remove(app);
        self.window_restore_rects.remove(app);
        self.window_motions.remove(app);
        self.window_snap_requests.remove(app);
        self.window_rects.remove(app);
        if self.active_workspace.as_deref() == Some(app) { self.active_workspace = None; }
        if had_window { self.record_activity(format!("Closed {app}")); }
    }

    fn window_switcher(&mut self, ctx: &egui::Context) {
        if !self.task_view_open { return; }
        let open_names: Vec<&'static str> = START_APPS.iter()
            .filter(|app| (self.app_is_open(app.name) || self.minimized_apps.contains(app.name)) && app.name != "Google Chrome")
            .map(|app| app.name).collect();
        let dark = self.is_dark_mode(ctx);
        egui::Area::new(egui::Id::new("nova-task-view-switcher"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -72.0))
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style())
                    .fill(if dark { Color32::from_rgb(24, 32, 49) } else { Color32::from_rgb(248, 250, 255) })
                    .stroke(egui::Stroke::new(1.0, if dark { Color32::from_white_alpha(28) } else { Color32::from_black_alpha(22) }))
                    .corner_radius(egui::CornerRadius::same(18))
                    .inner_margin(egui::Margin::same(12))
                    .show(ui, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(RichText::new("Task View · Open windows").strong());
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.small_button("✕").clicked() { self.task_view_open = false; }
                            });
                        });
                        ui.add_space(5.0);
                        if open_names.is_empty() {
                            ui.label(RichText::new("No open NOVA windows. Open an app from Start or the desktop.").small().weak());
                        } else {
                            ui.horizontal_wrapped(|ui| {
                                for name in &open_names {
                                    if START_APPS.iter().any(|app| app.name == *name) {
                                        let (accent, _) = app_icon_colors(name);
                                        let state = if self.minimized_apps.contains(*name) { "Minimized" }
                                            else if self.active_workspace.as_deref() == Some(*name) { "Active" }
                                            else { "Open" };
                                        if ui.button(RichText::new(format!("●  {name}  ·  {state}")).color(accent)).clicked() {
                                            self.open_app(name);
                                            self.task_view_open = false;
                                            ctx.request_repaint();
                                        }
                                    }
                                }
                            });
                        }
                    });
            });
    }

    fn is_dark_mode(&self, ctx: &egui::Context) -> bool {
        if self.settings.theme == "system" {
            ctx.theme() == egui::Theme::Dark
        } else {
            self.settings.theme != "light"
        }
    }

    fn open_app(&mut self, app: &str) {
        let was_open = self.app_is_open(app);
        let was_minimized = self.minimized_apps.remove(app);
        // Chrome is a native host window. NOVA launches it but cannot truthfully
        // claim ownership of its foreground/minimized state.
        if app != "Google Chrome" {
            self.active_workspace = Some(app.to_owned());
        }
        self.task_view_open = false;
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
                self.detect_chrome_if_needed();
                self.launch_chrome();
            },
            "UniKey" => {
                self.unikey_open = true;
                self.detect_unikey_if_needed();
                if self.unikey_path.is_some() && self.unikey_child.is_none() { self.start_unikey(); }
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
        self.notification_open = false;
        if app != "Google Chrome" && (!was_open || was_minimized) {
            self.record_activity(if was_minimized { format!("Restored {app}") } else { format!("Opened {app}") });
        }
    }

    /// A taskbar click focuses a background window; only the already-active
    /// window is minimized. This matches the normal Windows taskbar behavior.
    fn toggle_app(&mut self, app: &str) {
        if self.minimized_apps.contains(app) || !self.app_is_open(app) {
            self.open_app(app);
        } else if self.active_workspace.as_deref() == Some(app) {
            self.minimize_app(app);
        } else {
            self.active_workspace = Some(app.to_owned());
            self.task_view_open = false;
            self.start_open = false;
            self.notification_open = false;
            self.start_show_all = false;
        }
    }

    fn app_is_open(&self, app: &str) -> bool {
        match app {
            "Files" => self.files_open,
            "Terminal" => self.terminal_open,
            "Editor" => self.editor_open,
            "Settings" => self.settings_open,
            "Calculator" => self.calculator_open,
            "Monitor" => self.monitor_open,
            "Trash" => self.trash_open,
            "UniKey" => self.unikey_open,
            "Security Center" => self.security_open,
            "Network Discovery" => self.security_tool_open[SecurityTool::NetworkDiscovery.index()],
            "Vulnerability Management" => self.security_tool_open[SecurityTool::VulnerabilityManagement.index()],
            "Web Security" => self.security_tool_open[SecurityTool::WebSecurity.index()],
            "Packet Analyzer" => self.security_tool_open[SecurityTool::PacketAnalyzer.index()],
            "Web / API Inspector" => self.security_tool_open[SecurityTool::WebApiInspector.index()],
            // Chrome is a native host window; NOVA does not own its window state.
            "Google Chrome" => false,
            _ => false,
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
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if name.to_ascii_lowercase().starts_with(".nova-copy-") { continue; }
            let Ok(metadata) = fs::symlink_metadata(&path) else { continue };
            if is_link_or_reparse_point(&metadata) || (!metadata.is_dir() && !metadata.is_file()) { continue; }
            if canonical_user_path(&self.paths.user_dir, &path).is_err() { continue; }
            let is_dir = metadata.is_dir();
            let display_size = if metadata.is_file() {
                format_size(metadata.len())
            } else {
                "Folder".to_owned()
            };
            rows.push(CachedFileEntry { path, name, is_dir, display_size });
        }
        rows.sort_by_key(|entry| (!entry.is_dir, entry.name.to_lowercase()));
        self.files_entries = rows;
        self.files_dir = folder.clone();
        self.files_cache_dir = Some(folder);
        self.files_error = None;
        self.files_recursive_search_active = false;
        self.files_search_truncated = false;
        self.files_preview = None;
        self.files_multi_selected.retain(|path| self.files_entries.iter().any(|entry| &entry.path == path));
        if self.files_selected.as_ref().is_some_and(|path| !self.files_entries.iter().any(|entry| &entry.path == path)) {
            self.files_selected = None;
        }
    }

    fn selected_file_paths(&self) -> Vec<PathBuf> {
        let mut selected: Vec<PathBuf> = if self.files_multi_selected.is_empty() {
            self.files_selected.iter().cloned().collect()
        } else {
            self.files_multi_selected.iter().cloned().collect()
        };
        selected.sort();
        selected
    }

    fn visible_file_paths(&self) -> Vec<PathBuf> {
        let filter = if self.files_recursive_search_active { String::new() } else { self.files_search.trim().to_lowercase() };
        self.files_entries.iter()
            .filter(|entry| filter.is_empty() || entry.name.to_lowercase().contains(&filter))
            .map(|entry| entry.path.clone())
            .collect()
    }

    fn select_file_path(&mut self, path: PathBuf, additive: bool) {
        if additive {
            if !self.files_multi_selected.insert(path.clone()) {
                self.files_multi_selected.remove(&path);
            }
        } else {
            self.files_multi_selected.clear();
            self.files_multi_selected.insert(path.clone());
        }
        self.files_selected = self.files_multi_selected.iter().next().cloned();
        if self.files_multi_selected.is_empty() { self.files_selected = None; }
        self.files_preview = None;
    }

    fn start_files_operation(&mut self, sources: Vec<PathBuf>, cut: bool) {
        if self.files_worker_busy || sources.is_empty() { return; }
        let root = self.paths.user_dir.clone();
        let destination = self.files_dir.clone();
        let (sender, receiver) = mpsc::channel();
        let operation_name = if cut { "Move" } else { "Copy" };
        let spawn = thread::Builder::new()
            .name("nova-files-operation".to_owned())
            .spawn(move || {
                let mut completed = Vec::new();
                let mut failures = Vec::new();
                let mut retry_sources = Vec::new();
                for source in sources {
                    let result = if cut {
                        file_ops::move_item(&root, &source, &destination)
                    } else {
                        file_ops::copy_item(&root, &source, &destination)
                    };
                    match result {
                        Ok(path) => completed.push(path.file_name().and_then(|name| name.to_str()).unwrap_or("item").to_owned()),
                        Err(error) => {
                            failures.push(format!("{}: {error}", source.display()));
                            retry_sources.push(source);
                        }
                    }
                }
                let mut message = format!("{operation_name} completed for {} item(s).", completed.len());
                if !completed.is_empty() {
                    message.push_str(&format!(" Created: {}.", completed.join(", ")));
                }
                if !failures.is_empty() {
                    message.push_str(&format!(" {} item(s) failed: {}", failures.len(), failures.join("; ")));
                }
                let _ = sender.send(FilesWorkerResult::Operation { message, clear_clipboard: cut, retry_sources });
            });
        match spawn {
            Ok(_) => {
                self.files_worker_receiver = Some(receiver);
                self.files_worker_busy = true;
                self.status_message = if cut { "Moving files in the background…" } else { "Copying files in the background…" }.to_owned();
            }
            Err(error) => self.status_message = format!("Could not start file operation: {error}"),
        }
    }

    fn start_recursive_file_search(&mut self) {
        if self.files_worker_busy { return; }
        let query = self.files_search.trim().to_owned();
        if query.is_empty() {
            self.refresh_files();
            return;
        }
        let root = self.paths.user_dir.clone();
        let start = root.clone();
        let worker_query = query.clone();
        let (sender, receiver) = mpsc::channel();
        match thread::Builder::new().name("nova-files-search".to_owned()).spawn(move || {
            let result = file_ops::search_user_files(&root, &start, &worker_query).map_err(|error| error.to_string());
            let _ = sender.send(FilesWorkerResult::Search { query: worker_query, result });
        }) {
            Ok(_) => {
                self.files_worker_receiver = Some(receiver);
                self.files_worker_busy = true;
                self.status_message = format!("Searching NOVA Files for '{query}'…");
            }
            Err(error) => self.status_message = format!("Could not start search: {error}"),
        }
    }

    fn poll_files_worker(&mut self, ctx: &egui::Context) {
        let received = match self.files_worker_receiver.as_ref() {
            Some(receiver) => match receiver.try_recv() {
                Ok(result) => Some(Some(result)),
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => Some(None),
            },
            None => None,
        };
        match received {
            Some(Some(FilesWorkerResult::Operation { message, clear_clipboard, retry_sources })) => {
                self.files_worker_receiver = None;
                self.files_worker_busy = false;
                self.status_message = message;
                if clear_clipboard {
                    self.files_clipboard = retry_sources;
                    self.files_clipboard_cut = !self.files_clipboard.is_empty();
                    if !self.files_clipboard.is_empty() {
                        self.status_message.push_str(" Failed cut items remain in the clipboard for retry.");
                    }
                }
                self.refresh_files();
            }
            Some(Some(FilesWorkerResult::Search { query, result })) => {
                self.files_worker_receiver = None;
                self.files_worker_busy = false;
                match result {
                    Ok(result) => {
                        self.files_error = None;
                        self.files_entries = result.paths.into_iter().filter_map(|path| {
                            let metadata = fs::symlink_metadata(&path).ok()?;
                            let is_dir = metadata.is_dir();
                            let name = path.file_name()?.to_string_lossy().to_string();
                            let display_size = if is_dir { "Folder".to_owned() } else { format_size(metadata.len()) };
                            Some(CachedFileEntry { path, name, is_dir, display_size })
                        }).collect();
                        self.files_recursive_search_active = true;
                        self.files_search_truncated = result.truncated;
                        self.files_preview = None;
                        self.files_multi_selected.clear();
                        self.files_selected = None;
                        self.status_message = if result.truncated {
                            format!("Search for '{query}' reached a safety limit; showing the first {} matches.", self.files_entries.len())
                        } else {
                            format!("Search for '{query}' returned {} item(s).", self.files_entries.len())
                        };
                    }
                    Err(error) => self.status_message = format!("Search failed: {error}"),
                }
            }
            Some(None) => {
                self.files_worker_receiver = None;
                self.files_worker_busy = false;
                self.status_message = "File worker stopped unexpectedly.".to_owned();
            }
            None if self.files_worker_busy => ctx.request_repaint_after(Duration::from_millis(100)),
            None => {}
        }
    }

    fn preview_selected_file(&mut self) {
        let selected = self.selected_file_paths();
        if selected.len() != 1 {
            self.status_message = "Select exactly one file to preview.".to_owned();
            return;
        }
        match file_ops::read_text_preview(&self.paths.user_dir, &selected[0]) {
            Ok(text) => self.files_preview = Some((selected[0].clone(), text)),
            Err(error) => {
                self.files_preview = Some((selected[0].clone(), format!("Preview unavailable: {error}")));
            }
        }
    }

    fn start_network_scan(&mut self) {
        if self.network_busy { return; }
        let (sender, receiver) = mpsc::channel();
        match thread::Builder::new().name("nova-wifi-scan".to_owned()).spawn(move || {
            let result = wifi::scan_networks().map_err(|error| error.to_string());
            let _ = sender.send(NetworkWorkerResult::Scan(result));
        }) {
            Ok(_) => {
                self.network_worker_receiver = Some(receiver);
                self.network_busy = true;
                self.network_status = "Scanning nearby Wi-Fi networks…".to_owned();
            }
            Err(error) => self.network_status = format!("Could not start Wi-Fi scan: {error}"),
        }
    }

    fn start_network_connect(&mut self) {
        if self.network_busy { return; }
        let Some(ssid) = self.network_selected_ssid.clone().filter(|ssid| !ssid.trim().is_empty()) else {
            self.network_status = "Select a Wi-Fi network first.".to_owned();
            return;
        };
        let password = std::mem::take(&mut self.network_password);
        let password = if password.is_empty() { None } else { Some(password) };
        let paths = self.paths.clone();
        let (sender, receiver) = mpsc::channel();
        match thread::Builder::new().name("nova-wifi-connect".to_owned()).spawn(move || {
            let result = wifi::connect(&paths, &ssid, password.as_deref()).map_err(|error| error.to_string());
            let _ = sender.send(NetworkWorkerResult::Connect(result));
        }) {
            Ok(_) => {
                self.network_worker_receiver = Some(receiver);
                self.network_busy = true;
                self.network_status = format!("Connecting to '{ssid}'… The password is not saved in NOVA.");
            }
            Err(error) => self.network_status = format!("Could not start Wi-Fi connection: {error}"),
        }
    }

    fn start_network_disconnect(&mut self) {
        if self.network_busy { return; }
        let (sender, receiver) = mpsc::channel();
        match thread::Builder::new().name("nova-wifi-disconnect".to_owned()).spawn(move || {
            let result = wifi::disconnect().map_err(|error| error.to_string());
            let _ = sender.send(NetworkWorkerResult::Disconnect(result));
        }) {
            Ok(_) => {
                self.network_worker_receiver = Some(receiver);
                self.network_busy = true;
                self.network_status = "Disconnecting from the current Wi-Fi network…".to_owned();
            }
            Err(error) => self.network_status = format!("Could not start disconnect: {error}"),
        }
    }

    fn poll_network_worker(&mut self, ctx: &egui::Context) {
        let received = match self.network_worker_receiver.as_ref() {
            Some(receiver) => match receiver.try_recv() {
                Ok(result) => Some(Some(result)),
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => Some(None),
            },
            None => None,
        };
        match received {
            Some(Some(NetworkWorkerResult::Scan(result))) => {
                self.network_worker_receiver = None;
                self.network_busy = false;
                match result {
                    Ok(networks) => {
                        self.network_networks = networks;
                        if self.network_selected_ssid.as_ref().is_none_or(|ssid| !self.network_networks.contains(ssid)) {
                            self.network_selected_ssid = self.network_networks.first().cloned();
                        }
                        self.network_status = format!("Found {} network(s). Select one to connect.", self.network_networks.len());
                    }
                    Err(error) => self.network_status = format!("Wi-Fi scan failed: {error}"),
                }
            }
            Some(Some(NetworkWorkerResult::Connect(result))) => {
                self.network_worker_receiver = None;
                self.network_busy = false;
                self.network_status = result.unwrap_or_else(|error| format!("Connection failed: {error}"));
            }
            Some(Some(NetworkWorkerResult::Disconnect(result))) => {
                self.network_worker_receiver = None;
                self.network_busy = false;
                self.network_status = result.unwrap_or_else(|error| format!("Disconnect failed: {error}"));
            }
            Some(None) => {
                self.network_worker_receiver = None;
                self.network_busy = false;
                self.network_status = "Wi-Fi worker stopped unexpectedly.".to_owned();
            }
            None if self.network_busy => ctx.request_repaint_after(Duration::from_millis(100)),
            None => {}
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
            Some(path) => match apps::launch_chrome(path) {
                Ok(()) => self.status_message = "Opened the installed Google Chrome in its native Windows window.".to_owned(),
                Err(error) => self.status_message = format!("Could not launch Google Chrome: {error}"),
            },
            None => {
                self.status_message = "Google Chrome is not installed. Install it from google.com/chrome; NOVA will open the real Chrome application when available.".to_owned();
                if let Err(error) = apps::open_chrome_download_page() {
                    self.status_message = format!("Chrome was not found and its official download page could not be opened: {error}");
                }
            }
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


    fn start_unikey(&mut self) {
        self.unikey_path = unikey::find_portable(&self.paths.app_dir);
        self.unikey_checked = true;
        let Some(path) = self.unikey_path.clone() else {
            self.unikey_status = "UniKey is not bundled. Rebuild the portable package with internet access.".to_owned();
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

        let snap = self.take_window_snap("Security Center", ctx);
        let mut window = egui::Window::new("NOVA Security Center")
            .order(self.window_order("Security Center"))
            .open(&mut open)
            .fade_in(!self.settings.reduce_motion)
            .fade_out(!self.settings.reduce_motion)
            .collapsible(false)
            .default_size([820.0, 690.0]);
        if let Some((position, size)) = snap { window = window.current_pos(position).fixed_size(size); }
        let shown = window.show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
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

                let overview_columns = if ui.available_width() < 560.0 { 1 } else if ui.available_width() < 760.0 { 2 } else { 3 };
                egui::Grid::new("nova-security-overview-cards")
                    .num_columns(overview_columns)
                    .spacing(egui::vec2(10.0, 8.0))
                    .show(ui, |ui| {
                        let cards = [
                            ("GUI TOOLS", format!("{installed_count}/4"), "local desktop engines detected"),
                            ("WINDOWS SANDBOX", if sandbox_available { "Detected".to_owned() } else { "Unavailable".to_owned() }, "feature + virtualization required"),
                            ("TEST SCOPE", if scope_ready { "Confirmed".to_owned() } else { "Not confirmed".to_owned() }, "operator acknowledgement"),
                        ];
                        for (index, (label, value, description)) in cards.iter().enumerate() {
                            egui::Frame::group(ui.style()).show(ui, |ui| {
                                ui.set_min_width(175.0);
                                ui.label(RichText::new(*label).small().weak().strong());
                                ui.heading(value.as_str());
                                ui.label(RichText::new(*description).small().weak());
                            });
                            if (index + 1) % overview_columns == 0 { ui.end_row(); }
                        }
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
                let tool_columns = if ui.available_width() < 660.0 { 1 } else { 2 };
                egui::Grid::new("nova-security-tool-cards")
                    .num_columns(tool_columns)
                    .spacing(egui::vec2(12.0, 10.0))
                    .show(ui, |ui| {
                        for (position, info) in security_tools::TOOLS.iter().enumerate() {
                            let installed = self.security_tool_installed[info.tool.index()];
                            egui::Frame::group(ui.style()).show(ui, |ui| {
                                ui.set_min_width(220.0);
                                ui.set_min_height(112.0);
                                ui.horizontal_wrapped(|ui| {
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
                                ui.horizontal_wrapped(|ui| {
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
                            if (position + 1) % tool_columns == 0 { ui.end_row(); }
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
                        ui.label("Inspect in Sandbox stages a copy of one file, maps it read-only, disables networking and clipboard, then opens Explorer. Google Chrome runs as the normal Windows host application and is not sandboxed.");
                        ui.label("NOVA, Terminal/CMD, Google Chrome, UniKey, Wi-Fi and ordinary host applications still run on Windows itself. No isolation feature can promise protection against every Windows, hypervisor or firmware vulnerability.");
                        ui.horizontal_wrapped(|ui| {
                            ui.label(format!("Staged sample/config items: {}", self.quarantine_count));
                            if ui.add_enabled(self.quarantine_count > 0, egui::Button::new("Clear staged copies…")).clicked() {
                                clear_requested = true;
                            }
                        });
                    });
            });
        if let Some(shown) = shown {
            self.window_rects.insert("Security Center".to_owned(), shown.response.rect);
            if shown.response.hovered() { self.mark_window_active(ctx, "Security Center"); }
        }

        self.security_open = open;
        if refresh_detection {
            self.security_tools_checked = false;
            self.load_security_detection_if_needed();
        }
        if let Some(tool) = selected_tool {
            self.security_tool_open[tool.index()] = true;
            self.active_workspace = Some(tool.info().name.to_owned());
            self.record_activity(format!("Opened {}", tool.info().name));
        }
        if clear_requested {
            self.security_confirm_clear_quarantine = true;
        }
        if self.security_confirm_clear_quarantine {
            let mut confirm_open = true;
            let mut close_requested = false;
            let mut clear_confirmed = false;
            egui::Window::new("Clear NOVA quarantine staging?")
                .open(&mut confirm_open)
                .collapsible(false)
                .resizable(false)
                .fade_in(!self.settings.reduce_motion)
                .fade_out(!self.settings.reduce_motion)
                .show(ctx, |ui| {
                    ui.label("Close every Windows Sandbox window before removing staged copies and NOVA .wsb launch files. Original workspace files are not deleted.");
                    ui.horizontal_wrapped(|ui| {
                        if ui.button("Clear staged copies").clicked() { clear_confirmed = true; }
                        if ui.button("Cancel").clicked() { close_requested = true; }
                    });
                });
            self.security_confirm_clear_quarantine = confirm_open && !close_requested;
            if clear_confirmed {
                match isolation::clear_quarantine(&self.paths) {
                    Ok(count) => self.status_message = format!("Removed {count} staged sample/config item(s). Original workspace files were not changed."),
                    Err(error) => self.status_message = format!("Could not clear staged samples: {error}"),
                }
                self.quarantine_count = isolation::quarantine_item_count(&self.paths).unwrap_or(0);
                self.quarantine_count_loaded = true;
                self.security_confirm_clear_quarantine = false;
            }
        }
    }

    fn queue_security_tool_launch(&mut self, index: usize, tool: SecurityTool, allow_unsigned_zap: bool) -> Result<(), String> {
        if self.security_launch_receiver.is_some() {
            return Err("Another security-tool launch is already being verified. Wait for it to finish.".to_owned());
        }
        let app_dir = self.paths.app_dir.clone();
        let (sender, receiver) = mpsc::channel();
        self.security_launch_receiver = Some(receiver);
        self.security_launching[index] = true;
        self.security_tool_status[index] = "Verifying Authenticode publisher and starting application…".to_owned();
        // Dropping the handle detaches this bounded worker; its result is
        // collected through the channel on the next repaint.
        let _worker = thread::spawn(move || {
            let result = tool.launch(&app_dir, allow_unsigned_zap).map_err(|error| error.to_string());
            let _ = sender.send((index, result));
        });
        Ok(())
    }

    fn poll_security_tool_launch(&mut self) {
        let Some(receiver) = self.security_launch_receiver.take() else { return; };
        match receiver.try_recv() {
            Ok((index, result)) => {
                self.security_launching[index] = false;
                let status = match result {
                    Ok(path) => format!("Launched {} from {}", security_tools::TOOLS[index].name, path.display()),
                    Err(error) => error,
                };
                self.security_tool_status[index] = status.clone();
                self.status_message = status;
            }
            Err(TryRecvError::Empty) => self.security_launch_receiver = Some(receiver),
            Err(TryRecvError::Disconnected) => {
                for index in 0..self.security_launching.len() {
                    if self.security_launching[index] {
                        self.security_launching[index] = false;
                        self.security_tool_status[index] = "Security-tool launch worker stopped before returning a result.".to_owned();
                    }
                }
            }
        }
    }

    fn security_tool_window(&mut self, ctx: &egui::Context, tool: SecurityTool) {
        let index = tool.index();
        // Preserve lazy host detection until a security workspace has actually been opened.
        if !self.security_tool_open[index] && !self.security_tools_checked { return; }
        self.load_security_detection_if_needed();

        let info = tool.info();
        let detected = self.security_tool_paths[index].clone();
        let mut open = self.security_tool_open[index];
        let mut launch_requested = false;
        let mut requested_url: Option<&'static str> = None;
        let mut status = self.security_tool_status[index].clone();
        let scope_ready = security_tools::validate_authorized_scope(&self.security_scope, self.security_scope_confirmed).is_ok();

        let snap = self.take_window_snap(info.name, ctx);
        let mut window = egui::Window::new(format!("NOVA · {}", info.name))
            .order(self.window_order(info.name))
            .open(&mut open)
            .fade_in(!self.settings.reduce_motion)
            .fade_out(!self.settings.reduce_motion)
            .collapsible(false)
            .default_size([590.0, 410.0]);
        if let Some((position, size)) = snap { window = window.current_pos(position).fixed_size(size); }
        let shown = window.show(ctx, |ui| {
                ui.heading(info.name);
                ui.label(RichText::new(info.description).weak());
                ui.separator();
                ui.label(RichText::new("AUTHORIZED SCOPE").small().strong().weak());
                ui.label(if self.security_scope.trim().is_empty() { "No target scope configured in Security Center.".to_owned() } else { self.security_scope.clone() });
                if !scope_ready {
                    ui.colored_label(Color32::from_rgb(225, 166, 86), "Return to Security Center, enter the authorized scope and confirm permission before launching this tool.");
                }

                if tool == SecurityTool::WebSecurity {
                    ui.separator();
                    ui.colored_label(Color32::from_rgb(225, 166, 86), "OWASP ZAP's official Windows releases are currently unsigned. NOVA requires a per-session acknowledgement before launching an unsigned copy.");
                    ui.checkbox(
                        &mut self.security_unsigned_zap_acknowledged,
                        "I obtained this ZAP copy from the official OWASP ZAP project and accept launching an unsigned executable for this session.",
                    );
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
                    let launch_available = self.security_launch_receiver.is_none();
                    let launch_label = if self.security_launching[index] {
                        format!("Verifying {}…", info.executable)
                    } else {
                        format!("Launch {}", info.executable)
                    };
                    if ui.add_enabled(scope_ready && detected.is_some() && launch_available, egui::Button::new(launch_label)).clicked() {
                        launch_requested = true;
                    }
                }

                ui.separator();
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Official download / project page").clicked() { requested_url = Some(info.website); }
                    if ui.button("Documentation").clicked() { requested_url = Some(info.documentation); }
                });
                ui.label(RichText::new(&status).small().weak());
                ui.separator();
                ui.label(RichText::new("Safety: these third-party apps run on the Windows host, not inside Windows Sandbox. The scope acknowledgement is a reminder, not an enforced network boundary. Use only approved assets; start with passive or non-invasive features and use active testing only with explicit authorization.").small().weak());
            });
        if let Some(shown) = shown {
            self.window_rects.insert(info.name.to_owned(), shown.response.rect);
            if shown.response.hovered() { self.mark_window_active(ctx, info.name); }
        }

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
                match self.queue_security_tool_launch(index, tool, self.security_unsigned_zap_acknowledged) {
                    Ok(()) => status = "Verifying Authenticode publisher and starting application…".to_owned(),
                    Err(error) => status = error,
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
        let mut stop_requested = false;
        let snap = self.take_window_snap("UniKey", ctx);
        let mut window = egui::Window::new("UniKey — Vietnamese Input")
            .order(self.window_order("UniKey"))
            .fade_in(!self.settings.reduce_motion)
            .fade_out(!self.settings.reduce_motion)
            .resizable(false)
            .default_width(540.0);
        if let Some((position, size)) = snap { window = window.current_pos(position).fixed_size(size); }
        let shown = window.open(&mut self.unikey_open).show(ctx, |ui| {
                ui.heading("Vietnamese typing for NOVA and Windows");
                ui.label("NOVA uses Unicode text fields and system-font fallback for installed glyphs. UniKey adds Vietnamese Telex/VNI input on the Windows host, including NOVA Editor and Terminal.");
                ui.separator();
                if let Some(path) = &self.unikey_path {
                    ui.colored_label(Color32::from_rgb(87, 190, 140), "Portable UniKey executable found on this USB");
                    ui.label(RichText::new(path.display().to_string()).small().weak());
                } else {
                    ui.colored_label(Color32::from_rgb(225, 166, 86), "UniKey was not bundled in this build.");
                    ui.label("Rebuild the USB package with internet access so the official UniKey x64 binary can be downloaded and signature-verified automatically.");
                }
                ui.horizontal_wrapped(|ui| {
                    if self.unikey_process_kind == Some(UniKeyProcessKind::Running) {
                        ui.colored_label(Color32::from_rgb(87, 190, 140), "UniKey is running");
                        if ui.button("Stop UniKey").clicked() { stop_requested = true; }
                    } else if self.unikey_path.is_some() && self.unikey_child.is_none() {
                        ui.label("UniKey is bundled. Reopen the UniKey shortcut to retry startup.");
                    } else {
                        ui.label("UniKey is unavailable in this package.");
                    }
                });
                ui.label(RichText::new(&self.unikey_status).small().weak());
                ui.separator();
                ui.heading("Recommended UniKey settings");
                ui.label("1. Open the UniKey tray menu.");
                ui.label("2. Character set: Unicode.");
                ui.label("3. Input method: Telex or VNI.");
                ui.label("4. Keep UniKey running while typing. UniKey is a separate Windows-host process, not part of NOVA or Windows Sandbox. Closing NOVA stops the UniKey process that NOVA started when it can do so cleanly.");
                ui.label(RichText::new("Official source: unikey.org · Publisher must be PHAM KIM LONG · NOVA verifies Authenticode before launch · No Windows service or startup task is installed.").small().weak());
            });
        if let Some(shown) = shown {
            self.window_rects.insert("UniKey".to_owned(), shown.response.rect);
            if shown.response.hovered() { self.mark_window_active(ctx, "UniKey"); }
        }
        // Defer whole-self methods until egui releases the `.open(&mut self.unikey_open)` borrow.
        if stop_requested { self.stop_unikey(); }
    }

    fn catalog_app_button(&mut self, ui: &mut egui::Ui, app: StartApp, width: f32, height: f32) -> bool {
        let (rect, response) = ui.allocate_exact_size(egui::vec2(width.max(1.0), height.max(1.0)), egui::Sense::click());
        let dark = self.is_dark_mode(ui.ctx());
        let accent = app_icon_colors(app.name).0;
        let fill = if response.hovered() {
            if dark { Color32::from_rgb(42, 54, 78) } else { Color32::from_rgb(231, 239, 251) }
        } else if dark { Color32::from_rgb(31, 40, 60) } else { Color32::from_rgb(247, 249, 253) };
        ui.painter().rect_filled(rect, egui::CornerRadius::same(10), fill);
        ui.painter().rect_stroke(rect, egui::CornerRadius::same(10), egui::Stroke::new(1.0, if dark { Color32::from_white_alpha(18) } else { Color32::from_black_alpha(17) }), egui::StrokeKind::Inside);
        let icon_size = (height - 12.0).clamp(22.0, 32.0);
        paint_vector_app_icon(ui.painter(), egui::pos2(rect.left() + 22.0, rect.center().y), app.name, icon_size, dark);
        let text_x = rect.left() + 42.0;
        ui.painter().text(egui::pos2(text_x, rect.center().y - 5.0), egui::Align2::LEFT_CENTER, app.name, FontId::proportional(12.0), if dark { Color32::from_rgb(239, 244, 253) } else { Color32::from_rgb(35, 47, 67) });
        if height >= 46.0 {
            ui.painter().text(egui::pos2(text_x, rect.center().y + 11.0), egui::Align2::LEFT_CENTER, app.description, FontId::proportional(9.0), if dark { Color32::from_rgb(165, 180, 205) } else { Color32::from_rgb(92, 106, 128) });
        }
        let mut pins_changed = false;
        response.context_menu(|ui| {
            let is_pinned = self.settings.pinned_apps.iter().any(|name| name == app.name);
            if is_pinned {
                if ui.button("Unpin from Start").clicked() {
                    self.settings.pinned_apps.retain(|name| name != app.name);
                    pins_changed = true;
                    ui.close();
                }
                ui.menu_button("Reorder Start pin", |ui| {
                    let index = self.settings.pinned_apps.iter().position(|name| name == app.name);
                    if ui.add_enabled(index.is_some_and(|index| index > 0), egui::Button::new("Move earlier")).clicked() {
                        pins_changed |= move_pinned_app(&mut self.settings.pinned_apps, app.name, -1);
                        ui.close();
                    }
                    if ui.add_enabled(index.is_some_and(|index| index + 1 < self.settings.pinned_apps.len()), egui::Button::new("Move later")).clicked() {
                        pins_changed |= move_pinned_app(&mut self.settings.pinned_apps, app.name, 1);
                        ui.close();
                    }
                });
            } else if ui.button("Pin to Start").clicked() {
                self.settings.pinned_apps.push(app.name.to_owned());
                pins_changed = true;
                ui.close();
            }
            if self.app_is_open(app.name) && ui.button("Minimize to taskbar").clicked() {
                self.minimize_app(app.name);
                ui.close();
            } else if !self.app_is_open(app.name) && ui.button("Restore / Open").clicked() {
                self.open_app(app.name);
                ui.close();
            }
        });
        if pins_changed {
            self.status_message = match self.paths.save_settings(&self.settings) {
                Ok(()) => format!("Start pins updated: {}", app.name),
                Err(error) => format!("Start pins changed but could not be saved: {error}"),
            };
        }
        response.on_hover_text(RichText::new(format!("{} · {}", app.name, app.category)).color(accent)).clicked()
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
        let fill_alpha = (base_alpha as f32 + hover * 42.0).round() as u8;
        if fill_alpha > 0 {
            let fill = if dark {
                Color32::from_rgba_unmultiplied(255, 255, 255, fill_alpha)
            } else {
                Color32::from_rgba_unmultiplied(75, 112, 171, fill_alpha)
            };
            ui.painter().rect_filled(
                rect,
                egui::CornerRadius::same(if desktop { 8 } else { 11 }),
                fill,
            );
        }
        if response.hovered() {
            let stroke = if dark {
                Color32::from_white_alpha((hover * 70.0).round() as u8)
            } else {
                Color32::from_rgb(135, 166, 215).gamma_multiply(hover)
            };
            ui.painter().rect_stroke(
                rect,
                egui::CornerRadius::same(if desktop { 8 } else { 11 }),
                egui::Stroke::new(1.0, stroke),
                egui::StrokeKind::Inside,
            );
        }
        let icon_size: f32 = if desktop { 43.0 } else { 36.0 };
        paint_vector_app_icon(ui.painter(), egui::pos2(rect.center().x, rect.top() + if desktop { 29.0 } else { 25.0 }), app.name, icon_size, dark);
        let text_color = if dark { Color32::from_rgb(238, 243, 252) } else { Color32::from_rgb(35, 45, 63) };
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
                ui.menu_button("Reorder Start pin", |ui| {
                    if ui.add_enabled(self.settings.pinned_apps.first().map(String::as_str) != Some(app.name), egui::Button::new("Move earlier")).clicked() {
                        pins_changed |= move_pinned_app(&mut self.settings.pinned_apps, app.name, -1);
                        ui.close();
                    }
                    if ui.add_enabled(self.settings.pinned_apps.last().map(String::as_str) != Some(app.name), egui::Button::new("Move later")).clicked() {
                        pins_changed |= move_pinned_app(&mut self.settings.pinned_apps, app.name, 1);
                        ui.close();
                    }
                });
            } else if ui.button("Pin to Start").clicked() {
                self.settings.pinned_apps.push(app.name.to_owned());
                pins_changed = true;
                ui.close();
            }
            if self.app_is_open(app.name) && ui.button("Minimize to taskbar").clicked() {
                self.minimize_app(app.name);
                ui.close();
            }
            if !self.app_is_open(app.name) && ui.button("Restore / Open").clicked() {
                self.open_app(app.name);
                ui.close();
            }
            if (self.app_is_open(app.name) || self.minimized_apps.contains(app.name)) && ui.button("Close app").clicked() {
                self.close_app(app.name);
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
        if let Some(side) = self.window_snap_requests.remove(app) {
            let content = ctx.content_rect();
            if side == SnapSide::Restore {
                let previous = self.window_restore_rects.remove(app)?;
                let current = self.window_rects.get(app).copied().unwrap_or(previous);
                if self.settings.reduce_motion {
                    self.window_motions.remove(app);
                    return Some((previous.min, previous.size()));
                }
                self.window_motions.insert(app.to_owned(), WindowMotion {
                    from_pos: current.min,
                    from_size: current.size(),
                    to_pos: previous.min,
                    to_size: previous.size(),
                    started: Instant::now(),
                });
            } else {
            let work = egui::Rect::from_min_max(
                content.min + egui::vec2(10.0, 10.0),
                content.max - egui::vec2(10.0, 76.0),
            );
            let work_width = work.width().max(1.0);
            let half_width = ((work_width - 8.0) * 0.5).max(1.0);
            let (x, width) = match side {
                SnapSide::Left => (work.left(), half_width),
                SnapSide::Right => (work.right() - half_width, half_width),
                SnapSide::Maximize => (work.left(), work_width),
                SnapSide::Restore => unreachable!(),
            };
            let height = work.height().max(1.0);
            let to_pos = egui::pos2(x, work.top());
            let to_size = egui::vec2(width, height);
            let current_rect = self.window_rects.get(app).copied().unwrap_or_else(|| {
                let fallback_size = egui::vec2(
                    (width * 0.84).max(320.0).min(width),
                    (height * 0.88).max(240.0).min(height),
                );
                egui::Rect::from_center_size(content.center(), fallback_size)
            });
            self.window_restore_rects.entry(app.to_owned()).or_insert(current_rect);
            if self.settings.reduce_motion {
                self.window_motions.remove(app);
                return Some((to_pos, to_size));
            }
            let from_size = egui::vec2(
                current_rect.width().clamp(220.0, content.width().max(220.0)),
                current_rect.height().clamp(220.0, content.height().max(220.0)),
            );
            self.window_motions.insert(app.to_owned(), WindowMotion {
                from_pos: current_rect.min,
                from_size,
                to_pos,
                to_size,
                started: Instant::now(),
            });
            }
        }

        let motion = self.window_motions.get(app).copied()?;
        let t = (motion.started.elapsed().as_secs_f32() / 0.24).clamp(0.0, 1.0);
        let eased = t * t * (3.0 - 2.0 * t);
        let lerp = |a: f32, b: f32| a + (b - a) * eased;
        let position = egui::pos2(lerp(motion.from_pos.x, motion.to_pos.x), lerp(motion.from_pos.y, motion.to_pos.y));
        let size = egui::vec2(lerp(motion.from_size.x, motion.to_size.x), lerp(motion.from_size.y, motion.to_size.y));
        if t >= 1.0 {
            self.window_motions.remove(app);
        } else {
            ctx.request_repaint_after(Duration::from_millis(16));
        }
        Some((position, size))
    }

    fn taskbar_app_button(
        &mut self,
        ui: &mut egui::Ui,
        _glyph: &str,
        name: &str,
        is_open: bool,
        active: bool,
        dark: bool,
    ) -> bool {
        let (rect, response) = ui.allocate_exact_size(egui::vec2(40.0, 42.0), egui::Sense::click());
        let hover = if self.settings.reduce_motion {
            if response.hovered() { 1.0 } else { 0.0 }
        } else {
            ui.ctx().animate_bool_with_time(ui.scope_id().with(("taskbar-hover", name)), response.hovered(), 0.13)
        };
        let idle = if active {
            if dark { Color32::from_rgb(48, 62, 91) } else { Color32::from_rgb(224, 235, 252) }
        } else if dark {
            Color32::from_rgba_unmultiplied(255, 255, 255, 0)
        } else {
            Color32::from_rgba_unmultiplied(20, 42, 78, 0)
        };
        let hover_fill = if dark { Color32::from_rgb(55, 70, 99) } else { Color32::from_rgb(232, 240, 252) };
        let mix = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * hover).round() as u8;
        let fill = Color32::from_rgba_unmultiplied(
            mix(idle.r(), hover_fill.r()),
            mix(idle.g(), hover_fill.g()),
            mix(idle.b(), hover_fill.b()),
            mix(idle.a(), hover_fill.a()),
        );
        ui.painter().rect_filled(rect, egui::CornerRadius::same(12), fill);
        if active || hover > 0.01 {
            ui.painter().rect_stroke(
                rect,
                egui::CornerRadius::same(12),
                egui::Stroke::new(1.0, if dark { Color32::from_white_alpha((hover * 22.0) as u8) } else { Color32::from_black_alpha((hover * 16.0) as u8) }),
                egui::StrokeKind::Inside,
            );
        }
        paint_vector_app_icon(
            ui.painter(),
            rect.center() + egui::vec2(0.0, -hover * 1.4),
            name,
            27.0 + hover * 1.0,
            dark,
        );
        if active {
            let marker_width = 15.0 + hover * 5.0;
            let marker = egui::Rect::from_center_size(
                egui::pos2(rect.center().x, rect.bottom() - 2.2),
                egui::vec2(marker_width, 3.0),
            );
            let accent = ui.style().visuals.selection.bg_fill;
            ui.painter().rect_filled(marker, egui::CornerRadius::same(2), accent);
        } else if is_open {
            let marker = egui::Rect::from_center_size(
                egui::pos2(rect.center().x, rect.bottom() - 2.2),
                egui::vec2(7.0, 2.0),
            );
            ui.painter().rect_filled(marker, egui::CornerRadius::same(1), if dark { Color32::from_rgb(145, 159, 184) } else { Color32::from_rgb(115, 130, 154) });
        } else if self.minimized_apps.contains(name) {
            ui.painter().circle_filled(egui::pos2(rect.center().x, rect.bottom() - 2.2), 2.0, if dark { Color32::from_rgb(167, 181, 205) } else { Color32::from_rgb(111, 128, 153) });
        }
        if name != "Google Chrome" {
            response.context_menu(|ui| {
                ui.label(RichText::new(name).strong());
                let is_open = self.app_is_open(name);
                if is_open {
                    if ui.button("Minimize").clicked() { self.minimize_app(name); ui.close(); }
                    if ui.button("Close window").clicked() { self.close_app(name); ui.close(); }
                } else {
                    if ui.button("Restore / Open").clicked() {
                        self.open_app(name);
                        ui.close();
                    }
                    if self.minimized_apps.contains(name) && ui.button("Close app").clicked() {
                        self.close_app(name);
                        ui.close();
                    }
                }
                ui.separator();
                ui.label(RichText::new("Arrange window").small().weak());
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
                if ui.button("Maximize / Fill desktop").clicked() {
                    self.open_app(name);
                    self.window_snap_requests.insert(name.to_owned(), SnapSide::Maximize);
                    ui.close();
                }
                if self.window_restore_rects.contains_key(name) && ui.button("Restore previous size").clicked() {
                    self.open_app(name);
                    self.window_snap_requests.insert(name.to_owned(), SnapSide::Restore);
                    ui.close();
                }
            });
        }
        response.on_hover_text(name).clicked()
    }

    fn taskbar(&mut self, ui: &mut egui::Ui) {
        let dark = self.is_dark_mode(ui.ctx());
        let dock_width = ui.ctx().content_rect().width().max(1.0);
        let horizontal_padding = if dock_width < 440.0 { 4.0 } else if dock_width < 760.0 { 8.0 } else { 12.0 };
        let taskbar_fill = if dark {
            Color32::from_rgba_unmultiplied(22, 30, 47, 242)
        } else {
            Color32::from_rgba_unmultiplied(250, 252, 255, 246)
        };
        let border = if dark { Color32::from_white_alpha(28) } else { Color32::from_black_alpha(20) };
        egui::Frame::new()
            .fill(taskbar_fill)
            .stroke(egui::Stroke::new(1.0, border))
            .shadow(egui::Shadow { offset: [0, 5], blur: 24, spread: 0, color: Color32::from_black_alpha(55) })
            .corner_radius(egui::CornerRadius::same(18))
            .inner_margin(egui::Margin::symmetric(horizontal_padding as i8, 5))
            .show(ui, |ui| {
                ui.set_min_height(48.0);
                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    let (left_width, center_width, right_width, ultra_compact, medium) =
                        taskbar_rail_widths(ui.available_width());
                    // Keep the app group visually centered while allowing it to scroll.

                    ui.allocate_ui_with_layout(
                        egui::vec2(left_width, 44.0),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            if left_width >= 24.0 {
                                let logo_text = if ultra_compact || medium { "✦" } else { "✦  NOVA" };
                                let logo_width = if ultra_compact { left_width.min(34.0) } else if medium { left_width.min(44.0) } else { left_width.min(90.0) };
                                let accent = ui.style().visuals.selection.bg_fill;
                                let logo = egui::Button::new(RichText::new(logo_text).size(13.0).strong().color(accent))
                                    .min_size(egui::vec2(logo_width, 38.0))
                                    .corner_radius(egui::CornerRadius::same(12));
                                if ui.add_sized([logo_width, 38.0], logo).on_hover_text("NOVA desktop workspace · Open Settings").clicked() {
                                    self.open_app("Settings");
                                }
                            }
                        },
                    );

                    ui.allocate_ui_with_layout(
                        egui::vec2(center_width, 44.0),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            ui.spacing_mut().item_spacing.x = 4.0;
                            let mut task_apps: Vec<(&'static str, &'static str, bool, bool)> = self.settings.pinned_apps
                                .iter()
                                .filter_map(|name| START_APPS.iter().find(|app| app.name == name.as_str()))
                                .map(|app| {
                                    let is_open = self.app_is_open(app.name);
                                    (app.icon, app.name, is_open, workspace_is_active(self.active_workspace.as_deref(), app.name, is_open))
                                })
                                .collect();
                            for app in &START_APPS {
                                if (self.app_is_open(app.name) || self.minimized_apps.contains(app.name))
                                    && !task_apps.iter().any(|(_, name, _, _)| *name == app.name)
                                {
                                    let is_open = self.app_is_open(app.name);
                                    task_apps.push((app.icon, app.name, is_open, workspace_is_active(self.active_workspace.as_deref(), app.name, is_open)));
                                }
                            }
                            let show_search = center_width >= 520.0;
                            let search_width = if show_search { 88.0 } else { 0.0 };
                            let task_view_width = if center_width >= 112.0 { 38.0 } else { 0.0 };
                            let estimated = 40.0 + task_view_width + search_width + if show_search { 12.0 } else { 8.0 }
                                + task_apps.len() as f32 * 40.0
                                + task_apps.len().saturating_sub(1) as f32 * 4.0;
                            ui.add_space(((center_width - estimated).max(0.0)) * 0.5);

                            let accent = ui.style().visuals.selection.bg_fill;
                            if center_width >= 24.0 {
                                let start_button_width = center_width.min(40.0);
                                if ui.add_sized(
                                    [start_button_width, 38.0],
                                    egui::Button::new(RichText::new("⊞").size((start_button_width * 0.68).min(27.0)).color(accent))
                                        .corner_radius(egui::CornerRadius::same(12)),
                                ).on_hover_text("Start · Ctrl+Space").clicked() {
                                    self.start_open = !self.start_open;
                                    self.start_show_all = false;
                                    self.notification_open = false;
                                    self.task_view_open = false;
                                }
                            }
                            if center_width >= 112.0 {
                                if ui.add_sized([34.0, 36.0], egui::Button::new(RichText::new("▣").size(17.0))
                                    .corner_radius(egui::CornerRadius::same(10)))
                                    .on_hover_text("Task View · Switch open windows").clicked() {
                                    self.task_view_open = !self.task_view_open;
                                    self.start_open = false;
                                    self.notification_open = false;
                                }
                            }
                            if show_search && ui.add_sized(
                                [search_width, 34.0],
                                egui::Button::new(RichText::new("⌕  Search").size(13.0))
                                    .corner_radius(egui::CornerRadius::same(18)),
                            ).clicked() {
                                self.start_open = true;
                                self.start_show_all = false;
                                self.notification_open = false;
                                self.task_view_open = false;
                            }
                            ui.add_space(4.0);
                            egui::ScrollArea::horizontal()
                                .id_salt("nova-taskbar-centered-apps")
                                .max_width(ui.available_width().max(0.0))
                                .max_height(44.0)
                                .auto_shrink([false, true])
                                .show(ui, |ui| {
                                    ui.horizontal_centered(|ui| {
                                        for (glyph, name, is_open, active) in task_apps {
                                            if self.taskbar_app_button(ui, glyph, name, is_open, active, dark) {
                                                self.toggle_app(name);
                                            }
                                        }
                                    });
                                });
                        },
                    );

                    ui.allocate_ui_with_layout(
                        egui::vec2(right_width, 44.0),
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            ui.spacing_mut().item_spacing.x = 5.0;
                            if right_width >= 34.0
                                && ui.add_sized([30.0, 36.0], egui::Button::new("☰").corner_radius(egui::CornerRadius::same(10)))
                                    .on_hover_text("Quick Settings and recent activity").clicked() {
                                self.notification_open = !self.notification_open;
                                self.start_open = false;
                                self.task_view_open = false;
                            }
                            if !ultra_compact {
                                let (clock, date) = local_clock_strings();
                                ui.allocate_ui_with_layout(
                                    egui::vec2(if medium { 58.0 } else { 64.0 }, 36.0),
                                    egui::Layout::top_down(egui::Align::Center),
                                    |ui| {
                                        ui.label(RichText::new(clock).size(12.0).strong().color(if dark { Color32::from_rgb(231, 237, 248) } else { Color32::from_rgb(40, 52, 72) }));
                                        ui.label(RichText::new(date).size(10.0).color(if dark { Color32::from_rgb(166, 180, 204) } else { Color32::from_rgb(91, 105, 128) }));
                                    },
                                );
                                if !medium && ui.add_sized([30.0, 36.0], egui::Button::new("◉").corner_radius(egui::CornerRadius::same(10)))
                                    .on_hover_text("Hardware Center").clicked() {
                                    self.open_app("Monitor");
                                }
                                if ui.add_sized([30.0, 36.0], egui::Button::new("⌁").corner_radius(egui::CornerRadius::same(10)))
                                    .on_hover_text("Network settings").clicked() {
                                    self.settings_page = 3;
                                    self.open_app("Settings");
                                }
                            }
                        },
                    );
                });
            });
        // Keep the taskbar clock fresh without polling every frame.
        ui.ctx().request_repaint_after(Duration::from_secs(20));
    }

    fn notification_center(&mut self, ctx: &egui::Context) {
        let visibility = if self.settings.reduce_motion {
            if self.notification_open { 1.0 } else { 0.0 }
        } else {
            ctx.animate_bool_with_time(egui::Id::unique("nova-quick-settings-visibility"), self.notification_open, 0.19)
        };
        if visibility <= 0.001 { return; }
        let eased = visibility * visibility * (3.0 - 2.0 * visibility);
        if eased < 0.999 { ctx.request_repaint_after(Duration::from_millis(16)); }
        let slide = if self.settings.reduce_motion { 0.0 } else { (1.0 - eased) * 13.0 };
        let dark = self.is_dark_mode(ctx);
        let content_width = ctx.content_rect().width().max(1.0);
        let panel_width = (content_width - 24.0).max(1.0).min(320.0);
        let panel_margin = ((panel_width - 1.0) * 0.5).clamp(0.0, 17.0) as i8;
        let panel_inner_width = (panel_width - panel_margin as f32 * 2.0).max(1.0);
        let fill = if dark { Color32::from_rgb(25, 32, 49) } else { Color32::from_rgb(249, 251, 255) };
        egui::Area::new(egui::Id::unique("nova-quick-settings-panel"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-12.0, -66.0 + slide))
            .show(ctx, |ui| {
                ui.set_opacity(eased);
                egui::Frame::popup(ui.style()).fill(fill).stroke(egui::Stroke::new(1.0, if dark { Color32::from_white_alpha(25) } else { Color32::from_black_alpha(20) })).corner_radius(egui::CornerRadius::same(18))
                    .shadow(egui::Shadow { offset: [0, 12], blur: 28, spread: 0, color: Color32::from_black_alpha(75) })
                    .inner_margin(egui::Margin::same(panel_margin)).show(ui, |ui| {
                        // The inner width plus dynamic frame padding always equals
                        // the bounded outer panel width.
                        ui.set_min_width(panel_inner_width);
                        ui.set_max_width(panel_inner_width);
                        ui.horizontal_wrapped(|ui| {
                            ui.label(RichText::new("Quick settings").size(16.0).strong());
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.small_button("✕").clicked() { self.notification_open = false; }
                            });
                        });
                        ui.label(RichText::new(if self.paths.portable { "USB portable workspace" } else { "Local workspace" }).small().weak());
                        ui.separator();
                        ui.label(RichText::new("Quick controls").small().strong().weak());
                        ui.horizontal_wrapped(|ui| {
                            let compact_controls = panel_width < 280.0;
                            let control_gap = ui.spacing().item_spacing.x;
                            let control_width = ((ui.available_width() - control_gap) * 0.5).max(1.0);
                            let theme_label = if compact_controls {
                                if dark { "☀ Light" } else { "☾ Dark" }
                            } else if dark { "☀  Light mode" } else { "☾  Dark mode" };
                            let motion_label = if compact_controls {
                                if self.settings.reduce_motion { "Motion On" } else { "Motion Off" }
                            } else if self.settings.reduce_motion { "Motion: reduced" } else { "Motion: full" };
                            if ui.add_sized([control_width, 40.0], egui::Button::new(theme_label)).clicked() {
                                self.settings.theme = if dark { "light".to_owned() } else { "dark".to_owned() };
                                self.applied_theme.clear();
                                self.status_message = match self.paths.save_settings(&self.settings) {
                                    Ok(()) => format!("Theme changed to {} mode.", self.settings.theme),
                                    Err(error) => format!("Theme changed for this session but could not be saved: {error}"),
                                };
                            }
                            if ui.add_sized([control_width, 40.0], egui::Button::new(motion_label)).clicked() {
                                self.settings.reduce_motion = !self.settings.reduce_motion;
                                self.status_message = match self.paths.save_settings(&self.settings) {
                                    Ok(()) => if self.settings.reduce_motion { "Reduced motion enabled.".to_owned() } else { "Full motion enabled.".to_owned() },
                                    Err(error) => format!("Motion setting changed for this session but could not be saved: {error}"),
                                };
                            }
                        });
                        ui.horizontal_wrapped(|ui| {
                            if ui.button("⌁ Network").clicked() { self.settings_page = 3; self.open_app("Settings"); }
                            if ui.button("⚙ Settings").clicked() { self.open_app("Settings"); }
                        });
                        ui.horizontal_wrapped(|ui| {
                            if ui.button("◉ Monitor").clicked() { self.open_app("Monitor"); }
                            if ui.button("▦ Files").clicked() { self.open_app("Files"); }
                            if ui.button("⌘ Terminal").clicked() { self.open_app("Terminal"); }
                        });
                        ui.separator();
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("Recent activity").strong());
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.small_button("Clear").clicked() { self.activity_log.clear(); }
                            });
                        });
                        egui::ScrollArea::vertical().max_height(128.0).auto_shrink([false, true]).show(ui, |ui| {
                            if self.activity_log.is_empty() {
                                ui.label(RichText::new("No recent activity yet.").small().weak());
                            } else {
                                for event in &self.activity_log {
                                    ui.label(RichText::new(event).small().weak());
                                }
                            }
                        });
                        ui.add_space(5.0);
                        ui.label(RichText::new("Shortcuts: Ctrl+Space · Ctrl+E · Ctrl+Shift+Esc").small().weak());
                    });
            });
    }

    fn start_menu(&mut self, ctx: &egui::Context) {
        if self.start_open && ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
            self.start_open = false;
        }
        let raw_visibility = if self.settings.reduce_motion {
            if self.start_open { 1.0 } else { 0.0 }
        } else {
            ctx.animate_bool_with_time(egui::Id::unique("nova-windows-start-menu"), self.start_open, 0.22)
        };
        if raw_visibility <= 0.001 {
            return;
        }
        let visibility = raw_visibility * raw_visibility * (3.0 - 2.0 * raw_visibility);
        if visibility < 0.999 && !self.settings.reduce_motion {
            ctx.request_repaint_after(Duration::from_millis(16));
        }

        let mut selected_app: Option<&'static str> = None;
        let mut show_pinned = false;
        let mut show_all = false;
        let content_rect = ctx.content_rect();
        let (menu_width, menu_height, pinned_columns) =
            start_menu_dimensions(content_rect.width(), content_rect.height());
        let menu_margin = ((menu_width - 1.0) * 0.5).clamp(0.0, 19.0) as i8;
        let menu_inner_width = (menu_width - menu_margin as f32 * 2.0).max(1.0);
        let slide_up = if self.settings.reduce_motion { 0.0 } else { (1.0 - visibility) * 18.0 };
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
                    .inner_margin(egui::Margin::same(menu_margin))
                    .show(ui, |ui| {
                        ui.set_min_width(menu_inner_width);
                        ui.set_max_width(menu_inner_width);

                        ui.add(
                            TextEdit::singleline(&mut self.start_search)
                                .hint_text("Search apps and settings")
                                .desired_width(f32::INFINITY),
                        );
                        let query = self.start_search.trim().to_lowercase();
                        ui.add_space(13.0);

                        if self.start_show_all {
                            ui.horizontal_wrapped(|ui| {
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
                                    let columns = if ui.available_width() < 390.0 { 1 } else { 2 };
                                    let gap = 8.0;
                                    let button_width = ((ui.available_width() - gap * (columns - 1) as f32) / columns as f32).max(1.0);
                                    egui::Grid::new(("nova-start-category", category))
                                        .num_columns(columns)
                                        .spacing(egui::vec2(gap, 5.0))
                                        .show(ui, |ui| {
                                            for (index, app) in items.iter().enumerate() {
                                                if self.catalog_app_button(ui, *app, button_width, 47.0) {
                                                    selected_app = Some(app.name);
                                                }
                                                if (index + 1) % columns == 0 { ui.end_row(); }
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
                            ui.horizontal_wrapped(|ui| {
                                ui.heading(RichText::new(if query.is_empty() { "Pinned" } else { "Search results" }).size(19.0).strong());
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if ui.button("All apps  →").clicked() { show_all = true; }
                                });
                            });
                            ui.add_space(7.0);
                            egui::ScrollArea::vertical()
                                .max_height(menu_height)
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
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
                                ui.horizontal_wrapped(|ui| {
                                    ui.heading(RichText::new("Recommended").size(17.0).strong());
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        ui.label(RichText::new("Quick access").small().weak());
                                    });
                                });
                                ui.add_space(5.0);
                                let quick = ["Files", "Terminal", "Settings"];
                                let quick_available = ui.available_width().max(1.0);
                                let quick_columns: usize = if quick_available < 310.0 { 1 } else if quick_available < 470.0 { 2 } else { 3 };
                                let quick_button_width = (quick_available
                                    - (quick_columns.saturating_sub(1) as f32 * 8.0))
                                    / quick_columns as f32;
                                egui::Grid::new("nova-start-quick-access")
                                    .num_columns(quick_columns)
                                    .spacing(egui::vec2(8.0, 6.0))
                                    .show(ui, |ui| {
                                        for (index, name) in quick.iter().enumerate() {
                                            if let Some(app) = START_APPS.iter().find(|app| app.name == *name).copied() {
                                                if self.catalog_app_button(ui, app, quick_button_width, 43.0) {
                                                    selected_app = Some(app.name);
                                                }
                                            }
                                            if (index + 1) % quick_columns == 0 { ui.end_row(); }
                                        }
                                    });
                            }
                                });
                        }

                        ui.add_space(9.0);
                        ui.separator();
                        ui.add_space(5.0);
                        ui.horizontal_wrapped(|ui| {
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

    fn desktop_shortcuts(&mut self, ctx: &egui::Context, desktop: egui::Rect) {
        let columns = ((desktop.width() / 108.0).floor() as usize).clamp(1, 7);
        for (index, name) in DESKTOP_SHORTCUTS.iter().enumerate() {
            let Some(app) = START_APPS.iter().find(|app| app.name == *name).copied() else { continue; };
            let (travel_x, travel_y) = desktop_shortcut_travel(desktop);
            let pos = if let Some(normalized) = self.settings.desktop_icon_positions.get(*name) {
                egui::pos2(
                    desktop.left() + 12.0 + normalized[0] * travel_x,
                    desktop.top() + 58.0 + normalized[1] * travel_y,
                )
            } else {
                desktop_default_position(index, desktop, columns)
            };
            let output = egui::Area::new(egui::Id::new(("nova-desktop-icon", *name)))
                .order(egui::Order::Middle)
                .default_pos(pos)
                .movable(true)
                .constrain_to(desktop.shrink2(egui::vec2(4.0, 4.0)))
                .show(ctx, |ui| self.app_icon_button(ui, app, egui::vec2(94.0, 88.0), true));
            if output.inner { self.open_app(app.name); }
            if output.response.drag_stopped() {
                let actual = output.response.rect.min;
                let normalized = [
                    ((actual.x - desktop.left() - 12.0) / travel_x).clamp(0.0, 1.0),
                    ((actual.y - desktop.top() - 58.0) / travel_y).clamp(0.0, 1.0),
                ];
                self.settings.desktop_icon_positions.insert((*name).to_owned(), normalized);
                self.status_message = match self.paths.save_settings(&self.settings) {
                    Ok(()) => format!("Desktop shortcut position saved: {name}."),
                    Err(error) => format!("Shortcut moved but its position could not be saved: {error}"),
                };
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
            .order(self.window_order("Terminal"))
            .collapsible(false)
            .fade_in(!self.settings.reduce_motion)
            .fade_out(!self.settings.reduce_motion)
            .default_size([760.0, 490.0]);
        if let Some((position, size)) = snap {
            window = window.current_pos(position).fixed_size(size);
        } else {
            window = window.min_width(220.0);
        }
        let shown = window.open(&mut self.terminal_open).show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
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
                    .max_height((ui.available_height() - 82.0).max(80.0))
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for line in &self.terminal.output {
                            ui.label(RichText::new(line).font(FontId::monospace(13.0)));
                        }
                    });
                ui.separator();
                ui.horizontal_wrapped(|ui| {
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
        if let Some(shown) = shown {
            self.window_rects.insert("Terminal".to_owned(), shown.response.rect);
            if shown.response.hovered() { self.mark_window_active(ctx, "Terminal"); }
        }
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
        self.poll_files_worker(ctx);

        // File shortcuts only run when a text field is not consuming keyboard input.
        let (ctrl_a, ctrl_c, ctrl_x, ctrl_v, rename_key, delete_key, escape_key) = ctx.input(|input| (
            input.modifiers.ctrl && input.key_pressed(egui::Key::A),
            input.modifiers.ctrl && input.key_pressed(egui::Key::C),
            input.modifiers.ctrl && input.key_pressed(egui::Key::X),
            input.modifiers.ctrl && input.key_pressed(egui::Key::V),
            input.key_pressed(egui::Key::F2),
            input.key_pressed(egui::Key::Delete),
            input.key_pressed(egui::Key::Escape),
        ));
        if self.files_open && !ctx.wants_keyboard_input() {
            if ctrl_a {
                self.files_multi_selected = self.visible_file_paths().into_iter().collect();
                self.files_selected = self.files_multi_selected.iter().next().cloned();
            } else if ctrl_c || ctrl_x {
                let selected = self.selected_file_paths();
                if !selected.is_empty() {
                    self.files_clipboard = selected;
                    self.files_clipboard_cut = ctrl_x;
                    self.status_message = if ctrl_x { "Cut selection. Choose a destination folder and press Ctrl+V." } else { "Copied selection. Choose a destination folder and press Ctrl+V." }.to_owned();
                }
            } else if ctrl_v && !self.files_clipboard.is_empty() && !self.files_worker_busy {
                self.start_files_operation(self.files_clipboard.clone(), self.files_clipboard_cut);
            } else if rename_key && self.selected_file_paths().len() == 1 {
                let path = self.selected_file_paths()[0].clone();
                self.files_rename_input = path.file_name().map(|name| name.to_string_lossy().to_string()).unwrap_or_default();
                self.files_rename_target = Some(path);
            } else if delete_key && !self.selected_file_paths().is_empty() && !self.files_worker_busy {
                let paths = top_level_selected_paths(self.selected_file_paths());
                self.trash_selected_files(paths);
            } else if escape_key {
                if self.files_preview.is_some() {
                    self.files_preview = None;
                } else {
                    self.files_multi_selected.clear();
                    self.files_selected = None;
                }
            }
        }

        let mut navigate_to: Option<PathBuf> = None;
        let mut open_file: Option<PathBuf> = None;
        let mut create_folder = false;
        let mut refresh = false;
        let mut copy_cut_action: Option<(Vec<PathBuf>, bool)> = None;
        let mut paste_action = false;
        let mut preview_action = false;
        let mut rename_action: Option<PathBuf> = None;
        let mut trash_action: Option<Vec<PathBuf>> = None;
        let mut sandbox_sample: Option<PathBuf> = None;
        let mut search_workspace = false;
        let mut clear_preview = false;
        let mut clear_search = false;
        let mut select_all = false;
        let mut clear_selection = false;
        let selected_paths = self.selected_file_paths();
        let selected_count = selected_paths.len();
        let snap = self.take_window_snap("Files", ctx);
        let mut window = egui::Window::new("▦  NOVA Files")
            .order(self.window_order("Files"))
            .collapsible(false)
            .fade_in(!self.settings.reduce_motion)
            .fade_out(!self.settings.reduce_motion)
            .default_size([840.0, 570.0]);
        if let Some((position, size)) = snap {
            window = window.current_pos(position).fixed_size(size);
        } else {
            window = window.min_width(300.0);
        }
        let shown = window.open(&mut self.files_open).show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                if ui.button("↑  Up").clicked() {
                    if self.files_recursive_search_active {
                        clear_search = true;
                    } else if self.files_dir != self.paths.user_dir {
                        navigate_to = self.files_dir.parent().map(Path::to_path_buf);
                    }
                }
                if ui.button("↻  Refresh").clicked() { refresh = true; }
                ui.separator();
                if self.files_recursive_search_active {
                    ui.label(RichText::new(format!("Search results  /  {}", self.files_search)).strong());
                } else {
                    let relative = self.files_dir.strip_prefix(&self.paths.user_dir).unwrap_or(Path::new("."));
                    ui.label(RichText::new(format!("Home  /  {}", relative.display())).weak());
                }
                if self.files_worker_busy {
                    ui.spinner();
                    ui.label(RichText::new("Working…").small().weak());
                }
            });
            ui.separator();
            let toolbar_width = ui.available_width();
            let folder_input_width = (toolbar_width * 0.26).clamp(100.0, 170.0);
            let filter_input_width = (toolbar_width * 0.27).clamp(110.0, 190.0);
            ui.horizontal_wrapped(|ui| {
                ui.add_enabled_ui(!self.files_worker_busy && !self.files_recursive_search_active, |ui| {
                    ui.add(TextEdit::singleline(&mut self.files_new_folder).hint_text("New folder name").desired_width(folder_input_width));
                    if ui.button("＋ New folder").clicked() { create_folder = true; }
                });
                ui.separator();
                let search_response = ui.add_enabled(!self.files_worker_busy, TextEdit::singleline(&mut self.files_search)
                    .hint_text("Find files in this folder…")
                    .desired_width(filter_input_width));
                if ui.add_enabled(!self.files_worker_busy, egui::Button::new("Search workspace")).clicked()
                    || (!self.files_worker_busy && search_response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter))) {
                    search_workspace = true;
                }
                if self.files_recursive_search_active && ui.button("Clear search").clicked() { clear_search = true; }
                ui.selectable_value(&mut self.files_grid_view, false, "☷ List");
                ui.selectable_value(&mut self.files_grid_view, true, "▦ Grid");
            });
            if selected_count > 0 || !self.files_clipboard.is_empty() || !self.files_entries.is_empty() {
                ui.horizontal_wrapped(|ui| {
                    if selected_count > 0 {
                        ui.label(RichText::new(format!("{selected_count} selected")).strong());
                        if ui.add_enabled(selected_count == 1, egui::Button::new("Open")).clicked() {
                            let selected = &selected_paths[0];
                            if selected.is_dir() { navigate_to = Some(selected.clone()); } else { open_file = Some(selected.clone()); }
                        }
                        if ui.add_enabled(selected_count == 1 && selected_paths[0].is_file(), egui::Button::new("Preview")).clicked() { preview_action = true; }
                        if ui.add_enabled(!self.files_worker_busy, egui::Button::new("Copy")).clicked() { copy_cut_action = Some((selected_paths.clone(), false)); }
                        if ui.add_enabled(!self.files_worker_busy, egui::Button::new("Cut")).clicked() { copy_cut_action = Some((selected_paths.clone(), true)); }
                        if ui.add_enabled(selected_count == 1 && !self.files_worker_busy, egui::Button::new("Rename")).clicked() { rename_action = Some(selected_paths[0].clone()); }
                        if ui.add_enabled(selected_count == 1 && selected_paths[0].is_file(), egui::Button::new("Inspect in Sandbox")).clicked() { sandbox_sample = Some(selected_paths[0].clone()); }
                        if ui.add_enabled(!self.files_worker_busy, egui::Button::new("Move to Trash")).clicked() { trash_action = Some(selected_paths.clone()); }
                        if ui.button("Clear selection").clicked() { clear_selection = true; }
                    }
                    if !self.files_clipboard.is_empty() {
                        let label = if self.files_clipboard_cut { "Paste (Move)" } else { "Paste" };
                        if ui.add_enabled(!self.files_worker_busy, egui::Button::new(label)).clicked() { paste_action = true; }
                        ui.label(RichText::new(format!("{} item(s) in clipboard", self.files_clipboard.len())).small().weak());
                    }
                    if ui.button("Select all").clicked() { select_all = true; }
                });
            }
            ui.separator();
            let filter = if self.files_recursive_search_active { String::new() } else { self.files_search.trim().to_lowercase() };
            let visible_entries: Vec<CachedFileEntry> = self.files_entries.iter()
                .filter(|entry| filter.is_empty() || entry.name.to_lowercase().contains(&filter))
                .cloned()
                .collect();
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                if let Some(error) = &self.files_error {
                    ui.colored_label(Color32::LIGHT_RED, error);
                } else if visible_entries.is_empty() {
                    ui.add_space(18.0);
                    let empty_heading = if self.files_recursive_search_active { "No matches in this workspace" } else if filter.is_empty() { "This folder is empty" } else { "No matching items" };
                    let empty_hint = if self.files_recursive_search_active { "Try another search term or clear search to return to the current folder." } else if filter.is_empty() { "Create a folder or copy files into your NOVA workspace." } else { "Try a different search term." };
                    ui.heading(empty_heading);
                    ui.label(RichText::new(empty_hint).weak());
                } else if self.files_grid_view {
                    let columns = ((ui.available_width() / 150.0).floor() as usize).clamp(1, 5);
                    egui::Grid::new("nova-files-grid-view").num_columns(columns).spacing(egui::vec2(8.0, 8.0)).show(ui, |ui| {
                        for (index, entry) in visible_entries.iter().enumerate() {
                            let path = entry.path.clone();
                            let selected = self.files_multi_selected.contains(&path) || self.files_selected.as_ref() == Some(&path);
                            let label = format!("{}\n{}\n{}", if entry.is_dir { "▰" } else { "▤" }, entry.name, entry.display_size);
                            let response = ui.add_sized([136.0, 82.0], egui::Button::selectable(selected, label).corner_radius(egui::CornerRadius::same(10)))
                                .on_hover_text(path.display().to_string());
                            if response.clicked() {
                                let additive = ui.input(|input| input.modifiers.ctrl || input.modifiers.command);
                                self.select_file_path(path.clone(), additive);
                            }
                            if response.double_clicked() {
                                if entry.is_dir { navigate_to = Some(path.clone()); } else { open_file = Some(path.clone()); }
                            }
                            response.context_menu(|ui| {
                                if ui.button("Open").clicked() {
                                    if entry.is_dir { navigate_to = Some(path.clone()); } else { open_file = Some(path.clone()); }
                                    ui.close();
                                }
                                if !entry.is_dir && ui.button("Preview").clicked() { self.select_file_path(path.clone(), false); preview_action = true; ui.close(); }
                                if ui.button("Copy").clicked() {
                                    let paths = if self.files_multi_selected.contains(&path) { self.selected_file_paths() } else { vec![path.clone()] };
                                    copy_cut_action = Some((paths, false)); ui.close();
                                }
                                if ui.button("Cut").clicked() {
                                    let paths = if self.files_multi_selected.contains(&path) { self.selected_file_paths() } else { vec![path.clone()] };
                                    copy_cut_action = Some((paths, true)); ui.close();
                                }
                                if ui.button("Rename").clicked() { rename_action = Some(path.clone()); ui.close(); }
                                if ui.button("Move to Trash").clicked() {
                                    let paths = if self.files_multi_selected.contains(&path) { self.selected_file_paths() } else { vec![path.clone()] };
                                    trash_action = Some(paths); ui.close();
                                }
                            });
                            if index % columns == columns - 1 { ui.end_row(); }
                        }
                    });
                } else {
                    ui.horizontal(|ui| {
                        let name_width = (ui.available_width() - 74.0 - ui.spacing().item_spacing.x).max(24.0);
                        ui.add_sized([name_width, 20.0], egui::Label::new(RichText::new("NAME").small().strong().weak()));
                        ui.add_sized([74.0, 20.0], egui::Label::new(RichText::new("SIZE").small().strong().weak()));
                    });
                    for entry in &visible_entries {
                        let path = entry.path.clone();
                        let selected = self.files_multi_selected.contains(&path) || self.files_selected.as_ref() == Some(&path);
                        let row_width = ui.available_width().max(1.0);
                        let size_width = 74.0_f32.min(row_width * 0.42).max(42.0_f32.min(row_width));
                        let name_width = (row_width - size_width - ui.spacing().item_spacing.x).max(1.0);
                        let max_chars = ((name_width - 28.0).max(8.0) / 7.2).floor() as usize;
                        let display_name = compact_file_name(&entry.name, max_chars);
                        ui.horizontal(|ui| {
                            let response = ui.add_sized(
                                [name_width, 27.0],
                                egui::Button::selectable(selected, format!("{}  {}", if entry.is_dir { "▰" } else { "▤" }, display_name))
                                    .corner_radius(egui::CornerRadius::same(7)),
                            ).on_hover_text(path.display().to_string());
                            ui.add_sized([size_width, 27.0], egui::Label::new(RichText::new(&entry.display_size).small().weak()));
                            if response.clicked() {
                                let additive = ui.input(|input| input.modifiers.ctrl || input.modifiers.command);
                                self.select_file_path(path.clone(), additive);
                            }
                            if response.double_clicked() {
                                if entry.is_dir { navigate_to = Some(path.clone()); } else { open_file = Some(path.clone()); }
                            }
                            response.context_menu(|ui| {
                                if ui.button("Open").clicked() {
                                    if entry.is_dir { navigate_to = Some(path.clone()); } else { open_file = Some(path.clone()); }
                                    ui.close();
                                }
                                if !entry.is_dir && ui.button("Preview").clicked() { self.select_file_path(path.clone(), false); preview_action = true; ui.close(); }
                                if ui.button("Copy").clicked() {
                                    let paths = if self.files_multi_selected.contains(&path) { self.selected_file_paths() } else { vec![path.clone()] };
                                    copy_cut_action = Some((paths, false)); ui.close();
                                }
                                if ui.button("Cut").clicked() {
                                    let paths = if self.files_multi_selected.contains(&path) { self.selected_file_paths() } else { vec![path.clone()] };
                                    copy_cut_action = Some((paths, true)); ui.close();
                                }
                                if ui.button("Rename").clicked() { rename_action = Some(path.clone()); ui.close(); }
                                if ui.button("Move to Trash").clicked() {
                                    let paths = if self.files_multi_selected.contains(&path) { self.selected_file_paths() } else { vec![path.clone()] };
                                    trash_action = Some(paths); ui.close();
                                }
                            });
                        });
                    }
                }
            });
            if self.files_search_truncated {
                ui.label(RichText::new("Search results are limited for safety; refine your query to narrow the list.").small().weak());
            }
            if let Some((path, preview)) = &self.files_preview {
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new(format!("Preview · {}", path.file_name().unwrap_or_default().to_string_lossy())).strong());
                        if ui.small_button("Open in Editor").clicked() { open_file = Some(path.clone()); }
                        if ui.small_button("Close preview").clicked() { preview_action = false; clear_preview = true; }
                    });
                    egui::ScrollArea::vertical().max_height(150.0).show(ui, |ui| {
                        ui.label(RichText::new(preview).font(FontId::monospace(12.0)));
                    });
                });
            }
            ui.separator();
            ui.label(RichText::new("Ctrl-click selects multiple items · Ctrl+C / Ctrl+X / Ctrl+V copy or move · F2 renames · Delete moves selection to NOVA Trash").small().weak());
        });
        if let Some(shown) = shown {
            self.window_rects.insert("Files".to_owned(), shown.response.rect);
            if shown.response.hovered() { self.mark_window_active(ctx, "Files"); }
        }

        if refresh { self.refresh_files(); }
        if clear_search {
            self.files_search.clear();
            self.files_recursive_search_active = false;
            self.files_search_truncated = false;
            self.refresh_files();
        }
        if search_workspace { self.start_recursive_file_search(); }
        if select_all {
            self.files_multi_selected = self.visible_file_paths().into_iter().collect();
            self.files_selected = self.files_multi_selected.iter().next().cloned();
        }
        if clear_selection {
            self.files_multi_selected.clear();
            self.files_selected = None;
        }
        if create_folder {
            let name = self.files_new_folder.clone();
            if !name.is_empty()
                && !name.to_ascii_lowercase().starts_with(".nova-copy-")
                && !name.contains('/')
                && !name.contains('\\')
            {
                let validated_name = safe_user_path(&self.paths.user_dir, &name);
                let safe_parent = canonical_user_path(&self.paths.user_dir, &self.files_dir);
                match (validated_name, safe_parent) {
                    (Ok(_), Ok(parent)) => {
                        let target = parent.join(&name);
                        match fs::create_dir(&target) {
                            Ok(()) => {
                                self.files_new_folder.clear();
                                self.status_message = format!("Created folder: {name}");
                                self.refresh_files();
                            }
                            Err(error) => self.status_message = format!("Create folder failed: {error}"),
                        }
                    }
                    (Err(error), _) | (_, Err(error)) => self.status_message = format!("Create folder blocked: {error}"),
                }
            }
        }
        if let Some((paths, cut)) = copy_cut_action {
            self.files_clipboard = top_level_selected_paths(paths);
            self.files_clipboard_cut = cut;
            self.status_message = if cut { "Selection cut. Paste it into a destination folder." } else { "Selection copied. Paste it into a destination folder." }.to_owned();
        }
        if paste_action && !self.files_clipboard.is_empty() {
            self.start_files_operation(self.files_clipboard.clone(), self.files_clipboard_cut);
        }
        if preview_action { self.preview_selected_file(); }
        if clear_preview { self.files_preview = None; }
        if let Some(path) = rename_action {
            self.files_rename_input = path.file_name().map(|name| name.to_string_lossy().to_string()).unwrap_or_default();
            self.files_rename_target = Some(path);
        }
        if let Some(paths) = trash_action {
            self.trash_selected_files(top_level_selected_paths(paths));
        }
        if let Some(path) = navigate_to {
            match canonical_user_path(&self.paths.user_dir, &path) {
                Ok(folder) if folder.is_dir() => {
                    self.files_dir = folder;
                    self.files_selected = None;
                    self.files_multi_selected.clear();
                    self.files_search.clear();
                    self.files_recursive_search_active = false;
                    self.refresh_files();
                }
                Ok(_) => self.status_message = "That item is not a folder.".to_owned(),
                Err(error) => self.status_message = format!("Navigation blocked: {error}"),
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

        if let Some(target) = self.files_rename_target.clone() {
            let mut open = true;
            let mut rename_to: Option<String> = None;
            let mut cancel = false;
            egui::Window::new("Rename item")
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .fade_in(!self.settings.reduce_motion)
                .fade_out(!self.settings.reduce_motion)
                .show(ctx, |ui| {
                    ui.label(target.file_name().unwrap_or_default().to_string_lossy());
                    let response = ui.add(TextEdit::singleline(&mut self.files_rename_input).desired_width(280.0));
                    if response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)) {
                        rename_to = Some(self.files_rename_input.clone());
                    }
                    ui.horizontal(|ui| {
                        if ui.button("Rename").clicked() { rename_to = Some(self.files_rename_input.clone()); }
                        if ui.button("Cancel").clicked() { cancel = true; }
                    });
                });
            if let Some(name) = rename_to {
                match file_ops::rename_item(&self.paths.user_dir, &target, &name) {
                    Ok(path) => {
                        self.files_selected = Some(path.clone());
                        self.files_multi_selected.clear();
                        self.files_multi_selected.insert(path);
                        self.files_rename_target = None;
                        self.status_message = "Item renamed successfully.".to_owned();
                        self.refresh_files();
                    }
                    Err(error) => self.status_message = format!("Rename failed: {error}"),
                }
            } else if cancel || !open {
                self.files_rename_target = None;
            }
        }
    }

    fn trash_selected_files(&mut self, paths: Vec<PathBuf>) {
        if paths.is_empty() { return; }
        let mut moved = 0usize;
        let mut failures = Vec::new();
        for path in paths {
            match trash::move_to_trash(&self.paths, &path) {
                Ok(entry) => moved += 1,
                Err(error) => failures.push(format!("{}: {error}", path.display())),
            }
        }
        self.status_message = if failures.is_empty() {
            format!("Moved {moved} item(s) to NOVA Trash.")
        } else {
            format!("Moved {moved} item(s) to Trash; {} failed: {}", failures.len(), failures.join("; "))
        };
        self.files_selected = None;
        self.files_multi_selected.clear();
        self.refresh_files();
        self.refresh_trash();
    }

    fn open_file_in_editor(&mut self, path: PathBuf) {
        self.request_editor_action(PendingEditorAction::OpenFile(path));
    }

    fn request_editor_action(&mut self, action: PendingEditorAction) {
        if self.editor_dirty {
            self.editor_pending_action = Some(action);
            self.editor_open = true;
            self.editor_status = "Unsaved changes — save or discard them before continuing.".to_owned();
        } else {
            self.perform_editor_action(action);
        }
    }

    fn perform_editor_action(&mut self, action: PendingEditorAction) {
        match action {
            PendingEditorAction::NewDocument => {
                let documents_dir = self.paths.user_dir.join("Documents");
                let new_path_status = match create_user_directories(&self.paths.user_dir, &documents_dir) {
                    Ok(directory) => match next_untitled_file_name(&directory) {
                        Ok(name) => {
                            self.editor_path = format!("Documents/{name}");
                            None
                        }
                        Err(error) => {
                            self.editor_path = "Documents/Untitled-new.txt".to_owned();
                            Some(format!("Could not allocate a unique document name: {error}"))
                        }
                    },
                    Err(error) => {
                        // If Documents cannot be created safely, fall back to
                        // an unused file in the already-validated user root.
                        self.editor_path = next_untitled_file_name(&self.paths.user_dir)
                            .unwrap_or_else(|_| "Untitled-new.txt".to_owned());
                        Some(format!("Documents folder unavailable; using the user root: {error}"))
                    }
                };
                self.editor_text.clear();
                self.editor_saved_text.clear();
                self.editor_saved_path = None;
                self.editor_overwrite_pending = None;
                self.editor_dirty = false;
                self.editor_status = new_path_status.unwrap_or_else(|| "New document — not saved".to_owned());
                self.editor_open = true;
            }
            PendingEditorAction::OpenFile(path) => self.load_file_into_editor(path),
            PendingEditorAction::Close => self.editor_open = false,
            PendingEditorAction::ExitApplication => {
                self.editor_open = false;
                self.exit_after_editor = true;
            },
        }
    }

    fn load_file_into_editor(&mut self, path: PathBuf) {
        let source_metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) if !is_link_or_reparse_point(&metadata) => metadata,
            Ok(_) => {
                self.editor_status = "Links and Windows reparse points cannot be opened in the built-in Editor.".to_owned();
                self.editor_open = true;
                return;
            }
            Err(error) => {
                self.editor_status = format!("Cannot inspect file: {error}");
                self.editor_open = true;
                return;
            }
        };
        if !source_metadata.is_file() {
            self.editor_status = "The selected path is not a regular file.".to_owned();
            self.editor_open = true;
            return;
        }
        let canonical_file = match canonical_user_path(&self.paths.user_dir, &path) {
            Ok(file) => file,
            Err(error) => {
                self.editor_status = format!("Cannot open file: {error}");
                self.editor_open = true;
                return;
            }
        };
        match fs::metadata(&canonical_file) {
            Ok(meta) if meta.is_file() && meta.len() <= 2 * 1024 * 1024 => {
                match fs::read(&canonical_file).and_then(|bytes| unicode::decode_text_file(&bytes)) {
                    Ok(text) => {
                        self.editor_text = text;
                        self.editor_saved_text.clone_from(&self.editor_text);
                        self.editor_saved_path = Some(canonical_file.clone());
                        self.editor_overwrite_pending = None;
                        self.editor_dirty = false;
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
                }
            }
            Ok(meta) if meta.is_file() => self.editor_status = "Files larger than 2 MiB are not opened in the built-in editor.".to_owned(),
            Ok(_) => self.editor_status = "The selected path is not a regular file.".to_owned(),
            Err(error) => self.editor_status = format!("Cannot read file metadata: {error}"),
        }
        self.editor_open = true;
    }

    /// Save the current text. Returns `Ok(false)` when an overwrite confirmation
    /// is required; callers must keep the current document/action pending.
    fn save_editor_document(&mut self, confirm_overwrite: bool) -> Result<bool, String> {
        if self.editor_text.len() > 2 * 1024 * 1024 {
            return Err("Files larger than 2 MiB cannot be saved in the built-in editor.".to_owned());
        }
        let path = safe_user_path(&self.paths.user_dir, &self.editor_path)
            .map_err(|error| error.to_string())?;
        let parent = path.parent()
            .filter(|folder| !folder.as_os_str().is_empty())
            .unwrap_or(&self.paths.user_dir);
        let canonical_parent = create_user_directories(&self.paths.user_dir, parent)
            .map_err(|error| format!("Cannot create or resolve target folder: {error}"))?;
        let file_name = path.file_name().ok_or_else(|| "A file name is required.".to_owned())?;
        let safe_target = canonical_parent.join(file_name);

        let target_exists = match fs::symlink_metadata(&safe_target) {
            Ok(_) => true,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
            Err(error) => return Err(format!("Cannot inspect save target: {error}")),
        };
        let is_current_document = self.editor_saved_path.as_ref() == Some(&safe_target);
        if target_exists && !is_current_document && !confirm_overwrite {
            self.editor_overwrite_pending = Some(safe_target);
            self.editor_status = "A file with this name already exists. Confirm overwrite to replace it.".to_owned();
            return Ok(false);
        }
        if confirm_overwrite && self.editor_overwrite_pending.as_ref() != Some(&safe_target) {
            return Err("The overwrite target changed. Review the path and save again.".to_owned());
        }

        // The shared writer rejects reparse points and revalidates the entire
        // parent chain immediately before replacing the target.
        atomic_write_user_file(&safe_target, self.editor_text.as_bytes())
            .map_err(|error| format!("Save failed: {error}"))?;
        self.editor_saved_text.clone_from(&self.editor_text);
        self.editor_saved_path = Some(safe_target);
        self.editor_overwrite_pending = None;
        self.editor_dirty = false;
        self.editor_status = "Saved safely to NOVA user data".to_owned();
        Ok(true)
    }

    fn resolve_editor_pending_action(&mut self, decision: EditorDecision) {
        match decision {
            EditorDecision::Save => match self.save_editor_document(false) {
                Ok(true) => {
                    if let Some(action) = self.editor_pending_action.take() {
                        self.perform_editor_action(action);
                    }
                }
                Ok(false) => {} // Confirmation dialog is now active.
                Err(error) => self.editor_status = error,
            },
            EditorDecision::Discard => {
                if let Some(action) = self.editor_pending_action.take() {
                    // Discard must restore the last saved snapshot as well as
                    // clearing the dirty flag. Otherwise reopening a closed
                    // Editor could show discarded text marked as clean.
                    discard_unsaved_editor_text(&mut self.editor_text, &self.editor_saved_text);
                    self.editor_dirty = false;
                    self.perform_editor_action(action);
                }
            }
            EditorDecision::Cancel => {
                self.editor_pending_action = None;
                self.editor_open = true;
            }
        }
    }

    fn editor_window(&mut self, ctx: &egui::Context) {
        let mut save_requested = false;
        let mut new_document_requested = false;
        let mut replace_all_requested = false;
        let focus_find = ctx.input(|input| input.modifiers.ctrl && input.key_pressed(egui::Key::F));
        if focus_find && self.editor_open { self.editor_find_open = true; }
        let title = if self.editor_dirty { "NOVA Text Editor •" } else { "NOVA Text Editor" };
        let snap = self.take_window_snap("Editor", ctx);
        let mut window = egui::Window::new(title)
            .order(self.window_order("Editor"))
            .collapsible(false)
            .fade_in(!self.settings.reduce_motion)
            .fade_out(!self.settings.reduce_motion)
            .default_size([700.0, 520.0]);
        if let Some((position, size)) = snap { window = window.current_pos(position).fixed_size(size); }

        let mut window_open = self.editor_open;
        let shown = window.open(&mut window_open).show(ctx, |ui| {
            let can_edit = self.editor_pending_action.is_none() && self.editor_overwrite_pending.is_none();
            ui.add_enabled_ui(can_edit, |ui| {
                let path_width = (ui.available_width() * 0.48).clamp(110.0, 330.0);
                ui.horizontal_wrapped(|ui| {
                    ui.label("Path (inside NOVA user folder):");
                    ui.add(TextEdit::singleline(&mut self.editor_path).desired_width(path_width));
                    if ui.button("New").clicked() { new_document_requested = true; }
                    if ui.button("Save · Ctrl+S").clicked() { save_requested = true; }
                    if ui.button("Find / Replace · Ctrl+F").clicked() { self.editor_find_open = !self.editor_find_open; }
                });
                let response = ui.add(
                    TextEdit::multiline(&mut self.editor_text)
                        .font(FontId::monospace(13.0))
                        .desired_rows(20)
                        .desired_width(f32::INFINITY),
                );
                if response.changed() {
                    self.editor_dirty = self.editor_text != self.editor_saved_text;
                }
            });
            if self.editor_find_open {
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.label("Find");
                        let find_response = ui.add(TextEdit::singleline(&mut self.editor_find_query).desired_width(150.0));
                        if focus_find { find_response.request_focus(); }
                        ui.label("Replace");
                        ui.add(TextEdit::singleline(&mut self.editor_replace_text).desired_width(150.0));
                        let matches = if self.editor_find_query.is_empty() { 0 } else { self.editor_text.matches(self.editor_find_query.as_str()).count() };
                        ui.label(RichText::new(format!("{matches} match(es)")).small().weak());
                        if ui.add_enabled(matches > 0 && can_edit, egui::Button::new("Replace all")).clicked() { replace_all_requested = true; }
                        if ui.small_button("Close").clicked() { self.editor_find_open = false; }
                    });
                });
            }
            ui.horizontal_wrapped(|ui| {
                if self.editor_dirty {
                    ui.colored_label(Color32::from_rgb(225, 166, 86), "● Unsaved changes");
                } else {
                    ui.label(RichText::new("Saved").small().weak());
                }
                ui.label(RichText::new(&self.editor_status).small().weak());
                ui.label(RichText::new("Unicode: UTF-8 / UTF-16 · UniKey input").small().weak());
                ui.label(RichText::new(format!("{} chars · {} words · {} lines", self.editor_text.chars().count(), self.editor_text.split_whitespace().count(), self.editor_text.lines().count().max(1))).small().weak());
            });
        });
        if let Some(shown) = shown {
            self.window_rects.insert("Editor".to_owned(), shown.response.rect);
            if shown.response.hovered() { self.mark_window_active(ctx, "Editor"); }
        }

        if replace_all_requested && !self.editor_find_query.is_empty() {
            match replace_all_bounded(
                &self.editor_text,
                &self.editor_find_query,
                &self.editor_replace_text,
                2 * 1024 * 1024,
            ) {
                Ok((updated, matches)) if matches > 0 && updated != self.editor_text => {
                    self.editor_text = updated;
                    self.editor_dirty = self.editor_text != self.editor_saved_text;
                    self.editor_status = format!("Replaced {matches} occurrence(s). Review changes before saving.");
                }
                Ok((_, matches)) if matches > 0 => {
                    self.editor_status = format!("Found {matches} occurrence(s); replacement text is identical, so no changes were made.");
                }
                Ok(_) => self.editor_status = "No matching text was found.".to_owned(),
                Err(error) => self.editor_status = format!("Replace all cancelled: {error}"),
            }
        }
        if self.editor_open && !window_open {
            self.request_editor_action(PendingEditorAction::Close);
        } else if self.editor_pending_action.is_none() {
            self.editor_open = window_open;
        }
        if new_document_requested {
            self.request_editor_action(PendingEditorAction::NewDocument);
        }
        if save_requested && self.editor_overwrite_pending.is_none() {
            if let Err(error) = self.save_editor_document(false) {
                self.editor_status = error;
            }
        }

        if let Some(target) = self.editor_overwrite_pending.as_ref() {
            let target_label = target.strip_prefix(&self.paths.user_dir)
                .unwrap_or(target)
                .display()
                .to_string();
            let mut decision = None;
            egui::Window::new("Confirm file overwrite")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .show(ctx, |ui| {
                    ui.label("A file already exists at this path:");
                    ui.label(RichText::new(target_label).monospace().strong());
                    ui.label("Replace this existing file with the current document?");
                    ui.horizontal(|ui| {
                        if ui.button("Overwrite").clicked() { decision = Some(true); }
                        if ui.button("Cancel").clicked() { decision = Some(false); }
                    });
                    ui.label(RichText::new("The existing file is not changed unless you confirm.").small().weak());
                });
            if let Some(confirm) = decision {
                if confirm {
                    match self.save_editor_document(true) {
                        Ok(true) => {
                            if let Some(action) = self.editor_pending_action.take() {
                                self.perform_editor_action(action);
                            }
                        }
                        Ok(false) => {}
                        Err(error) => self.editor_status = error,
                    }
                } else {
                    self.editor_overwrite_pending = None;
                    self.editor_pending_action = None;
                    self.editor_open = true;
                    self.editor_status = "Overwrite cancelled. Your document is unchanged and the pending action was cancelled.".to_owned();
                }
            }
        } else if self.editor_pending_action.is_some() {
            let mut decision = None;
            egui::Window::new("Unsaved changes")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .show(ctx, |ui| {
                    ui.label("This document has unsaved changes.");
                    ui.label("Save your work before continuing?");
                    ui.horizontal(|ui| {
                        if ui.button("Save").clicked() { decision = Some(EditorDecision::Save); }
                        if ui.button("Discard").clicked() { decision = Some(EditorDecision::Discard); }
                        if ui.button("Cancel").clicked() { decision = Some(EditorDecision::Cancel); }
                    });
                    if !self.editor_status.is_empty() {
                        ui.label(RichText::new(&self.editor_status).small().weak());
                    }
                });
            if let Some(decision) = decision {
                self.resolve_editor_pending_action(decision);
            }
        }
    }

    fn settings_window(&mut self, ctx: &egui::Context) {
        self.poll_network_worker(ctx);
        let mut changed = false;
        let mut launch_app: Option<&'static str> = None;
        let mut open_files_requested = false;
        let snap = self.take_window_snap("Settings", ctx);
        let mut window = egui::Window::new("⚙  NOVA Settings")
            .order(self.window_order("Settings"))
            .collapsible(false)
            .fade_in(!self.settings.reduce_motion)
            .fade_out(!self.settings.reduce_motion)
            .default_size([620.0, 460.0]);
        if let Some((position, size)) = snap {
            window = window.current_pos(position).fixed_size(size);
        } else {
            window = window.min_width(220.0);
        }
        let shown = window.open(&mut self.settings_open).show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
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
                        let accent_color = match self.settings.accent.as_str() {
                            "violet" => Color32::from_rgb(151, 120, 255),
                            "teal" => Color32::from_rgb(50, 190, 174),
                            "rose" => Color32::from_rgb(238, 112, 151),
                            "amber" => Color32::from_rgb(232, 171, 76),
                            _ => Color32::from_rgb(92, 139, 245),
                        };
                        if ui.available_width() < 560.0 {
                            ui.vertical(|ui| {
                                ui.heading("Theme");
                                ui.horizontal_wrapped(|ui| {
                                    changed |= ui.selectable_value(&mut self.settings.theme, "system".to_owned(), "System").changed();
                                    changed |= ui.selectable_value(&mut self.settings.theme, "dark".to_owned(), "Dark").changed();
                                    changed |= ui.selectable_value(&mut self.settings.theme, "light".to_owned(), "Light").changed();
                                });
                                ui.add_space(8.0);
                                ui.heading("Accent color");
                                egui::ComboBox::from_id_salt("nova-accent")
                                    .selected_text(self.settings.accent.to_uppercase())
                                    .show_ui(ui, |ui| {
                                        for accent in ["blue", "violet", "teal", "rose", "amber"] {
                                            changed |= ui.selectable_value(&mut self.settings.accent, accent.to_owned(), accent.to_uppercase()).changed();
                                        }
                                    });
                                ui.add_space(8.0);
                                ui.heading("Preview");
                                ui.group(|ui| {
                                    ui.label(RichText::new("NOVA OS").size(20.0).strong());
                                    ui.label("Portable desktop workspace");
                                    ui.horizontal_wrapped(|ui| {
                                        ui.label("●  Accent");
                                        ui.label(RichText::new("Active").color(accent_color));
                                    });
                                });
                            });
                        } else {
                            ui.columns(2, |columns| {
                                columns[0].heading("Theme");
                                columns[0].horizontal_wrapped(|ui| {
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
                                    ui.horizontal_wrapped(|ui| {
                                        ui.label("●  Accent");
                                        ui.label(RichText::new("Active").color(accent_color));
                                    });
                                });
                            });
                        }
                        ui.separator();
                        changed |= ui.checkbox(&mut self.settings.reduce_motion, "Reduce animations and visual effects").changed();
                    }
                    1 => {
                        ui.heading("System & accessibility");
                        ui.label("Choose how NOVA behaves on this Windows computer.");
                        changed |= ui.checkbox(&mut self.settings.reduce_motion, "Reduce animations and visual effects").changed();
                        ui.separator();
                        ui.horizontal_wrapped(|ui| {
                            if ui.button("Open Hardware Center").clicked() {
                                self.monitor_open = true;
                                self.active_workspace = Some("Monitor".to_owned());
                            }
                            if ui.button("Open File Manager").clicked() { open_files_requested = true; }
                        });
                        ui.horizontal_wrapped(|ui| {
                            if ui.button("Open UniKey input").clicked() {
                                // Route through the common launcher so bundled UniKey is
                                // detected and started, and the window receives focus.
                                launch_app = Some("UniKey");
                            }
                            if ui.button("Open Security Center").clicked() {
                                self.security_open = true;
                                self.active_workspace = Some("Security Center".to_owned());
                            }
                        });
                    }
                    2 => {
                        ui.heading("Installed NOVA workspaces");
                        ui.label(RichText::new("Choose an app to open. Optional host applications are detected only when needed.").small().weak());
                        egui::ScrollArea::vertical().max_height(285.0).show(ui, |ui| {
                            for app in START_APPS {
                                ui.horizontal_wrapped(|ui| {
                                    let (icon_rect, _) = ui.allocate_exact_size(egui::vec2(30.0, 30.0), egui::Sense::hover());
                                    paint_vector_app_icon(ui.painter(), icon_rect.center(), app.name, 27.0, self.is_dark_mode(ctx));
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
                        ui.label("Manage Wi-Fi on the Windows host. NOVA does not create a virtual network adapter or isolate host traffic.");
                        ui.horizontal_wrapped(|ui| {
                            if ui.add_enabled(!self.network_busy, egui::Button::new("Scan Wi-Fi")).clicked() { self.start_network_scan(); }
                            if ui.add_enabled(!self.network_busy, egui::Button::new("Disconnect")).clicked() { self.start_network_disconnect(); }
                            if self.network_busy { ui.spinner(); ui.label(RichText::new("Working…").small().weak()); }
                        });
                        ui.label(RichText::new(&self.network_status).small().weak());
                        ui.separator();
                        ui.label(RichText::new("Nearby networks").strong());
                        egui::ScrollArea::vertical().max_height(170.0).show(ui, |ui| {
                            if self.network_networks.is_empty() {
                                ui.label(RichText::new("No scan results yet. Select Scan Wi-Fi to discover available SSIDs.").small().weak());
                            } else {
                                let networks = self.network_networks.clone();
                                for ssid in &networks {
                                    let selected = self.network_selected_ssid.as_ref() == Some(ssid);
                                    if ui.selectable_label(selected, ssid).clicked() {
                                        self.network_selected_ssid = Some(ssid.clone());
                                        self.network_password.clear();
                                    }
                                }
                            }
                        });
                        if let Some(ssid) = self.network_selected_ssid.clone() {
                            ui.separator();
                            ui.label(format!("Selected network: {ssid}"));
                            ui.horizontal_wrapped(|ui| {
                                ui.label("Password");
                                ui.add_enabled(!self.network_busy, TextEdit::singleline(&mut self.network_password)
                                    .password(true).desired_width(220.0).hint_text("Leave empty for an open network"));
                                if ui.add_enabled(!self.network_busy, egui::Button::new("Connect")).clicked() { self.start_network_connect(); }
                            });
                        }
                        ui.horizontal_wrapped(|ui| {
                            if ui.button("Open Terminal").clicked() {
                                self.terminal_open = true;
                                self.active_workspace = Some("Terminal".to_owned());
                            }
                            if ui.button("Network Discovery").clicked() { launch_app = Some("Network Discovery"); }
                        });
                        ui.label(RichText::new("Wi-Fi scan/connect/disconnect run through Windows WLAN commands with your current account. Passwords are sent only to the worker performing the connection and are not written to NOVA settings or terminal history. Windows may retain the imported Wi-Fi profile under the current user. Signal strength and secured/open status are not inferred from SSID names.").small().weak());
                    }
                    4 => {
                        ui.heading("Security & isolation");
                        ui.label("Manage optional security workspaces and Windows Sandbox integration.");
                        if ui.button("Open Security Center").clicked() {
                            self.security_open = true;
                            self.active_workspace = Some("Security Center".to_owned());
                        }
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
                        ui.monospace("Ctrl + Shift + Esc    Hardware Center");
                        ui.monospace("Esc             Close Start / quick panel");
                    }
                }
                ui.separator();
                ui.label(RichText::new("Settings are stored in the NOVA user data folder. NOVA does not install Windows services or startup tasks.").small().weak());
            });
        if let Some(shown) = shown {
            self.window_rects.insert("Settings".to_owned(), shown.response.rect);
            if shown.response.hovered() { self.mark_window_active(ctx, "Settings"); }
        }
        if !self.settings_open {
            self.network_password.clear();
        }
        if open_files_requested { self.open_app("Files"); }
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
            .order(self.window_order("Trash"))
            .collapsible(false)
            .fade_in(!self.settings.reduce_motion)
            .fade_out(!self.settings.reduce_motion)
            .default_size([690.0, 450.0]);
        if let Some((position, size)) = snap { window = window.current_pos(position).fixed_size(size); }
        let shown = window.open(&mut self.trash_open).show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
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
        if let Some(shown) = shown {
            self.window_rects.insert("Trash".to_owned(), shown.response.rect);
            if shown.response.hovered() { self.mark_window_active(ctx, "Trash"); }
        }

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
            let mut confirm_open = true;
            let mut close_requested = false;
            let mut confirmed_id: Option<String> = None;
            egui::Window::new("Delete permanently?")
                .open(&mut confirm_open)
                .collapsible(false)
                .resizable(false)
                .fade_in(!self.settings.reduce_motion)
                .fade_out(!self.settings.reduce_motion)
                .show(ctx, |ui| {
                    ui.label("This item will be permanently deleted from NOVA Trash.");
                    ui.horizontal_wrapped(|ui| {
                        if ui.button("Delete permanently").clicked() {
                            confirmed_id = self.trash_selected.clone();
                            close_requested = true;
                        }
                        if ui.button("Cancel").clicked() { close_requested = true; }
                    });
                });
            self.trash_confirm_delete = confirm_open && !close_requested;
            if let Some(id) = confirmed_id {
                match trash::delete_permanently(&self.paths, &id) {
                    Ok(()) => self.status_message = "Item permanently deleted.".to_owned(),
                    Err(error) => self.status_message = format!("Permanent delete failed: {error}"),
                }
                self.refresh_trash();
            }
        }

        if self.trash_confirm_empty {
            let mut confirm_open = true;
            let mut close_requested = false;
            let mut empty_confirmed = false;
            egui::Window::new("Empty NOVA Trash?")
                .open(&mut confirm_open)
                .collapsible(false)
                .resizable(false)
                .fade_in(!self.settings.reduce_motion)
                .fade_out(!self.settings.reduce_motion)
                .show(ctx, |ui| {
                    ui.label("All items in NOVA Trash will be permanently deleted. This cannot be undone.");
                    ui.horizontal_wrapped(|ui| {
                        if ui.button("Empty Trash").clicked() { empty_confirmed = true; }
                        if ui.button("Cancel").clicked() { close_requested = true; }
                    });
                });
            self.trash_confirm_empty = confirm_open && !close_requested;
            if empty_confirmed {
                match trash::empty_trash(&self.paths) {
                    Ok(count) => self.status_message = format!("Permanently deleted {count} item(s) from NOVA Trash."),
                    Err(error) => self.status_message = format!("Could not empty Trash: {error}"),
                }
                self.refresh_trash();
                self.trash_confirm_empty = false;
            }
        }
    }


    fn calculator_window(&mut self, ctx: &egui::Context) {
        let snap = self.take_window_snap("Calculator", ctx);
        let mut window = egui::Window::new("NOVA Scientific Calculator")
            .order(self.window_order("Calculator"))
            .collapsible(false)
            .fade_in(!self.settings.reduce_motion)
            .fade_out(!self.settings.reduce_motion)
            .default_size([590.0, 650.0]);
        if let Some((position, size)) = snap {
            window = window.current_pos(position).fixed_size(size);
        } else {
            window = window.min_size([220.0, 220.0]);
        }
        let mut evaluate_now = false;
        let mut clear_now = false;
        let mut inserted: Option<&'static str> = None;
        let mut inserted_dynamic: Option<String> = None;
        let shown = window.open(&mut self.calculator_open).show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("CLASSWIZ-INSPIRED SCIENTIFIC WORKSPACE").small().strong().color(Color32::from_rgb(104, 167, 245)));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    egui::ComboBox::from_id_salt("nova-calc-angle")
                        .selected_text(self.calc_angle.label())
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.calc_angle, AngleUnit::Degree, "Degree (DEG)");
                            ui.selectable_value(&mut self.calc_angle, AngleUnit::Radian, "Radian (RAD)");
                            ui.selectable_value(&mut self.calc_angle, AngleUnit::Gradian, "Gradian (GRA)");
                        });
                });
            });
            ui.horizontal_wrapped(|ui| {
                egui::ComboBox::from_id_salt("nova-calc-mode")
                    .selected_text(&self.calc_mode)
                    .show_ui(ui, |ui| {
                        for mode in ["Calculate", "Statistics", "Equation", "Inequality", "Table", "Calculus", "Distribution", "Base-N", "Matrix", "Vector", "Ratio"] {
                            ui.selectable_value(&mut self.calc_mode, mode.to_owned(), mode);
                        }
                    });
                ui.label(RichText::new("Ans stores the previous numeric result").small().weak());
            });
            ui.separator();
            match self.calc_mode.as_str() {
                "Calculate" => {
                    ui.label("Expression");
                    let response = ui.add(TextEdit::singleline(&mut self.calc_expression)
                        .font(egui::TextStyle::Heading).desired_width(f32::INFINITY)
                        .hint_text("Example: sin(30)+sqrt(16)+nCr(8,2)"));
                    if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) { evaluate_now = true; }
                    ui.horizontal_wrapped(|ui| {
                        for (label, value) in [("sin", "sin("), ("cos", "cos("), ("tan", "tan("), ("√", "sqrt("), ("x²", "^2"), ("xʸ", "^"), ("π", "pi"), ("e", "e"), ("ln", "ln("), ("log", "log("), ("!", "!"), ("nCr", "nCr("), ("nPr", "nPr("), ("Ans", "ans")] {
                            if ui.button(label).clicked() { inserted = Some(value); }
                        }
                    });
                    ui.horizontal_wrapped(|ui| {
                        if ui.button("Calculate  ↵").clicked() { evaluate_now = true; }
                        if ui.button("Clear").clicked() { clear_now = true; }
                        if ui.button("⌫").clicked() { let _ = self.calc_expression.pop(); }
                    });
                    ui.horizontal_wrapped(|ui| {
                        if ui.button("MS").on_hover_text("Store the last answer in memory").clicked() {
                            self.calc_memory = self.calc_answer;
                            self.calc_memory_present = true;
                        }
                        if ui.button("M+").clicked() {
                            let updated = self.calc_memory + self.calc_answer;
                            if updated.is_finite() {
                                self.calc_memory = updated;
                                self.calc_memory_present = true;
                            } else {
                                self.calc_result = "Memory overflow: result was not stored.".to_owned();
                            }
                        }
                        if ui.button("M−").clicked() {
                            let updated = self.calc_memory - self.calc_answer;
                            if updated.is_finite() {
                                self.calc_memory = updated;
                                self.calc_memory_present = true;
                            } else {
                                self.calc_result = "Memory overflow: result was not stored.".to_owned();
                            }
                        }
                        if ui.add_enabled(self.calc_memory_present, egui::Button::new("MR")).clicked() {
                            inserted_dynamic = Some(format_number(self.calc_memory));
                        }
                        if ui.add_enabled(self.calc_memory_present, egui::Button::new("MC")).clicked() {
                            self.calc_memory = 0.0;
                            self.calc_memory_present = false;
                        }
                        ui.label(RichText::new(if self.calc_memory_present { format!("M = {}", format_number(self.calc_memory)) } else { "Memory empty".to_owned() }).small().weak());
                    });
                    ui.add_space(5.0);
                    egui::Frame::group(ui.style()).show(ui, |ui| {
                        ui.label(RichText::new("RESULT").small().weak());
                        ui.heading(RichText::new(&self.calc_result).size(29.0).color(Color32::from_rgb(105, 183, 255)));
                    });
                    if !self.calc_history.is_empty() {
                        ui.collapsing("Calculation history", |ui| {
                            for item in self.calc_history.iter().rev().take(8) { ui.label(RichText::new(item).monospace().small()); }
                        });
                    }
                    ui.label(RichText::new("Functions: trig/inverse trig, hyperbolic, log/ln, powers, roots, factorial, %, nCr/nPr, gcd/lcm, min/max, rounding, constants π/e/φ, implicit multiplication.").small().weak());
                }
                "Statistics" => {
                    ui.label("X data (separate values with commas or spaces)");
                    ui.add(TextEdit::multiline(&mut self.calc_stats_data).desired_rows(2).desired_width(f32::INFINITY));
                    ui.label("Y data (optional; same number of values for linear regression)");
                    ui.add(TextEdit::multiline(&mut self.calc_stats_y).desired_rows(2).desired_width(f32::INFINITY));
                    if ui.button("Analyze data").clicked() {
                        self.calc_result = match parse_number_list(&self.calc_stats_data).and_then(|values| calculator::statistics(&values).map(|stats|(values,stats))) {
                            Ok((values,s)) => {
                                let mut report=format!("n={}   Σx={}   x̄={}\nMedian={}   Min={}   Max={}\nσx={}   sx={}", s.count, format_number(s.sum), format_number(s.mean), format_number(s.median), format_number(s.min), format_number(s.max), format_number(s.population_sd), s.sample_sd.map(format_number).unwrap_or_else(|| "—".to_owned()));
                                if !self.calc_stats_y.trim().is_empty() { report.push_str("\n"); report.push_str(&linear_regression_report(&values,&self.calc_stats_y)); }
                                report
                            }
                            Err(error) => error,
                        };
                    }
                    ui.label(RichText::new("Includes one-variable statistics and optional two-variable linear regression (slope, intercept and correlation). ").small().weak());
                }
                "Equation" => {
                    ui.label("Solve a·x² + b·x + c = 0 (linear if a = 0)");
                    ui.horizontal_wrapped(|ui| {
                        ui.label("a"); ui.add(TextEdit::singleline(&mut self.calc_equation_a).desired_width(90.0));
                        ui.label("b"); ui.add(TextEdit::singleline(&mut self.calc_equation_b).desired_width(90.0));
                        ui.label("c"); ui.add(TextEdit::singleline(&mut self.calc_equation_c).desired_width(90.0));
                    });
                    if ui.button("Solve equation").clicked() {
                        self.calc_result = match (self.calc_equation_a.parse::<f64>(), self.calc_equation_b.parse::<f64>(), self.calc_equation_c.parse::<f64>()) {
                            (Ok(a),Ok(b),Ok(c)) => calculator::solve_quadratic(a,b,c).map(|roots| roots.iter().enumerate().map(|(i,x)| format!("x{} = {}",i+1,format_number(*x))).collect::<Vec<_>>().join("\n")).unwrap_or_else(|e|e),
                            _ => "Enter valid finite coefficients".to_owned(),
                        };
                    }
                }
                "Inequality" => {
                    ui.label("Solve a·x² + b·x + c  [operator]  0");
                    ui.horizontal_wrapped(|ui| {
                        ui.label("a"); ui.add(TextEdit::singleline(&mut self.calc_equation_a).desired_width(75.0));
                        ui.label("b"); ui.add(TextEdit::singleline(&mut self.calc_equation_b).desired_width(75.0));
                        ui.label("c"); ui.add(TextEdit::singleline(&mut self.calc_equation_c).desired_width(75.0));
                        egui::ComboBox::from_id_salt("nova-calc-inequality-op").selected_text(&self.calc_inequality_operator).show_ui(ui, |ui| { for op in [">",">=","<","<="] { ui.selectable_value(&mut self.calc_inequality_operator,op.to_owned(),op); } });
                    });
                    if ui.button("Solve inequality").clicked() {
                        self.calc_result = match (self.calc_equation_a.parse::<f64>(),self.calc_equation_b.parse::<f64>(),self.calc_equation_c.parse::<f64>()) {
                            (Ok(a),Ok(b),Ok(c)) => calculator::solve_quadratic_inequality(a,b,c,&self.calc_inequality_operator).unwrap_or_else(|e|e),
                            _ => "Enter valid finite coefficients".to_owned(),
                        };
                    }
                }
                "Calculus" => {
                    ui.label("Function f(x)");
                    ui.add(TextEdit::singleline(&mut self.calc_expression).desired_width(f32::INFINITY));
                    ui.horizontal_wrapped(|ui| { ui.label("Derivative at x"); ui.add(TextEdit::singleline(&mut self.calc_calculus_x).desired_width(80.0)); });
                    if ui.button("Differentiate numerically").clicked() {
                        self.calc_result = match self.calc_calculus_x.parse::<f64>() {
                            Ok(x) => calculator::numerical_derivative(&self.calc_expression,x,self.calc_angle,self.calc_answer).map(format_number).unwrap_or_else(|e|e),
                            Err(_) => "Enter a valid x value".to_owned(),
                        };
                    }
                    ui.separator();
                    ui.horizontal_wrapped(|ui| {
                        ui.label("Integral from"); ui.add(TextEdit::singleline(&mut self.calc_integral_start).desired_width(70.0));
                        ui.label("to"); ui.add(TextEdit::singleline(&mut self.calc_integral_end).desired_width(70.0));
                        ui.label("Steps"); ui.add(TextEdit::singleline(&mut self.calc_integral_steps).desired_width(70.0));
                    });
                    if ui.button("Integrate (Simpson)").clicked() {
                        self.calc_result = match (self.calc_integral_start.parse::<f64>(),self.calc_integral_end.parse::<f64>(),self.calc_integral_steps.parse::<usize>()) {
                            (Ok(a),Ok(b),Ok(n)) => calculator::numerical_integral(&self.calc_expression,a,b,n,self.calc_angle,self.calc_answer).map(format_number).unwrap_or_else(|e|e),
                            _ => "Enter valid integration bounds and an even step count".to_owned(),
                        };
                    }
                }
                "Distribution" => {
                    ui.label("Enter a distribution expression in Calculate syntax:");
                    ui.label(RichText::new("normalpdf(value,mean,sigma) · normalcdf(lower,upper,mean,sigma) · binompdf(n,p,k) · binomcdf(n,p,k) · poissonpdf(lambda,k) · poissoncdf(lambda,k)").monospace().small());
                    ui.label(RichText::new("Example: normalcdf(-1,1,0,1) or binompdf(10,0.5,4)").small().weak());
                    ui.add(TextEdit::singleline(&mut self.calc_expression).desired_width(f32::INFINITY));
                    if ui.button("Evaluate distribution").clicked() {
                        self.calc_result = calculator::evaluate(&self.calc_expression,EvalContext{angle:self.calc_angle,ans:self.calc_answer,x:0.0}).map(format_number).unwrap_or_else(|e|e);
                    }
                }
                "Table" => {
                    ui.label("Function f(x)");
                    ui.add(TextEdit::singleline(&mut self.calc_expression).desired_width(f32::INFINITY));
                    ui.horizontal_wrapped(|ui| {
                        ui.label("Start"); ui.add(TextEdit::singleline(&mut self.calc_table_start).desired_width(70.0));
                        ui.label("End"); ui.add(TextEdit::singleline(&mut self.calc_table_end).desired_width(70.0));
                        ui.label("Step"); ui.add(TextEdit::singleline(&mut self.calc_table_step).desired_width(70.0));
                    });
                    if ui.button("Generate table").clicked() {
                        self.calc_result = generate_table(&self.calc_expression, &self.calc_table_start, &self.calc_table_end, &self.calc_table_step, self.calc_angle, self.calc_answer);
                    }
                }
                "Base-N" => {
                    ui.label("Integer input");
                    ui.add(TextEdit::singleline(&mut self.calc_base_input).desired_width(f32::INFINITY));
                    egui::ComboBox::from_label("Input base").selected_text(self.calc_base_from.to_string()).show_ui(ui, |ui| {
                        for base in [2_u32,8,10,16] { ui.selectable_value(&mut self.calc_base_from,base,base.to_string()); }
                    });
                    if ui.button("Convert BIN / OCT / DEC / HEX").clicked() {
                        self.calc_result = calculator::convert_base(&self.calc_base_input,self.calc_base_from).unwrap_or_else(|e|e);
                    }
                }
                "Matrix" => {
                    ui.label("Matrix A (up to 3×3; rows separated by semicolons, values by commas)");
                    ui.add(TextEdit::multiline(&mut self.calc_matrix_a).desired_rows(2).desired_width(f32::INFINITY));
                    ui.label("Matrix B (for A × B)");
                    ui.add(TextEdit::multiline(&mut self.calc_matrix_b).desired_rows(2).desired_width(f32::INFINITY));
                    ui.horizontal_wrapped(|ui| {
                        if ui.button("det(A)").clicked() { self.calc_result = matrix_determinant(&self.calc_matrix_a).map(format_number).unwrap_or_else(|e|e); }
                        if ui.button("inverse(A)").clicked() { self.calc_result = matrix_inverse(&self.calc_matrix_a).unwrap_or_else(|e|e); }
                        if ui.button("A × B").clicked() { self.calc_result = matrix_multiply(&self.calc_matrix_a,&self.calc_matrix_b).unwrap_or_else(|e|e); }
                    });
                }
                "Vector" => {
                    ui.label("Vector A (2D or 3D; comma-separated)"); ui.add(TextEdit::singleline(&mut self.calc_vector_a).desired_width(f32::INFINITY));
                    ui.label("Vector B (same dimension)"); ui.add(TextEdit::singleline(&mut self.calc_vector_b).desired_width(f32::INFINITY));
                    if ui.button("Calculate dot / cross / magnitudes").clicked() { self.calc_result = vector_calculate(&self.calc_vector_a,&self.calc_vector_b); }
                }
                "Ratio" => {
                    ui.label("Solve a : b = c : x");
                    ui.horizontal_wrapped(|ui| { ui.add(TextEdit::singleline(&mut self.calc_ratio_a).desired_width(75.0)); ui.label(":"); ui.add(TextEdit::singleline(&mut self.calc_ratio_b).desired_width(75.0)); ui.label("="); ui.add(TextEdit::singleline(&mut self.calc_ratio_c).desired_width(75.0)); ui.label(": x"); });
                    if ui.button("Solve ratio").clicked() {
                        self.calc_result = match (self.calc_ratio_a.parse::<f64>(),self.calc_ratio_b.parse::<f64>(),self.calc_ratio_c.parse::<f64>()) { (Ok(a),Ok(b),Ok(c)) if a!=0.0 => format!("x = {}",format_number(b*c/a)), (Ok(_),_,_) => "The first ratio value cannot be zero".to_owned(), _ => "Enter valid ratio values".to_owned() };
                    }
                }
                _ => {}
            }
            if self.calc_mode != "Calculate" {
                ui.add_space(6.0);
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.label(RichText::new("RESULT").small().weak());
                    ui.label(RichText::new(&self.calc_result).monospace().color(Color32::from_rgb(105, 183, 255)));
                });
            }
        });
        if let Some(shown) = shown {
            self.window_rects.insert("Calculator".to_owned(), shown.response.rect);
            if shown.response.hovered() { self.mark_window_active(ctx, "Calculator"); }
        }
        if let Some(value) = inserted { self.calc_expression.push_str(value); }
        if let Some(value) = inserted_dynamic { self.calc_expression.push_str(&value); }
        if clear_now { self.calc_expression.clear(); self.calc_result = "0".to_owned(); }
        if evaluate_now {
            let context = EvalContext { angle: self.calc_angle, ans: self.calc_answer, x: 0.0 };
            match calculator::evaluate(&self.calc_expression, context) {
                Ok(value) => {
                    self.calc_answer = value;
                    self.calc_result = format_number(value);
                    self.calc_history.push(format!("{} = {}",self.calc_expression,self.calc_result));
                    if self.calc_history.len()>30 { self.calc_history.remove(0); }
                }
                Err(error) => self.calc_result = error,
            }
        }
    }

    fn monitor_window(&mut self, ctx: &egui::Context) {
        if self.monitor_open && self.monitor.is_none() && self.monitor_error.is_none() {
            match System::new() {
                Ok(system) => self.monitor = Some(system),
                Err(error) => self.monitor_error = Some(format!("Hardware Center unavailable: {error:?}")),
            }
        }
        if self.monitor_open && self.hardware.is_none() {
            self.hardware = Some(HardwareTelemetry::new());
        }
        if self.monitor_open {
            if let Some(hardware) = self.hardware.as_mut() {
                if hardware.refresh() {
                    self.hardware_snapshot = hardware.snapshot().clone();
                }
            }
        }

        let metrics = if self.monitor_open {
            if let Some(monitor) = self.monitor.as_mut() {
                if self.last_monitor_refresh.elapsed() >= Duration::from_secs(1) {
                    monitor.refresh_cpu_usage();
                    monitor.refresh_memory();
                    if self.monitor_cpu_baseline_captured {
                        self.monitor_cpu_sample_ready = true;
                    } else {
                        self.monitor_cpu_baseline_captured = true;
                    }
                    self.last_monitor_refresh = Instant::now();
                }
                if self.last_cpu_frequency_refresh.elapsed() >= Duration::from_secs(5) {
                    monitor.refresh_cpu_frequency();
                    self.last_cpu_frequency_refresh = Instant::now();
                }
                if self.last_process_refresh.elapsed() >= Duration::from_secs(5) {
                    monitor.refresh_processes_specifics(
                        ProcessesToUpdate::All,
                        true,
                        ProcessRefreshKind::nothing().with_cpu().with_memory().without_tasks(),
                    );
                    let mut refreshed_rows: Vec<ProcessMetric> = monitor.processes().iter().map(|(pid, process)| ProcessMetric {
                        pid: pid.as_u32().to_string(),
                        name: process.name().to_string_lossy().into_owned(),
                        cpu_percent: {
                            let usage = process.cpu_usage();
                            if usage.is_finite() { usage.clamp(0.0, 100.0) } else { 0.0 }
                        },
                        memory_bytes: process.memory(),
                    }).collect();
                    refreshed_rows.sort_by(|a, b| {
                        b.cpu_percent.total_cmp(&a.cpu_percent)
                            .then_with(|| b.memory_bytes.cmp(&a.memory_bytes))
                    });
                    refreshed_rows.truncate(30);
                    self.monitor_process_rows = refreshed_rows;
                    if self.monitor_process_baseline_captured {
                        self.monitor_process_cpu_ready = true;
                    } else {
                        self.monitor_process_baseline_captured = true;
                    }
                    self.last_process_refresh = Instant::now();
                }
                let reported_cpu = monitor.global_cpu_usage();
                let cpu = if reported_cpu.is_finite() { reported_cpu.clamp(0.0, 100.0) } else { 0.0 };
                let used = monitor.used_memory() as f64 / 1024.0 / 1024.0 / 1024.0;
                let total = monitor.total_memory() as f64 / 1024.0 / 1024.0 / 1024.0;
                let swap_used = monitor.used_swap() as f64 / 1024.0 / 1024.0 / 1024.0;
                let swap_total = monitor.total_swap() as f64 / 1024.0 / 1024.0 / 1024.0;
                let process_count = monitor.processes().len();
                let process_rows = self.monitor_process_rows.clone();
                let cpu_name = monitor.cpus().first().map(|cpu| cpu.brand().trim().to_owned()).filter(|name| !name.is_empty());
                let cpu_cores: Vec<CpuCoreMetric> = monitor.cpus().iter().enumerate().map(|(index, cpu)| {
                    let reported_name = cpu.name().trim();
                    CpuCoreMetric {
                        name: if reported_name.is_empty() { format!("CPU {}", index + 1) } else { reported_name.to_owned() },
                        usage_percent: cpu.usage().clamp(0.0, 100.0),
                        frequency_mhz: cpu.frequency(),
                    }
                }).collect();
                Some((cpu, used, total, swap_used, swap_total, process_count, process_rows, cpu_name, cpu_cores, self.monitor_cpu_sample_ready, self.monitor_process_cpu_ready))
            } else {
                None
            }
        } else {
            None
        };
        let monitor_error = self.monitor_error.clone();

        let snap = self.take_window_snap("Monitor", ctx);
        let mut window = egui::Window::new("NOVA Hardware Center")
            .order(self.window_order("Monitor"))
            .collapsible(false)
            .fade_in(!self.settings.reduce_motion)
            .fade_out(!self.settings.reduce_motion)
            .default_size([760.0, 570.0]);
        if let Some((position, size)) = snap { window = window.current_pos(position).fixed_size(size); }
        let shown = window.open(&mut self.monitor_open).show(ctx, |ui| {
            let Some((cpu, used, total, swap_used, swap_total, process_count, process_rows, cpu_name, cpu_cores, cpu_sample_ready, process_cpu_ready)) = metrics.as_ref() else {
                ui.colored_label(Color32::LIGHT_RED, monitor_error.as_deref().unwrap_or("System information is unavailable on this device."));
                return;
            };

            let snapshot = &self.hardware_snapshot;
            ui.horizontal_wrapped(|ui| {
                let tabs = ["Overview", "Processes", "Storage", "Network", "GPU & Power", "Sensors & Controls"];
                for (index, label) in tabs.iter().enumerate() {
                    if ui.selectable_label(self.monitor_tab == index, *label).clicked() {
                        self.monitor_tab = index;
                    }
                }
                if ui.button("↻ Refresh").on_hover_text("Refresh hardware telemetry").clicked() {
                    if let Some(hardware) = self.hardware.as_mut() {
                        hardware.request_refresh();
                    }
                    self.last_monitor_refresh = Instant::now().checked_sub(Duration::from_secs(2)).unwrap_or_else(Instant::now);
                }
            });
            ui.separator();

            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                match self.monitor_tab {
                    0 => {
                        ui.horizontal_wrapped(|ui| {
                            ui.vertical(|ui| {
                                ui.label(RichText::new("CPU LOAD").small().weak().strong());
                                ui.heading(if *cpu_sample_ready { format!("{cpu:.1}%") } else { "Sampling…".to_owned() });
                            });
                            ui.separator();
                            ui.vertical(|ui| {
                                ui.label(RichText::new("MEMORY").small().weak().strong());
                                ui.heading(format!("{used:.2} / {total:.2} GiB"));
                            });
                            ui.separator();
                            ui.vertical(|ui| {
                                ui.label(RichText::new("PROCESSES").small().weak().strong());
                                ui.heading(process_count.to_string());
                            });
                        });
                        if *cpu_sample_ready {
                            ui.add(egui::ProgressBar::new((*cpu / 100.0).clamp(0.0, 1.0)).show_percentage());
                        } else {
                            ui.label("Collecting the first CPU sample; usage appears after the next refresh.");
                        }
                        ui.add_space(6.0);
                        let ram_fraction = if *total > 0.0 { (*used / *total) as f32 } else { 0.0 };
                        ui.add(egui::ProgressBar::new(ram_fraction.clamp(0.0, 1.0)).text("RAM"));
                        if *swap_total > 0.0 {
                            ui.label(format!("Swap: {swap_used:.2} / {swap_total:.2} GiB"));
                            ui.add(egui::ProgressBar::new(((*swap_used / *swap_total) as f32).clamp(0.0, 1.0)).text("Page file / swap"));
                        } else {
                            ui.label("Page file / swap: unavailable or disabled");
                        }
                        ui.add_space(12.0);
                        ui.heading("Processor");
                        ui.label(cpu_name.as_deref().unwrap_or("CPU model unavailable"));
                        ui.label(format!("{} logical processor(s)", cpu_cores.len()));
                        egui::CollapsingHeader::new("Per-core usage and frequency")
                            .default_open(false)
                            .show(ui, |ui| {
                                if cpu_cores.is_empty() {
                                    ui.label("Per-core telemetry is unavailable.");
                                }
                                egui::Grid::new("nova-hardware-cpu-core-grid").striped(true).show(ui, |ui| {
                                    ui.strong("Core"); ui.strong("Usage"); ui.strong("Frequency"); ui.end_row();
                                    for core in cpu_cores {
                                        ui.label(&core.name);
                                        if *cpu_sample_ready {
                                            ui.add(egui::ProgressBar::new((core.usage_percent / 100.0).clamp(0.0, 1.0)).desired_width(110.0).text(format!("{:.1}%", core.usage_percent)));
                                        } else {
                                            ui.label("Sampling…");
                                        }
                                        ui.label(if core.frequency_mhz > 0 { format!("{} MHz", core.frequency_mhz) } else { "—".to_owned() });
                                        ui.end_row();
                                    }
                                });
                            });
                        let os_name = System::name().unwrap_or_else(|| "Windows host".to_owned());
                        let os_version = System::os_version().unwrap_or_default();
                        ui.label(if os_version.is_empty() { os_name } else { format!("{os_name} {os_version}") });
                        ui.add_space(12.0);
                        ui.heading("Storage overview");
                        if snapshot.disks.is_empty() {
                            ui.label(if snapshot.disk_provider_available { "No mounted storage volumes were reported." } else { "Storage telemetry is unavailable on this host." });
                        } else {
                            for disk in &snapshot.disks {
                                let used_bytes = disk.total_bytes.saturating_sub(disk.available_bytes);
                                let fraction = if disk.total_bytes == 0 { 0.0 } else { used_bytes as f32 / disk.total_bytes as f32 };
                                ui.label(format!("{}  ·  {} free of {}", disk.mount_point, hardware::format_bytes(disk.available_bytes), hardware::format_bytes(disk.total_bytes)));
                                ui.add(egui::ProgressBar::new(fraction.clamp(0.0, 1.0)).show_percentage());
                            }
                        }
                        ui.add_space(10.0);
                        ui.heading("Network throughput");
                        let rates_ready = !snapshot.networks.is_empty() && snapshot.networks.iter().all(|network| network.rate_available);
                        if snapshot.networks.is_empty() {
                            ui.label(if snapshot.network_provider_available { "No active network interfaces were reported." } else { "Network telemetry is unavailable on this host." });
                        } else if rates_ready {
                            let down: f64 = snapshot.networks.iter().map(|network| network.received_per_sec).sum();
                            let up: f64 = snapshot.networks.iter().map(|network| network.transmitted_per_sec).sum();
                            ui.horizontal_wrapped(|ui| {
                                ui.label(format!("↓ {}", hardware::format_rate(down)));
                                ui.separator();
                                ui.label(format!("↑ {}", hardware::format_rate(up)));
                            });
                        } else {
                            ui.label("Network throughput: sampling baseline…");
                        }
                        if let Some(battery) = &snapshot.battery {
                            ui.add_space(10.0);
                            ui.heading("Power");
                            let charge = match battery.battery_present {
                                Some(true) => battery.charge_percent.map(|value| format!("{value}%")).unwrap_or_else(|| "Unknown".to_owned()),
                                Some(false) => "No battery".to_owned(),
                                None => "Battery status unknown".to_owned(),
                            };
                            ui.label(format!("Battery: {charge}{}", if battery.charging { " · Charging" } else { "" }));
                        }
                    }
                    1 => {
                        ui.heading(if *process_cpu_ready { "Top processes by CPU" } else { "Processes" });
                        ui.label(RichText::new(if *process_cpu_ready { "Process readings refresh every 5 seconds. Values are reported by the Windows host." } else { "Collecting a baseline; process CPU rankings become accurate after the next five-second refresh." }).small().weak());
                        egui::Grid::new("nova-process-grid").striped(true).min_col_width(48.0).show(ui, |ui| {
                            ui.strong("PID"); ui.strong("Process"); ui.strong("CPU"); ui.strong("Memory"); ui.end_row();
                            for process in process_rows {
                                ui.monospace(&process.pid);
                                ui.label(RichText::new(&process.name).size(12.0));
                                ui.label(if *process_cpu_ready { format!("{:.1}%", process.cpu_percent) } else { "Sampling…".to_owned() });
                                ui.label(hardware::format_bytes(process.memory_bytes));
                                ui.end_row();
                            }
                        });
                        if process_rows.is_empty() { ui.label("No process data is available yet."); }
                    }
                    2 => {
                        ui.heading("Storage volumes");
                        if snapshot.disks.is_empty() {
                            ui.label(if snapshot.disk_provider_available { "No mounted volumes were reported." } else { "Storage telemetry is unavailable on this host." });
                        }
                        for disk in &snapshot.disks {
                            let used_bytes = disk.total_bytes.saturating_sub(disk.available_bytes);
                            let fraction = if disk.total_bytes == 0 { 0.0 } else { used_bytes as f32 / disk.total_bytes as f32 };
                            egui::Frame::group(ui.style()).show(ui, |ui| {
                                ui.horizontal_wrapped(|ui| {
                                    ui.heading(&disk.mount_point);
                                    ui.label(RichText::new(if disk.removable { "Removable" } else { "Fixed / mounted" }).small().weak());
                                    if disk.read_only { ui.label(RichText::new("Read-only").small().color(Color32::YELLOW)); }
                                });
                                ui.label(format!("{}  ·  {}", disk.name, if disk.file_system.is_empty() { "Filesystem unknown" } else { &disk.file_system }));
                                ui.add(egui::ProgressBar::new(fraction.clamp(0.0, 1.0)).show_percentage());
                                ui.label(format!("{} used  ·  {} available  ·  {} total", hardware::format_bytes(used_bytes), hardware::format_bytes(disk.available_bytes), hardware::format_bytes(disk.total_bytes)));
                            });
                        }
                        ui.label(RichText::new("Disk activity counters are not shown as utilization because their availability varies by driver and storage controller.").small().weak());
                    }
                    3 => {
                        ui.heading("Network interfaces");
                        if snapshot.networks.is_empty() {
                            ui.label(if snapshot.network_provider_available { "No active network interfaces were reported." } else { "Network telemetry is unavailable on this host." });
                        }
                        for network in &snapshot.networks {
                            egui::Frame::group(ui.style()).show(ui, |ui| {
                                ui.heading(&network.name);
                                ui.horizontal_wrapped(|ui| {
                                    if network.rate_available {
                                        ui.label(format!("↓ {}", hardware::format_rate(network.received_per_sec)));
                                        ui.separator();
                                        ui.label(format!("↑ {}", hardware::format_rate(network.transmitted_per_sec)));
                                    } else {
                                        ui.label("Throughput: sampling baseline…");
                                    }
                                });
                                ui.label(format!("Received {} total  ·  Sent {} total", hardware::format_bytes(network.total_received), hardware::format_bytes(network.total_transmitted)));
                            });
                        }
                        ui.label(RichText::new("Traffic is sampled per interface; link speed and Wi-Fi signal strength are not inferred from throughput.").small().weak());
                    }
                    4 => {
                        ui.heading("Graphics adapters");
                        if snapshot.gpus.is_empty() {
                            ui.label(if snapshot.gpu_provider_available { "No GPU adapters were reported." } else { "GPU telemetry provider is unavailable. Check the graphics driver and Windows support." });
                        }
                        for gpu in &snapshot.gpus {
                            egui::Frame::group(ui.style()).show(ui, |ui| {
                                ui.heading(&gpu.name);
                                if let Some(usage) = gpu.usage_percent {
                                    ui.label(format!("GPU engine usage: {usage:.1}%"));
                                    ui.add(egui::ProgressBar::new((usage / 100.0).clamp(0.0, 1.0)).show_percentage());
                                } else {
                                    ui.label("GPU usage: waiting for driver sample / unavailable");
                                }
                                match (gpu.used_memory, gpu.total_memory) {
                                    (Some(used), Some(total)) if total > 0 => {
                                        ui.label(format!("Dedicated VRAM: {} / {}", hardware::format_bytes(used), hardware::format_bytes(total)));
                                        ui.add(egui::ProgressBar::new(((used as f64 / total as f64) as f32).clamp(0.0, 1.0)).show_percentage());
                                    }
                                    (_, Some(total)) => { ui.label(format!("Dedicated VRAM total: {} · usage unavailable", hardware::format_bytes(total))); }
                                    _ => { ui.label("Dedicated VRAM counters are not exposed by this adapter/driver."); }
                                }
                            });
                        }
                        ui.add_space(10.0);
                        ui.heading("Battery and power");
                        if let Some(battery) = &snapshot.battery {
                            let ac = match battery.ac_online { Some(true) => "AC power", Some(false) => "Battery power", None => "Power source unknown" };
                            ui.label(ac);
                            match battery.battery_present {
                                Some(true) => {
                                    ui.label(format!("Charge: {}{}", battery.charge_percent.map(|value| format!("{value}%")).unwrap_or_else(|| "Unknown".to_owned()), if battery.charging { " · Charging" } else { "" }));
                                    if let Some(seconds) = battery.remaining_seconds { ui.label(format!("Estimated remaining time: {}", hardware::format_duration(seconds))); }
                                }
                                Some(false) => { ui.label("No battery is reported by Windows."); }
                                None => { ui.label("Windows could not determine whether a battery is present."); }
                            }
                            ui.label(format!("Battery saver: {}", if battery.battery_saver { "On" } else { "Off" }));
                        } else {
                            ui.label("Windows power status is unavailable on this device.");
                        }
                    }
                    _ => {
                        ui.heading("Temperature sensors");
                        if snapshot.sensors.is_empty() {
                            ui.label(if snapshot.sensor_provider_available { "No temperature sensors are exposed by the current driver/firmware provider." } else { "Temperature sensor provider is unavailable." });
                        }
                        for sensor in &snapshot.sensors {
                            ui.horizontal(|ui| {
                                ui.label(&sensor.name);
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| { ui.strong(format!("{:.1} °C", sensor.temperature_c)); });
                            });
                        }
                        ui.separator();
                        ui.heading("Windows hardware controls");
                        ui.label(RichText::new("These buttons open Windows' own controls. NOVA does not emulate or override device-specific controls when Windows/driver support is required.").small().weak());
                        ui.horizontal_wrapped(|ui| {
                            if ui.button("Display / brightness").clicked() { self.status_message = hardware_control_status(HostControl::Display); }
                            if ui.button("Sound / volume").clicked() { self.status_message = hardware_control_status(HostControl::Sound); }
                            if ui.button("Network").clicked() { self.status_message = hardware_control_status(HostControl::Network); }
                            if ui.button("Bluetooth").clicked() { self.status_message = hardware_control_status(HostControl::Bluetooth); }
                            if ui.button("Power settings").clicked() { self.status_message = hardware_control_status(HostControl::Power); }
                        });
                    }
                }
            });
            ui.separator();
            ui.label(RichText::new("Telemetry is read from the Windows host. GPU and temperature availability depends on the installed driver/firmware; unsupported readings are shown as unavailable.").small().weak());
        });
        if let Some(shown) = shown {
            self.window_rects.insert("Monitor".to_owned(), shown.response.rect);
            if shown.response.hovered() { self.mark_window_active(ctx, "Monitor"); }
        }
    }

}

fn hardware_control_status(control: HostControl) -> String {
    match hardware::open_host_control(control) {
        Ok(()) => format!("Opened Windows {control:?} settings."),
        Err(error) => format!("Could not open Windows {control:?} settings: {error}"),
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
        self.poll_security_tool_launch();
        let close_requested = ctx.input(|input| input.viewport().close_requested());
        if close_requested && self.editor_dirty {
            // Cancel the host close until the document has been saved or the
            // user explicitly discards it in the Editor confirmation dialog.
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.request_editor_action(PendingEditorAction::ExitApplication);
        }
        let (toggle_start, open_files, open_monitor, close_panels, save_editor_shortcut) = ctx.input(|input| (
            input.modifiers.ctrl && input.key_pressed(egui::Key::Space),
            input.modifiers.ctrl && input.key_pressed(egui::Key::E),
            input.modifiers.ctrl && input.modifiers.shift && input.key_pressed(egui::Key::Escape),
            input.key_pressed(egui::Key::Escape),
            self.editor_open && input.modifiers.ctrl && input.key_pressed(egui::Key::S),
        ));
        if toggle_start { self.start_open = !self.start_open; self.notification_open = false; self.task_view_open = false; }
        if save_editor_shortcut && self.editor_overwrite_pending.is_none() {
            if self.editor_pending_action.is_some() {
                self.resolve_editor_pending_action(EditorDecision::Save);
            } else if let Err(error) = self.save_editor_document(false) {
                self.editor_status = error;
            }
        }
        if open_files { self.open_app("Files"); }
        if open_monitor { self.open_app("Monitor"); }
        if close_panels {
            if self.security_confirm_clear_quarantine {
                self.security_confirm_clear_quarantine = false;
            } else if self.trash_confirm_delete {
                self.trash_confirm_delete = false;
            } else if self.trash_confirm_empty {
                self.trash_confirm_empty = false;
            } else {
                self.start_open = false;
                self.notification_open = false;
                self.task_view_open = false;
            }
        }

        let dark = self.is_dark_mode(&ctx);
        let desktop_fill = if dark { Color32::from_rgb(12, 20, 36) } else { Color32::from_rgb(222, 232, 247) };
        egui::Frame::new()
            .fill(if dark { Color32::from_rgb(9, 14, 25) } else { Color32::from_rgb(231, 237, 247) })
            .inner_margin(egui::Margin::same(8))
            .show(ui, |ui| {
                ui.vertical(|ui| {
                    let taskbar_height = 60.0;
                    let available_height = ui.available_height().max(1.0);
                    let desktop_outer_height = (available_height - taskbar_height - 8.0).max(120.0);
                    // Keep the dock visible and let the desktop contract on smaller viewports.
                    let desktop_content_height = (desktop_outer_height - 36.0).max(84.0);
                    egui::Frame::new()
                        .fill(desktop_fill)
                        .stroke(egui::Stroke::new(1.0, if dark { Color32::from_white_alpha(18) } else { Color32::from_black_alpha(14) }))
                        .corner_radius(egui::CornerRadius::same(16))
                        .inner_margin(egui::Margin::same(18))
                        .show(ui, |ui| {
                            ui.set_min_size(egui::vec2(ui.available_width(), desktop_content_height));
                            let rect = ui.max_rect();
                            let desktop_size = rect.size();
                            if desktop_layout_changed(self.desktop_last_size, desktop_size) {
                                // Re-seed shortcut Areas from their normalized Settings
                                // positions after a resize; otherwise egui retains old pixels.
                                self.desktop_layout_generation = self.desktop_layout_generation.wrapping_add(1);
                            }
                            self.desktop_last_size = Some(desktop_size);
                            self.desktop_rect = Some(rect);
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
                                ui.horizontal_wrapped(|ui| {
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

                                ui.label(RichText::new("Right-click shortcuts to pin apps · Drag icons to arrange the desktop").small().weak());

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

                    // Reserve the dock's footprint. It is painted in a foreground Area
                    // after workspace windows so large windows cannot cover the taskbar.
                    ui.add_space(8.0);
                });
            });

        if let Some(desktop_rect) = self.desktop_rect {
            self.desktop_shortcuts(&ctx, desktop_rect);
        }

        // Draw app windows first. Shell surfaces are rendered afterward so Start,
        // Quick Settings and the floating dock stay above every workspace window.
        self.terminal_window(&ctx);
        self.files_window(&ctx);
        self.editor_window(&ctx);
        if self.exit_after_editor {
            self.exit_after_editor = false;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        self.settings_window(&ctx);
        self.calculator_window(&ctx);
        self.monitor_window(&ctx);
        self.trash_window(&ctx);
        self.unikey_window(&ctx);
        self.security_window(&ctx);
        for info in &security_tools::TOOLS {
            self.security_tool_window(&ctx, info.tool);
        }
        if self.security_launch_receiver.is_some() {
            // Poll the bounded Authenticode check without blocking the UI thread.
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        self.prune_closed_window_state();
        self.capture_status_activity();
        self.window_switcher(&ctx);
        self.notification_center(&ctx);
        self.start_menu(&ctx);
        egui::Area::new(egui::Id::unique("nova-taskbar-floating-dock"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -8.0))
            .show(&ctx, |ui| {
                let dock_width = (ui.ctx().content_rect().width() - 16.0).max(1.0);
                ui.set_width(dock_width);
                self.taskbar(ui);
            });
        if self.monitor_open {
            // The monitor data is refreshed once per second; faster frames add no information.
            ctx.request_repaint_after(Duration::from_secs(1));
        }
    }
}


#[cfg(windows)]
fn local_clock_strings() -> (String, String) {
    #[repr(C)]
    struct SystemTime {
        year: u16,
        month: u16,
        day_of_week: u16,
        day: u16,
        hour: u16,
        minute: u16,
        second: u16,
        milliseconds: u16,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetLocalTime(time: *mut SystemTime);
    }
    let mut value = SystemTime {
        year: 0, month: 0, day_of_week: 0, day: 0,
        hour: 0, minute: 0, second: 0, milliseconds: 0,
    };
    // Windows supplies local time using the host's configured time zone and locale settings.
    unsafe { GetLocalTime(&mut value); }
    (format!("{:02}:{:02}", value.hour, value.minute), format!("{:02}/{:02}/{:04}", value.day, value.month, value.year))
}

#[cfg(not(windows))]
fn local_clock_strings() -> (String, String) {
    // Non-Windows development builds still render a stable UTC clock without adding a date dependency.
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let minute = (seconds / 60) % 1440;
    let hour = minute / 60;
    let min = minute % 60;
    let days = (seconds / 86_400) as i64;
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let mut year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    year += if month <= 2 { 1 } else { 0 };
    (format!("{:02}:{:02}", hour, min), format!("{:02}/{:02}/{:04} UTC", month, day, year))
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

fn format_matrix_number(value: f64) -> String {
    if !value.is_finite() || value == 0.0 || value.abs() > 1.0e12 {
        return format_number(value);
    }

    // Gauss-Jordan elimination can produce values such as -1.9999999999999996.
    // Clean only values within a few ULPs of a 12-decimal display value. Avoid
    // rounding to zero so genuinely small matrix entries remain visible.
    const DISPLAY_SCALE: f64 = 1.0e12;
    let rounded = (value * DISPLAY_SCALE).round() / DISPLAY_SCALE;
    let tolerance = value.abs().max(1.0) * f64::EPSILON * 2.0;
    if rounded != 0.0 && (value - rounded).abs() <= tolerance {
        format_number(rounded)
    } else {
        format_number(value)
    }
}

fn parse_number_list(input: &str) -> Result<Vec<f64>, String> {
    let values = input.split(|ch: char| ch == ',' || ch == ';' || ch.is_whitespace())
        .filter(|part| !part.trim().is_empty())
        .map(|part| part.trim().parse::<f64>().map_err(|_| format!("Invalid number: {part}")))
        .collect::<Result<Vec<_>, _>>()?;
    if values.is_empty() || values.iter().any(|value| !value.is_finite()) {
        return Err("Enter at least one finite number".to_owned());
    }
    Ok(values)
}

fn linear_regression_report(x:&[f64], y_input:&str)->String {
    let y=match parse_number_list(y_input){Ok(values)=>values,Err(error)=>return error};
    if x.len()!=y.len()||x.len()<2{return "Regression requires equally sized X/Y lists with at least two pairs".to_owned();}
    let n=x.len() as f64;let mean_x=x.iter().sum::<f64>()/n;let mean_y=y.iter().sum::<f64>()/n;
    let sxx=x.iter().map(|v|(v-mean_x).powi(2)).sum::<f64>();let syy=y.iter().map(|v|(v-mean_y).powi(2)).sum::<f64>();
    let sxy=x.iter().zip(&y).map(|(a,b)|(a-mean_x)*(b-mean_y)).sum::<f64>();
    if sxx==0.0||syy==0.0{return "Regression is undefined when X or Y has zero variance".to_owned();}
    let slope=sxy/sxx;let intercept=mean_y-slope*mean_x;let correlation=sxy/(sxx*syy).sqrt();
    if !slope.is_finite()||!intercept.is_finite()||!correlation.is_finite(){return "Regression result is outside the supported range".to_owned();}
    format!("Linear regression\ny = {}x + {}\nr = {}",format_number(slope),format_number(intercept),format_number(correlation))
}

fn generate_table(expression: &str, start: &str, end: &str, step: &str, angle: AngleUnit, ans: f64) -> String {
    let parsed = (start.parse::<f64>(), end.parse::<f64>(), step.parse::<f64>());
    let (Ok(start), Ok(end), Ok(step)) = parsed else { return "Enter valid start, end and step values".to_owned(); };
    if !start.is_finite() || !end.is_finite() || !step.is_finite() || step == 0.0 {
        return "Start/end/step must be finite and step cannot be zero".to_owned();
    }
    if (end - start).signum() != step.signum() && end != start {
        return "Step direction must move from start toward end".to_owned();
    }
    let count = ((end - start) / step).floor().abs() as usize + 1;
    if count > 101 { return "Table is limited to 101 rows per calculation".to_owned(); }
    let mut output = String::from("       x                 f(x)\n");
    for index in 0..count {
        let x = start + index as f64 * step;
        if (step > 0.0 && x > end + step.abs() * 1e-10) || (step < 0.0 && x < end - step.abs() * 1e-10) { break; }
        let value = calculator::evaluate(expression, EvalContext { angle, ans, x });
        match value {
            Ok(value) => output.push_str(&format!("{:>12}   {:>16}\n", format_number(x), format_number(value))),
            Err(error) => output.push_str(&format!("{:>12}   {error}\n", format_number(x))),
        }
    }
    output
}

fn parse_square_matrix(input: &str) -> Result<Vec<Vec<f64>>, String> {
    let rows = input.split(';').map(parse_number_list).collect::<Result<Vec<_>, _>>()?;
    if rows.len() < 2 || rows.len() > 3 || rows.iter().any(|row| row.len() != rows.len()) {
        return Err("Matrix must be square, either 2×2 or 3×3".to_owned());
    }
    Ok(rows)
}

fn matrix_determinant(input: &str) -> Result<f64, String> {
    let m = parse_square_matrix(input)?;
    let det = if m.len() == 2 { m[0][0] * m[1][1] - m[0][1] * m[1][0] }
        else { m[0][0]*(m[1][1]*m[2][2]-m[1][2]*m[2][1]) - m[0][1]*(m[1][0]*m[2][2]-m[1][2]*m[2][0]) + m[0][2]*(m[1][0]*m[2][1]-m[1][1]*m[2][0]) };
    if det.is_finite() { Ok(det) } else { Err("Matrix determinant is outside the supported range".to_owned()) }
}

fn matrix_inverse(input: &str) -> Result<String, String> {
    let matrix = parse_square_matrix(input)?;
    let n = matrix.len();
    let mut augmented = vec![vec![0.0; n * 2]; n];
    for row in 0..n { for col in 0..n { augmented[row][col] = matrix[row][col]; } augmented[row][n + row] = 1.0; }
    for pivot in 0..n {
        let best = (pivot..n).max_by(|&a,&b| augmented[a][pivot].abs().total_cmp(&augmented[b][pivot].abs())).unwrap_or(pivot);
        if augmented[best][pivot].abs() < 1e-12 { return Err("Matrix is singular and has no inverse".to_owned()); }
        augmented.swap(pivot,best);
        let divisor = augmented[pivot][pivot];
        for col in 0..n*2 { augmented[pivot][col] /= divisor; }
        for row in 0..n { if row != pivot { let factor=augmented[row][pivot]; for col in 0..n*2 { augmented[row][col] -= factor*augmented[pivot][col]; } } }
    }
    Ok((0..n).map(|row| (0..n).map(|col| format_matrix_number(augmented[row][n+col])).collect::<Vec<_>>().join(", ")).collect::<Vec<_>>().join("\n"))
}

fn matrix_multiply(a: &str, b: &str) -> Result<String, String> {
    let a = parse_square_matrix(a)?; let b = parse_square_matrix(b)?;
    if a.len() != b.len() { return Err("Matrices must have the same dimension".to_owned()); }
    let n=a.len(); let mut out=vec![vec![0.0;n];n];
    for row in 0..n { for col in 0..n { for k in 0..n { out[row][col] += a[row][k]*b[k][col]; } } }
    if out.iter().flatten().any(|v|!v.is_finite()) { return Err("Matrix result is outside the supported range".to_owned()); }
    Ok(out.iter().map(|row|row.iter().map(|v|format_matrix_number(*v)).collect::<Vec<_>>().join(", ")).collect::<Vec<_>>().join("\n"))
}

fn vector_calculate(a: &str, b: &str) -> String {
    let (a,b) = match (parse_number_list(a),parse_number_list(b)) { (Ok(a),Ok(b)) if a.len()==b.len() && (2..=3).contains(&a.len()) => (a,b), _ => return "Enter two vectors with matching 2D or 3D dimensions".to_owned() };
    let dot: f64 = a.iter().zip(&b).map(|(x,y)|x*y).sum();
    let mag_a=a.iter().map(|v|v*v).sum::<f64>().sqrt(); let mag_b=b.iter().map(|v|v*v).sum::<f64>().sqrt();
    let mut result=format!("A·B = {}\n|A| = {}\n|B| = {}",format_number(dot),format_number(mag_a),format_number(mag_b));
    if a.len()==3 { let cross=[a[1]*b[2]-a[2]*b[1],a[2]*b[0]-a[0]*b[2],a[0]*b[1]-a[1]*b[0]]; result.push_str(&format!("\nA×B = ({}, {}, {})",format_number(cross[0]),format_number(cross[1]),format_number(cross[2]))); }
    result
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use eframe::egui;
    use super::{compact_file_name, desktop_default_position, desktop_layout_changed, desktop_shortcut_travel, format_number, move_pinned_app, normalize_desktop_positions, normalize_pinned_apps, start_menu_dimensions, taskbar_rail_widths, workspace_is_active, START_APPS, START_PINS};
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
    fn desktop_shortcuts_reference_only_catalog_apps_without_duplicates() {
        for (index, name) in super::DESKTOP_SHORTCUTS.iter().enumerate() {
            assert!(START_APPS.iter().any(|app| app.name == *name));
            assert!(!super::DESKTOP_SHORTCUTS[..index].contains(name));
        }
    }

    #[test]
    fn taskbar_marks_only_the_foreground_open_window_as_active() {
        assert!(workspace_is_active(Some("Files"), "Files", true));
        assert!(!workspace_is_active(Some("Files"), "Terminal", true));
        assert!(!workspace_is_active(Some("Files"), "Files", false));
        assert!(!workspace_is_active(None, "Files", true));
    }

    #[test]
    fn taskbar_rails_never_exceed_available_width() {
        for width in [0.0, 48.0, 160.0, 320.0, 440.0, 520.0, 760.0, 900.0, 1280.0, 2560.0] {
            let (left, center, right, _, _) = taskbar_rail_widths(width);
            assert!(left.is_finite() && center.is_finite() && right.is_finite());
            assert!(left >= 0.0 && center >= 0.0 && right >= 0.0);
            assert!((left + center + right - width.max(0.0)).abs() < 0.01);
            assert!((left - right).abs() < 0.01);
        }
    }

    #[test]
    fn start_menu_stays_within_the_viewport_and_uses_valid_columns() {
        for (width, height) in [(1.0, 1.0), (16.0, 80.0), (200.0, 300.0), (320.0, 500.0), (800.0, 500.0), (1280.0, 800.0)] {
            let (menu_width, scroll_height, columns) = start_menu_dimensions(width, height);
            assert!(menu_width <= width && menu_width > 0.0);
            assert!(scroll_height <= height && scroll_height > 0.0);
            assert!((1..=6).contains(&columns));
        }
    }


    #[test]
    fn start_pins_can_be_reordered_without_losing_entries() {
        let mut pins = vec!["Files".to_owned(), "Terminal".to_owned(), "Editor".to_owned()];
        assert!(move_pinned_app(&mut pins, "Editor", -1));
        assert_eq!(pins, vec!["Files", "Editor", "Terminal"]);
        assert!(!move_pinned_app(&mut pins, "Files", -1));
        assert!(!move_pinned_app(&mut pins, "Missing", 1));
    }

    #[test]
    fn desktop_positions_reject_unknown_and_non_finite_values() {
        let mut positions = BTreeMap::new();
        positions.insert("Files".to_owned(), [0.2, 0.8]);
        positions.insert("Unknown App".to_owned(), [0.5, 0.5]);
        positions.insert("Terminal".to_owned(), [f32::NAN, 0.4]);
        positions.insert("Editor".to_owned(), [1.8, -0.2]);
        let normalized = normalize_desktop_positions(&positions);
        assert_eq!(normalized.get("Files"), Some(&[0.2, 0.8]));
        assert_eq!(normalized.get("Editor"), Some(&[1.0, 0.0]));
        assert!(!normalized.contains_key("Unknown App"));
        assert!(!normalized.contains_key("Terminal"));
    }

    #[test]
    fn desktop_layout_reseeds_icon_positions_only_after_a_real_resize() {
        assert!(!desktop_layout_changed(None, egui::vec2(800.0, 600.0)));
        assert!(!desktop_layout_changed(Some(egui::vec2(800.0, 600.0)), egui::vec2(800.5, 600.5)));
        assert!(desktop_layout_changed(Some(egui::vec2(800.0, 600.0)), egui::vec2(1024.0, 600.0)));
        assert!(desktop_layout_changed(Some(egui::vec2(800.0, 600.0)), egui::vec2(800.0, 500.0)));
    }

    #[test]
    fn desktop_shortcut_normalized_travel_accounts_for_icon_size_and_insets() {
        let rect = egui::Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(800.0, 600.0));
        assert_eq!(desktop_shortcut_travel(rect), (690.0, 450.0));
        let tiny = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(80.0, 100.0));
        assert_eq!(desktop_shortcut_travel(tiny), (1.0, 1.0));
    }

    #[test]
    fn desktop_default_positions_stay_inside_expected_grid_rows() {
        let rect = egui::Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(640.0, 480.0));
        let first = desktop_default_position(0, rect, 5);
        let sixth = desktop_default_position(5, rect, 5);
        assert_eq!(first, egui::pos2(24.0, 86.0));
        assert_eq!(sixth, egui::pos2(24.0, 180.0));
    }

    #[test]
    fn compact_file_names_truncate_without_splitting_unicode_scalars() {
        assert_eq!(compact_file_name("report.txt", 20), "report.txt");
        assert_eq!(compact_file_name("Báo cáo học kỳ.docx", 9), "Báo cáo …");
        assert_eq!(compact_file_name("filename", 1), "…");
        assert_eq!(compact_file_name("filename", 0), "…");
    }

    #[test]
    fn start_menu_widths_remain_positive_for_tiny_viewports() {
        for (width, height) in [(1.0, 1.0), (16.0, 80.0), (23.0, 120.0)] {
            let (menu_width, scroll_height, _) = start_menu_dimensions(width, height);
            assert!(menu_width >= 1.0 && menu_width <= width);
            assert!(scroll_height >= 1.0 && scroll_height <= height);
        }
    }

    #[test]
    fn discarding_editor_changes_restores_the_last_saved_snapshot() {
        let mut current = "unsaved edits".to_owned();
        discard_unsaved_editor_text(&mut current, "last saved text");
        assert_eq!(current, "last saved text");
    }

    #[test]
    fn new_editor_document_uses_an_unused_untitled_name() {
        let base = std::env::temp_dir().join(format!("nova-editor-untitled-name-{}", std::process::id()));
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(base.join("Untitled.txt"), "keep").unwrap();
        std::fs::write(base.join("Untitled (2).txt"), "keep too").unwrap();
        assert_eq!(next_untitled_file_name(&base).unwrap(), "Untitled (3).txt");
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn invalid_and_duplicate_settings_pins_are_removed() {
        let raw = vec![
            "Files".to_owned(), "Unknown App".to_owned(), "Files".to_owned(),
            "Terminal".to_owned(), "../../outside.exe".to_owned(),
        ];
        assert_eq!(normalize_pinned_apps(&raw), vec!["Files".to_owned(), "Terminal".to_owned()]);
    }

    #[test]
    fn empty_settings_pins_remain_empty_instead_of_repinning_apps() {
        assert!(normalize_pinned_apps(&[]).is_empty());
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
    fn calculator_table_is_bounded_and_evaluates_x() {
        let table = super::generate_table("x^2", "-2", "2", "1", super::AngleUnit::Degree, 0.0);
        assert!(table.contains("4"));
        assert!(table.contains("0"));
        assert!(super::generate_table("x", "0", "1000", "1", super::AngleUnit::Degree, 0.0).contains("limited to 101"));
    }

    #[test]
    fn calculator_matrix_inverse_and_determinant_are_consistent() {
        assert_eq!(super::matrix_determinant("1,2;3,4").unwrap(), -2.0);
        let inverse = super::matrix_inverse("1,2;3,4").unwrap();
        assert_eq!(inverse, "-2, 1\n1.5, -0.5");
        assert!(super::matrix_inverse("1,2;2,4").is_err());
    }

    #[test]
    fn calculator_vector_dot_and_cross_are_computed() {
        let result = super::vector_calculate("1,2,3", "4,5,6");
        assert!(result.contains("A·B = 32"));
        assert!(result.contains("A×B = (-3, 6, -3)"));
    }

    #[test]
    fn editor_replace_all_counts_matches_and_preserves_non_matches() {
        let (updated, count) = super::replace_all_bounded("hello world, hello", "hello", "NOVA", 64).unwrap();
        assert_eq!(updated, "NOVA world, NOVA");
        assert_eq!(count, 2);
        let (unchanged, count) = super::replace_all_bounded("hello", "missing", "x", 64).unwrap();
        assert_eq!(unchanged, "hello");
        assert_eq!(count, 0);
        let (same, count) = super::replace_all_bounded("hello", "hello", "hello", 64).unwrap();
        assert_eq!(same, "hello");
        assert_eq!(count, 1);
    }

    #[test]
    fn editor_replace_all_checks_output_size_before_allocating() {
        let error = super::replace_all_bounded("aaaa", "a", "0123456789", 16).unwrap_err();
        assert!(error.contains("exceed"));
        let (unchanged, count) = super::replace_all_bounded("hello", "", "x", 64).unwrap();
        assert_eq!(unchanged, "hello");
        assert_eq!(count, 0);
    }

    #[test]
    fn calculator_format_does_not_round_small_values_to_zero() {
        assert_eq!(format_number(0.000000001), "0.000000001");
        assert_eq!(format_number(-0.0000000000000001), "-0.0000000000000001");
        assert_eq!(format_number(15.0), "15");
        assert_eq!(format_number(-0.0), "0");
    }

    #[test]
    fn matrix_display_removes_roundoff_without_hiding_small_values() {
        assert_eq!(super::format_matrix_number(-1.9999999999999996), "-2");
        assert_eq!(super::format_matrix_number(0.9999999999999998), "1");
        assert_eq!(super::format_matrix_number(1.2345678901234), "1.2345678901234");
        assert_eq!(super::format_matrix_number(-0.0000000000000001), "-0.0000000000000001");
        assert_eq!(format_number(1.000000000000001), "1.000000000000001");
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
        // NOVA closes so the portable workspace does not leave input hooks running.
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
