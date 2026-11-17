use crate::storage::AppPaths;
use crate::wifi;
use std::io;
use std::path::{Path, PathBuf};
#[cfg(windows)]
use std::process::Command;
use std::time::Duration;

#[cfg(windows)]
const COMMAND_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_OUTPUT_HISTORY_LINES: usize = 1500;
const MAX_OUTPUT_HISTORY_BYTES: usize = 4 * 1024 * 1024;

/// A potentially slow terminal operation. The UI sends this to a worker thread
/// so CMD and Windows WLAN operations never block the egui event loop.
pub enum TerminalTask {
    Command { command: String, cwd: PathBuf },
    WifiScan,
    WifiConnect { paths: AppPaths, ssid: String, password: Option<String> },
    WifiDelete { paths: AppPaths, ssid: String },
}

impl TerminalTask {
    pub fn run(self) -> String {
        match self {
            Self::Command { command, cwd } => match run_cmd(&command, &cwd) {
                Ok(output) if output.is_empty() => String::new(),
                Ok(output) => output,
                Err(error) => format!("Error: {error}"),
            },
            Self::WifiScan => match wifi::scan_networks() {
                Ok(networks) if networks.is_empty() => "No visible Wi-Fi networks were returned by Windows WLAN.".to_owned(),
                Ok(networks) => {
                    let mut lines = vec!["Nearby Wi-Fi networks:".to_owned()];
                    for (index, network) in networks.iter().enumerate() {
                        lines.push(format!("  {}. {}", index + 1, network));
                    }
                    lines.push("Use: wifi select \"SSID\"  |  wifi connect \"SSID\" [password]".to_owned());
                    lines.join("\n")
                }
                Err(error) => format!("Wi-Fi scan failed: {error}"),
            },
            Self::WifiConnect { paths, ssid, password } => {
                match wifi::connect(&paths, &ssid, password.as_deref()).and_then(|_| {
                    wifi::wait_for_connection(Some(&ssid), Duration::from_secs(15))
                }) {
                    Ok(message) => message,
                    Err(error) => format!("Wi-Fi connection failed: {error}"),
                }
            }
            Self::WifiDelete { paths, ssid } => match wifi::delete_profile(&paths, &ssid) {
                Ok(message) => message,
                Err(error) => format!("Wi-Fi profile deletion failed: {error}"),
            },
        }
    }
}

#[derive(Debug)]
pub struct TerminalState {
    pub input: String,
    pub output: Vec<String>,
    pub cwd: PathBuf,
    pub wifi_mode: Option<String>,
}

impl TerminalState {
    pub fn new(paths: &AppPaths) -> Self {
        let mut state = Self {
            input: String::new(),
            output: Vec::new(),
            cwd: paths.user_dir.clone(),
            wifi_mode: None,
        };
        state.line("NOVA OS Native Terminal 0.5.7");
        state.line("Type help for commands. Wi-Fi: wifi -h");
        state.line(format!("Working directory: {}", state.cwd.display()));
        state.line("");
        state
    }

    pub fn prompt(&self) -> String {
        match &self.wifi_mode {
            Some(ssid) => format!("<wifi-{ssid}>:"),
            None => format!("{}>", self.cwd.display()),
        }
    }

    pub fn password_entry(&self) -> bool {
        fn starts_ascii_case_insensitive(value: &str, prefix: &str) -> bool {
            value.get(..prefix.len()).is_some_and(|start| start.eq_ignore_ascii_case(prefix))
        }
        let input = self.input.trim_start();
        self.wifi_mode.is_some()
            || starts_ascii_case_insensitive(input, "wifi connect ")
            || starts_ascii_case_insensitive(input, "/wifi connect ")
    }

    pub fn line(&mut self, text: impl Into<String>) {
        let mut text = text.into();
        const TRUNCATION_PREFIX: &str = "[NOVA] Beginning of oversized output omitted\n";
        if text.len() > MAX_OUTPUT_HISTORY_BYTES {
            let mut start = text.len() - (MAX_OUTPUT_HISTORY_BYTES - TRUNCATION_PREFIX.len());
            while !text.is_char_boundary(start) {
                start += 1;
            }
            text = format!("{TRUNCATION_PREFIX}{}", &text[start..]);
        }

        self.output.push(text);
        let mut total_bytes: usize = self.output.iter().map(String::len).sum();
        while self.output.len() > MAX_OUTPUT_HISTORY_LINES || total_bytes > MAX_OUTPUT_HISTORY_BYTES {
            if self.output.is_empty() {
                break;
            }
            let removed = self.output.remove(0);
            total_bytes = total_bytes.saturating_sub(removed.len());
        }
    }

