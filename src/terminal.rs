use crate::storage::AppPaths;
use crate::wifi;
use std::io::{self, Read};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const MAX_OUTPUT_BYTES: u64 = 1024 * 1024;
const COMMAND_TIMEOUT: Duration = Duration::from_secs(30);
#[cfg(windows)]
const OUTPUT_DRAIN_TIMEOUT: Duration = Duration::from_secs(2);
#[cfg(windows)]
const OUTPUT_DRAIN_KILL_GRACE: Duration = Duration::from_millis(750);

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
        state.line("NOVA OS Native Terminal 0.3.2");
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
        self.wifi_mode.is_some()
            || self
                .input
                .trim_start()
                .to_ascii_lowercase()
                .starts_with("wifi connect ")
            || self
                .input
                .trim_start()
                .to_ascii_lowercase()
                .starts_with("/wifi connect ")
    }

    pub fn line(&mut self, text: impl Into<String>) {
        self.output.push(text.into());
        if self.output.len() > 1500 {
            let excess = self.output.len() - 1500;
            self.output.drain(0..excess);
        }
    }

    pub fn clear(&mut self) {
        self.output.clear();
    }

    pub fn submit(&mut self, paths: &AppPaths, raw: &str) {
        let line = raw.trim();
        if line.is_empty() {
            return;
        }

        if let Some(ssid) = self.wifi_mode.clone() {
            self.submit_wifi_session(paths, &ssid, line);
            return;
        }

        let lower = line.to_ascii_lowercase();
        if lower == "cls" || lower == "clear" {
            self.clear();
            return;
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
            return;
        }
        if lower == "exit" {
            self.line("Close the Terminal window to leave NOVA Terminal.");
            return;
        }

        if lower == "cd" || lower.starts_with("cd ") || lower.starts_with("chdir ") || lower == "cd.." || lower == "cd\\" {
            self.change_directory(line);
            return;
        }

        if lower == "wifi" || lower.starts_with("wifi ") || lower == "/wifi" || lower.starts_with("/wifi ") {
            let args = match parse_words(line) {
                Ok(args) => args,
                Err(error) => {
                    self.line(format!("Wi-Fi command syntax error: {error}"));
                    return;
                }
            };
            self.submit_wifi_command(paths, &args);
            return;
        }

        self.line(format!("{} {line}", self.cwd.display()));
        match run_cmd(line, &self.cwd) {
            Ok(output) => {
                if !output.trim().is_empty() {
                    for line in output.lines() {
                        self.line(line.to_owned());
                    }
                }
            }
            Err(error) => self.line(format!("Error: {error}")),
        }
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
        if parts.first().is_some_and(|v| v.eq_ignore_ascii_case("/d")) {
            parts.remove(0);
        }
        if parts.is_empty() {
            self.line(self.cwd.display().to_string());
            return;
        }
        let target = PathBuf::from(parts.join(" "));
        let candidate = if target.is_absolute() {
            target
        } else {
            self.cwd.join(target)
        };
        if candidate.is_dir() {
            match candidate.canonicalize() {
                Ok(path) => self.cwd = path,
                Err(error) => self.line(format!("cd: {error}")),
            }
        } else {
            self.line("The system cannot find the path specified.");
        }
    }

    fn submit_wifi_command(&mut self, paths: &AppPaths, args: &[String]) {
        // `wifi` and `/wifi` share the same option parser; only the prefix differs.
        let options: Vec<String> = args.iter().skip(1).cloned().collect();
        let option = options.first().map(|s| s.to_ascii_lowercase());

        match option.as_deref() {
            None => match wifi::scan_networks() {
                Ok(networks) if networks.is_empty() => self.line("No visible Wi-Fi networks were returned by Windows WLAN."),
                Ok(networks) => {
                    self.line("Nearby Wi-Fi networks:");
                    for (index, network) in networks.iter().enumerate() {
                        self.line(format!("  {}. {}", index + 1, network));
                    }
                    self.line("Use: wifi select \"SSID\"  |  wifi connect \"SSID\" [password]");
                }
                Err(error) => self.line(format!("Wi-Fi scan failed: {error}")),
            },
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
            }
            Some("select") => {
                if options.len() < 2 {
                    self.line("Usage: wifi select \"SSID\"");
                    return;
                }
                let ssid = options[1..].join(" ");
                if let Err(error) = wifi::validate_ssid(&ssid) {
                    self.line(format!("Wi-Fi: {error}"));
                    return;
                }
                self.wifi_mode = Some(ssid.clone());
                self.line(format!("Selected network: {ssid}"));
                self.line("Enter password <password> to configure/connect, password delete to remove the NOVA profile, or exit.");
            }
            Some("connect") => {
                if options.len() < 2 {
                    self.line("Usage: wifi connect \"SSID\" [password]");
                    return;
                }
                let ssid = options[1].clone();
                let password = options.get(2).map(String::as_str);
                if options.len() > 3 {
                    self.line("Too many arguments. Quote the SSID and password if they contain spaces.");
                    return;
                }
                match wifi::connect(paths, &ssid, password) {
                    Ok(message) => self.line(message),
                    Err(error) => self.line(format!("Wi-Fi connection failed: {error}")),
                }
            }
            Some(other) => self.line(format!("Unknown Wi-Fi option: {other}. Type wifi -h.")),
        }
    }

    fn submit_wifi_session(&mut self, paths: &AppPaths, ssid: &str, line: &str) {
        let parts = match parse_words(line) {
            Ok(parts) => parts,
            Err(error) => {
                self.line(format!("Wi-Fi: {error}"));
                return;
            }
        };
        let Some(command) = parts.first().map(|s| s.to_ascii_lowercase()) else {
            return;
        };
        match command.as_str() {
            "exit" => {
                self.wifi_mode = None;
                self.line("Returned to Windows CMD mode.");
            }
            "password" => {
                if parts.get(1).is_some_and(|value| value.eq_ignore_ascii_case("delete")) {
                    if parts.len() != 2 {
                        self.line("Usage: password delete");
                        return;
                    }
                    match wifi::delete_profile(paths, ssid) {
                        Ok(message) => self.line(message),
                        Err(error) => self.line(format!("Wi-Fi profile deletion failed: {error}")),
                    }
                } else {
                    if parts.len() < 2 {
                        self.line("Usage: password <password>");
                        return;
                    }
                    let password = parts[1..].join(" ");
                    match wifi::connect(paths, ssid, Some(&password)) {
                        Ok(message) => self.line(message),
                        Err(error) => self.line(format!("Wi-Fi setup failed: {error}")),
                    }
                }
            }
            "connect" => {
                if parts.len() != 1 {
                    self.line("For a secured network, use password <password>. For an open network, type connect.");
                    return;
                }
                match wifi::connect(paths, ssid, None) {
                    Ok(message) => self.line(message),
                    Err(error) => self.line(format!("Wi-Fi connection failed: {error}")),
                }
            }
            "help" | "-h" => {
                self.line("password <password> — configure the selected network and connect");
                self.line("password delete — remove the NOVA-managed WLAN profile");
                self.line("exit — return to CMD");
            }
            other => self.line(format!("Unknown Wi-Fi setup command: {other}. Type help.")),
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

#[cfg(windows)]
fn run_cmd(command: &str, cwd: &std::path::Path) -> io::Result<String> {
    let mut child = Command::new("cmd.exe")
        .args(["/D", "/S", "/C", command])
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let pid = child.id();
    let stdout = child.stdout.take().expect("piped stdout");
    let stderr = child.stderr.take().expect("piped stderr");
    let out_reader = thread::spawn(move || read_limited(stdout));
    let err_reader = thread::spawn(move || read_limited(stderr));

    let deadline = Instant::now() + COMMAND_TIMEOUT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {}
            Err(error) => {
                kill_process_tree(pid);
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
        }
        if Instant::now() >= deadline {
            kill_process_tree(pid);
            // Also attempt to stop cmd.exe itself if taskkill could not reach it.
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        thread::sleep(Duration::from_millis(80));
    };

    // A descendant may inherit CMD's output handles and keep a pipe open after
    // cmd.exe exits. Never block the NOVA UI forever waiting for EOF.
    let mut readers_finished = wait_for_readers(&out_reader, &err_reader, OUTPUT_DRAIN_TIMEOUT);
    if !readers_finished {
        kill_process_tree(pid);
        readers_finished = wait_for_readers(&out_reader, &err_reader, OUTPUT_DRAIN_KILL_GRACE);
    }

    let (stdout, stdout_truncated) = if out_reader.is_finished() {
        out_reader
            .join()
            .map_err(|_| io::Error::other("NOVA stdout reader thread panicked"))??
    } else {
        drop(out_reader); // Detach an unresponsive reader rather than freezing the UI.
        (Vec::new(), false)
    };
    let (stderr, stderr_truncated) = if err_reader.is_finished() {
        err_reader
            .join()
            .map_err(|_| io::Error::other("NOVA stderr reader thread panicked"))??
    } else {
        drop(err_reader);
        (Vec::new(), false)
    };
    let mut output = String::from_utf8_lossy(&stdout).to_string();
    let error_text = String::from_utf8_lossy(&stderr);
    if !error_text.trim().is_empty() {
        if !output.is_empty() {
            output.push('\n');
        }
        output.push_str(error_text.trim_end());
    }
    if stdout_truncated || stderr_truncated {
        output.push_str("\n[NOVA] Output was truncated at 1 MiB per stream.");
    }
    if !readers_finished {
        output.push_str("\n[NOVA] Output collection stopped after the drain timeout; a descendant may still hold a pipe open.");
    }
    if status.is_none() {
        output.push_str("\n[NOVA] Command timed out after 30 seconds. NOVA requested process-tree termination.");
    } else if output.is_empty() {
        if let Some(status) = status {
            if !status.success() {
                output = format!("Command exited with code {}.", status.code().unwrap_or(-1));
            }
        }
    }
    Ok(output)
}

#[cfg(windows)]
fn wait_for_readers(
    stdout: &thread::JoinHandle<io::Result<(Vec<u8>, bool)>>,
    stderr: &thread::JoinHandle<io::Result<(Vec<u8>, bool)>>,
    timeout: Duration,
) -> bool {
    let deadline = Instant::now() + timeout;
    while (!stdout.is_finished() || !stderr.is_finished()) && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(20));
    }
    stdout.is_finished() && stderr.is_finished()
}

#[cfg(windows)]
fn read_limited<R: Read>(reader: R) -> io::Result<(Vec<u8>, bool)> {
    let mut bytes = Vec::new();
    reader.take(MAX_OUTPUT_BYTES + 1).read_to_end(&mut bytes)?;
    let truncated = bytes.len() as u64 > MAX_OUTPUT_BYTES;
    if truncated {
        bytes.truncate(MAX_OUTPUT_BYTES as usize);
    }
    Ok((bytes, truncated))
}

#[cfg(windows)]
fn kill_process_tree(pid: u32) {
    let _ = Command::new("taskkill.exe")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

#[cfg(not(windows))]
fn run_cmd(_command: &str, _cwd: &std::path::Path) -> io::Result<String> {
    Ok("Windows CMD is available only in the Windows build of NOVA.".to_owned())
}

#[cfg(test)]
mod tests {
    use super::parse_words;

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
}