    pub fn clear(&mut self) {
        self.output.clear();
    }

    /// Process fast NOVA built-ins immediately and return slow Windows work
    /// for the background worker. Wi-Fi passwords are deliberately not echoed.
    pub fn submit(&mut self, paths: &AppPaths, raw: &str) -> Option<TerminalTask> {
        let line = raw.trim();
        if line.is_empty() {
            return None;
        }

        if let Some(ssid) = self.wifi_mode.clone() {
            return self.submit_wifi_session(paths, &ssid, line);
        }

        let lower = line.to_ascii_lowercase();
        if lower == "cls" || lower == "clear" {
            self.clear();
            return None;
        }
        if lower == "nova" || lower == "nova -h" || lower == "nova /?" {
            self.line("NOVA Terminal commands:");
            self.line("  cls / clear                 Clear the NOVA terminal view");
            self.line("  wifi                        Scan nearby wireless networks");
            self.line("  wifi -h                     Show Wi-Fi help");
            self.line("  wifi connect \"SSID\" [password]");
            self.line("  wifi select \"SSID\"");
            self.line("Windows CMD commands are passed to cmd.exe. Type help for Windows command help.");
            self.line("Warning: CMD commands run as your Windows user and can change the real computer.");
            return None;
        }
        if lower == "exit" {
            self.line("Close the Terminal window to leave NOVA Terminal.");
            return None;
        }

        if lower == "cd" || lower.starts_with("cd ") || lower.starts_with("chdir ") || lower == "cd.." || lower == "cd\\" {
            self.change_directory(line);
            return None;
        }

        if lower == "wifi" || lower.starts_with("wifi ") || lower == "/wifi" || lower.starts_with("/wifi ") {
            let args = match parse_words(line) {
                Ok(args) => args,
                Err(error) => {
                    self.line(format!("Wi-Fi command syntax error: {error}"));
                    return None;
                }
            };
            return self.submit_wifi_command(paths, &args);
        }

        self.line(format!("{} {line}", self.cwd.display()));
        Some(TerminalTask::Command { command: line.to_owned(), cwd: self.cwd.clone() })
    }

    fn change_directory(&mut self, line: &str) {
        let trimmed = line.trim();
        let lower = trimmed.to_ascii_lowercase();
        if lower == "cd" || lower == "chdir" {
            self.line(self.cwd.display().to_string());
            return;
        }
        if lower == "cd.." {
            if let Some(parent) = self.cwd.parent() {
                self.cwd = parent.to_path_buf();
            }
            self.line(self.cwd.display().to_string());
            return;
        }
        if lower == "cd\\" {
            if let Some(root) = self.cwd.ancestors().last() {
                self.cwd = root.to_path_buf();
            }
            self.line(self.cwd.display().to_string());
            return;
        }

        let mut parts = match parse_words(trimmed) {
            Ok(parts) => parts,
            Err(error) => {
                self.line(format!("cd: {error}"));
                return;
            }
        };
        if parts.is_empty() {
            self.line(self.cwd.display().to_string());
            return;
        }
        parts.remove(0);
        if parts.first().is_some_and(|value| value.eq_ignore_ascii_case("/d")) {
            parts.remove(0);
        }
        if parts.is_empty() {
            self.line(self.cwd.display().to_string());
            return;
        }

        let target = PathBuf::from(parts.join(" "));
        let candidate = if target.is_absolute() { target } else { self.cwd.join(target) };
        if candidate.is_dir() {
            match candidate.canonicalize() {
                Ok(path) => self.cwd = path,
                Err(error) => self.line(format!("cd: {error}")),
            }
        } else {
            self.line("The system cannot find the path specified.");
        }
    }

    fn submit_wifi_command(&mut self, paths: &AppPaths, args: &[String]) -> Option<TerminalTask> {
        let options: Vec<String> = args.iter().skip(1).cloned().collect();
        let option = options.first().map(|value| value.to_ascii_lowercase());
        match option.as_deref() {
            None => Some(TerminalTask::WifiScan),
            Some("-h") | Some("--help") | Some("help") => {
                self.line("NOVA Wi-Fi commands:");
                self.line("  wifi                         List nearby Wi-Fi networks");
                self.line("  wifi -h                      Show this help");
                self.line("  wifi connect \"SSID\" [pass] Connect directly");
                self.line("  wifi select \"SSID\"        Enter Wi-Fi setup mode");
                self.line("In setup mode:");
                self.line("  password <password>         Create/update NOVA profile and connect");
                self.line("  password delete             Delete only the NOVA-managed profile");
                self.line("  exit                         Return to CMD");
                self.line("WPA2-Personal supported; enterprise/802.1X and captive portals are not configured.");
                None
            }
            Some("select") => {
                if options.len() < 2 {
                    self.line("Usage: wifi select \"SSID\"");
                    return None;
                }
                let ssid = options[1..].join(" ");
                if let Err(error) = wifi::validate_ssid(&ssid) {
                    self.line(format!("Wi-Fi: {error}"));
                    return None;
                }
                self.wifi_mode = Some(ssid.clone());
                self.line(format!("Selected network: {ssid}"));
                self.line("Enter password <password> to configure/connect, password delete to remove the NOVA profile, or exit.");
                None
            }
            Some("connect") => {
                if options.len() < 2 {
                    self.line("Usage: wifi connect \"SSID\" [password]");
                    return None;
                }
                if options.len() > 3 {
                    self.line("Too many arguments. Quote the SSID and password if they contain spaces.");
                    return None;
                }
                let ssid = options[1].clone();
                let password = options.get(2).cloned();
                Some(TerminalTask::WifiConnect { paths: paths.clone(), ssid, password })
            }
            Some(other) => {
                self.line(format!("Unknown Wi-Fi option: {other}. Type wifi -h."));
                None
            }
        }
    }

    fn submit_wifi_session(&mut self, paths: &AppPaths, ssid: &str, line: &str) -> Option<TerminalTask> {
        let parts = match parse_words(line) {
            Ok(parts) => parts,
            Err(error) => {
                self.line(format!("Wi-Fi: {error}"));
                return None;
            }
        };
        let Some(command) = parts.first().map(|value| value.to_ascii_lowercase()) else {
            return None;
        };
        match command.as_str() {
            "exit" => {
                self.wifi_mode = None;
                self.line("Returned to Windows CMD mode.");
                None
            }
            "password" => {
                if parts.get(1).is_some_and(|value| value.eq_ignore_ascii_case("delete")) {
                    if parts.len() != 2 {
                        self.line("Usage: password delete");
                        return None;
                    }
                    Some(TerminalTask::WifiDelete { paths: paths.clone(), ssid: ssid.to_owned() })
                } else {
                    if parts.len() < 2 {
                        self.line("Usage: password <password>");
                        return None;
                    }
                    let password = parts[1..].join(" ");
                    Some(TerminalTask::WifiConnect { paths: paths.clone(), ssid: ssid.to_owned(), password: Some(password) })
                }
            }
            "connect" => {
                if parts.len() != 1 {
                    self.line("For a secured network, use password <password>. For an open network, type connect.");
                    return None;
                }
                Some(TerminalTask::WifiConnect { paths: paths.clone(), ssid: ssid.to_owned(), password: None })
            }
            "help" | "-h" => {
                self.line("password <password> — configure the selected network and connect");
                self.line("password delete — remove the NOVA-managed WLAN profile");
                self.line("exit — return to CMD");
                None
            }
            other => {
                self.line(format!("Unknown Wi-Fi setup command: {other}. Type help."));
                None
            }
        }
    }
}

pub fn parse_words(input: &str) -> Result<Vec<String>, String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\\' && quote.is_some() {
            let next_is_escape = chars.peek().is_some_and(|next| Some(*next) == quote || *next == '\\');
            if next_is_escape {
                current.push(chars.next().expect("peeked character"));
            } else {
                current.push('\\');
            }
            continue;
        }
        if ch == '"' || ch == '\'' {
            if quote == Some(ch) {
                quote = None;
            } else if quote.is_none() {
                quote = Some(ch);
            } else {
                current.push(ch);
            }
            continue;
        }
        if ch.is_whitespace() && quote.is_none() {
            if !current.is_empty() {
                words.push(std::mem::take(&mut current));
            }
        } else {
            current.push(ch);
        }
    }

    if quote.is_some() {
        return Err("A quoted argument was not closed.".to_owned());
    }
    if !current.is_empty() {
        words.push(current);
    }
    Ok(words)
}

/// Ask CMD and console-mode child programs to use UTF-8 for redirected text.
/// Process arguments and filesystem paths are still passed to Windows as UTF-16.
fn utf8_cmd_payload(command: &str) -> String {
    format!("chcp 65001>nul & {command}")
}

#[cfg(windows)]
fn run_cmd(command: &str, cwd: &Path) -> io::Result<String> {
    let mut process = Command::new(crate::windows_process::system_binary("cmd.exe")?);
    let payload = utf8_cmd_payload(command);
    process.args(["/D", "/S", "/C", &payload]).current_dir(cwd);
    let output = crate::windows_process::run_captured(process, COMMAND_TIMEOUT)?;
    Ok(output.combined_text())
}

#[cfg(not(windows))]
fn run_cmd(_command: &str, _cwd: &Path) -> io::Result<String> {
    Ok("Windows CMD is available only in the Windows build of NOVA.".to_owned())
}

#[cfg(test)]
mod tests {
    use super::{parse_words, TerminalState, TerminalTask};
    use std::path::PathBuf;

    #[test]
    fn parses_quoted_ssid_and_password() {
        let parsed = parse_words("wifi connect \"Home WiFi\" \"secret pass\"").unwrap();
        assert_eq!(parsed, ["wifi", "connect", "Home WiFi", "secret pass"]);
    }

    #[test]
    fn parses_slash_wifi() {
        let parsed = parse_words("/wifi -h").unwrap();
        assert_eq!(parsed, ["/wifi", "-h"]);
    }

    #[test]
    fn rejects_unclosed_quotes() {
        assert!(parse_words("wifi select \"Home WiFi").is_err());
    }

    #[test]
    fn cmd_payload_enables_utf8_before_user_command() {
        assert_eq!(super::utf8_cmd_payload("echo Tiếng Việt 日本語"), "chcp 65001>nul & echo Tiếng Việt 日本語");
    }

    #[test]
    fn terminal_output_history_is_bounded_by_bytes() {
        let mut state = TerminalState { input: String::new(), output: Vec::new(), cwd: PathBuf::from("."), wifi_mode: None };
        state.line("x".repeat(super::MAX_OUTPUT_HISTORY_BYTES + 64));
        let first_total: usize = state.output.iter().map(String::len).sum();
        assert!(first_total <= super::MAX_OUTPUT_HISTORY_BYTES);
        assert!(state.output[0].starts_with("[NOVA] Beginning of oversized output omitted"));
        state.line("latest line");
        let total: usize = state.output.iter().map(String::len).sum();
        assert!(total <= super::MAX_OUTPUT_HISTORY_BYTES);
        assert!(state.output.iter().any(|line| line == "latest line"));
    }

    #[test]
    fn normal_commands_are_scheduled_as_background_jobs() {
        let mut state = TerminalState { input: String::new(), output: Vec::new(), cwd: PathBuf::from("."), wifi_mode: None };
        let task = state.submit(&dummy_paths(), "echo hello");
        assert!(matches!(task, Some(TerminalTask::Command { .. })));
    }

    #[test]
    fn wifi_scan_is_scheduled_as_a_background_job() {
        let mut state = TerminalState { input: String::new(), output: Vec::new(), cwd: PathBuf::from("."), wifi_mode: None };
        assert!(matches!(state.submit(&dummy_paths(), "wifi"), Some(TerminalTask::WifiScan)));
    }

    #[test]
    fn password_entry_masks_direct_and_selected_wifi_passwords_only() {
        let mut state = TerminalState { input: String::new(), output: Vec::new(), cwd: PathBuf::from("."), wifi_mode: None };
        assert!(!state.password_entry());
        state.input = "wifi connect \"Lab AP\" secret".to_owned();
        assert!(state.password_entry());
        state.input = "/WIFI CONNECT \"Lab AP\" secret".to_owned();
        assert!(state.password_entry());
        state.input = "echo safe".to_owned();
        assert!(!state.password_entry());
        state.wifi_mode = Some("Lab AP".to_owned());
        assert!(state.password_entry());
    }

    #[test]
    fn selected_wifi_password_is_not_written_to_terminal_history() {
        let mut state = TerminalState { input: String::new(), output: Vec::new(), cwd: PathBuf::from("."), wifi_mode: None };
        let paths = dummy_paths();
        assert!(state.submit(&paths, "wifi select \"Lab AP\"").is_none());
        let task = state.submit(&paths, "password \"secret phrase\"");
        assert!(matches!(task, Some(TerminalTask::WifiConnect { password: Some(secret), .. }) if secret == "secret phrase"));
        assert!(!state.output.join("\n").contains("secret phrase"));
    }

    fn dummy_paths() -> crate::storage::AppPaths {
        // The command scheduling path only needs the current directory, but
        // AppPaths is used for the general API. Tests use a deterministic temp root.
        let root = std::env::temp_dir();
        crate::storage::AppPaths {
            app_dir: root.clone(),
            data_dir: root.clone(),
            security_dir: root.join("Security"),
            user_dir: root.clone(),
            settings_file: root.join("settings.json"),
            wifi_manifest: root.join("wifi.json"),
            trash_dir: root.join("Trash"),
            portable: true,
        }
    }
}
