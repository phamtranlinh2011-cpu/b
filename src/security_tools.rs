//! GUI-first integrations for the five NOVA Security applications.
//!
//! NOVA provides a guided launcher and scope reminder. The scanner engines are
//! external, separately installed applications/services; NOVA does not bundle
//! them or silently run scans. Every tool must be used only against explicitly
//! authorized assets. The scope checkbox is an operator acknowledgement, not
//! a technical sandbox around the external tool.

use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SecurityTool {
    NetworkDiscovery,
    VulnerabilityManagement,
    WebSecurity,
    PacketAnalyzer,
    WebApiInspector,
}

#[derive(Clone, Copy, Debug)]
pub struct ToolInfo {
    pub tool: SecurityTool,
    pub name: &'static str,
    pub description: &'static str,
    pub executable: &'static str,
    pub website: &'static str,
    pub documentation: &'static str,
    pub requires_local_executable: bool,
}

pub static TOOLS: [ToolInfo; 5] = [
    ToolInfo {
        tool: SecurityTool::NetworkDiscovery,
        name: "Network Discovery",
        description: "Zenmap is the visual interface for mapping devices, ports and services on networks you are authorized to assess.",
        executable: "Zenmap GUI",
        website: "https://nmap.org/download.html",
        documentation: "https://nmap.org/zenmap/",
        requires_local_executable: true,
    },
    ToolInfo {
        tool: SecurityTool::VulnerabilityManagement,
        name: "Vulnerability Management",
        description: "Greenbone provides a web console for authenticated vulnerability assessment, task management and reports. The scanner/server is installed separately.",
        executable: "Greenbone Security Assistant (web console)",
        website: "https://greenbone.github.io/docs/latest/",
        documentation: "https://greenbone.github.io/docs/latest/",
        requires_local_executable: false,
    },
    ToolInfo {
        tool: SecurityTool::WebSecurity,
        name: "Web Security",
        description: "OWASP ZAP provides a desktop workspace for proxy history, passive alerts and authorized web-application testing.",
        executable: "ZAP desktop",
        website: "https://www.zaproxy.org/download/",
        documentation: "https://www.zaproxy.org/docs/desktop/",
        requires_local_executable: true,
    },
    ToolInfo {
        tool: SecurityTool::PacketAnalyzer,
        name: "Packet Analyzer",
        description: "Wireshark is a graphical protocol analyzer for inspecting capture files or traffic on networks and interfaces you are permitted to monitor.",
        executable: "Wireshark GUI",
        website: "https://www.wireshark.org/download.html",
        documentation: "https://www.wireshark.org/docs/wsug_html_chunked/",
        requires_local_executable: true,
    },
    ToolInfo {
        tool: SecurityTool::WebApiInspector,
        name: "Web / API Inspector",
        description: "Burp Suite Community/Professional provides a desktop proxy and manual web/API testing tools. Use a lab or explicitly authorized application scope.",
        executable: "Burp Suite GUI",
        website: "https://portswigger.net/burp/downloads",
        documentation: "https://portswigger.net/burp/documentation/desktop/getting-started",
        requires_local_executable: true,
    },
];

impl SecurityTool {
    pub fn info(self) -> &'static ToolInfo {
        TOOLS.iter().find(|info| info.tool == self).expect("all security tools have metadata")
    }

    pub fn index(self) -> usize {
        TOOLS.iter().position(|info| info.tool == self).expect("all security tools have an index")
    }

    pub fn detect(self, app_dir: &Path) -> Option<PathBuf> {
        if self == Self::VulnerabilityManagement {
            return None;
        }
        executable_candidates(self, app_dir).into_iter().find_map(|candidate| {
            let canonical = candidate.canonicalize().ok()?;
            if canonical.is_file() { Some(canonical) } else { None }
        })
    }

    pub fn launch(self, app_dir: &Path) -> io::Result<PathBuf> {
        if self == Self::VulnerabilityManagement {
            return Err(io::Error::new(io::ErrorKind::Unsupported, "Greenbone is a separately hosted web console. Enter your authorized GSA URL and open it from NOVA."));
        }
        let executable = self.detect(app_dir).ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotFound, format!("{} was not found. Install it from the official website, then choose Detect again.", self.info().name))
        })?;
        let working_directory = executable.parent().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "Tool executable has no parent folder."))?;
        Command::new(&executable).current_dir(working_directory).spawn()?;
        Ok(executable)
    }
}

#[cfg(windows)]
fn executable_candidates(tool: SecurityTool, app_dir: &Path) -> Vec<PathBuf> {
    use std::env;
    let mut roots = Vec::new();
    for variable in ["PROGRAMFILES", "PROGRAMFILES(X86)", "PROGRAMW6432", "LOCALAPPDATA"] {
        if let Some(value) = env::var_os(variable).filter(|value| !value.is_empty()) {
            roots.push(PathBuf::from(value));
        }
    }
    let mut candidates = Vec::new();
    match tool {
        SecurityTool::NetworkDiscovery => {
            for root in &roots {
                candidates.push(root.join("Nmap/zenmap.exe"));
                candidates.push(root.join("Nmap/zenmap/zenmap.exe"));
                candidates.push(root.join("Nmap/zenmap/bin/zenmap.exe"));
            }
            candidates.push(app_dir.join("Apps/Nmap/zenmap.exe"));
            candidates.push(app_dir.join("Apps/Zenmap/zenmap.exe"));
        }
        SecurityTool::VulnerabilityManagement => {}
        SecurityTool::WebSecurity => {
            for root in &roots {
                candidates.push(root.join("OWASP/Zed Attack Proxy/ZAP.exe"));
                candidates.push(root.join("OWASP/Zed Attack Proxy/zap.exe"));
                candidates.push(root.join("ZAP/Zed Attack Proxy/ZAP.exe"));
                candidates.push(root.join("ZAP/Zed Attack Proxy/zap.exe"));
                candidates.push(root.join("Zed Attack Proxy/ZAP.exe"));
                candidates.push(root.join("Zed Attack Proxy/zap.exe"));
                candidates.push(root.join("ZAP/zap.exe"));
            }
            candidates.push(app_dir.join("Apps/ZAP/zap.exe"));
        }
        SecurityTool::PacketAnalyzer => {
            for root in &roots {
                candidates.push(root.join("Wireshark/Wireshark.exe"));
            }
            candidates.push(app_dir.join("Apps/Wireshark/Wireshark.exe"));
        }
        SecurityTool::WebApiInspector => {
            for root in &roots {
                candidates.push(root.join("Burp Suite Community Edition/BurpSuiteCommunity.exe"));
                candidates.push(root.join("Burp Suite Professional/BurpSuitePro.exe"));
                candidates.push(root.join("BurpSuiteCommunity/BurpSuiteCommunity.exe"));
                candidates.push(root.join("BurpSuitePro/BurpSuitePro.exe"));
            }
            candidates.push(app_dir.join("Apps/Burp/BurpSuiteCommunity.exe"));
            candidates.push(app_dir.join("Apps/Burp/BurpSuitePro.exe"));
        }
    }
    candidates
}

#[cfg(not(windows))]
fn executable_candidates(_tool: SecurityTool, _app_dir: &Path) -> Vec<PathBuf> {
    Vec::new()
}

/// Open an explicitly entered HTTP(S) URL using Windows' registered browser.
/// Arguments are passed to ShellExecuteW directly; no shell command is built.
#[cfg(windows)]
pub fn open_web_console(value: &str) -> io::Result<String> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    let url = validate_web_console_url(value)?;
    let operation: Vec<u16> = OsStr::new("open").encode_wide().chain(Some(0)).collect();
    let target: Vec<u16> = OsStr::new(&url).encode_wide().chain(Some(0)).collect();
    #[link(name = "shell32")]
    unsafe extern "system" {
        fn ShellExecuteW(
            window: *mut std::ffi::c_void,
            operation: *const u16,
            file: *const u16,
            parameters: *const u16,
            directory: *const u16,
            show_command: i32,
        ) -> *mut std::ffi::c_void;
    }
    let result = unsafe {
        ShellExecuteW(std::ptr::null_mut(), operation.as_ptr(), target.as_ptr(), std::ptr::null(), std::ptr::null(), 1) as isize
    };
    if result <= 32 {
        return Err(io::Error::other(format!("Windows could not open the Greenbone console (ShellExecute code {result}).")));
    }
    Ok(url)
}

#[cfg(not(windows))]
pub fn open_web_console(_value: &str) -> io::Result<String> {
    Err(io::Error::new(io::ErrorKind::Unsupported, "Opening the Greenbone web console is supported in the Windows NOVA build."))
}

pub fn validate_web_console_url(value: &str) -> io::Result<String> {
    let value = value.trim();
    let invalid = || io::Error::new(io::ErrorKind::InvalidInput, "Enter a valid Greenbone HTTP(S) URL with a hostname and optional port/path.");
    if value.is_empty() || value.len() > 2048 || value.chars().any(|ch| ch.is_control() || ch.is_whitespace() || ch == '\\') {
        return Err(invalid());
    }
    let normalized = if value.contains("://") { value.to_owned() } else { format!("https://{value}") };
    let (scheme, remainder) = normalized.split_once("://").ok_or_else(invalid)?;
    if !matches!(scheme.to_ascii_lowercase().as_str(), "https" | "http") {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "Greenbone console URL must use HTTP or HTTPS."));
    }
    let authority = remainder.split(['/', '?', '#']).next().unwrap_or_default();
    if authority.is_empty() || authority.contains('@') || authority.contains('\'') || authority.contains('"') {
        return Err(invalid());
    }
    // Validate the authority without accepting user-info, malformed ports, or
    // unbracketed IPv6. Paths/query/fragments are allowed after a valid authority.
    let host_port = if authority.starts_with('[') {
        let close = authority.find(']').ok_or_else(invalid)?;
        let ip = &authority[1..close];
        ip.parse::<std::net::Ipv6Addr>().map_err(|_| invalid())?;
        let suffix = &authority[close + 1..];
        if suffix.is_empty() { None } else {
            let port = suffix.strip_prefix(':').ok_or_else(invalid)?;
            Some(port)
        }
    } else {
        if authority.matches(':').count() > 1 { return Err(invalid()); }
        let (host, port) = match authority.rsplit_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (authority, None),
        };
        if host.is_empty() || !host.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-'))
            || host.starts_with('.') || host.ends_with('.') || host.contains("..") || host.starts_with('-') || host.ends_with('-') {
            return Err(invalid());
        }
        port
    };
    if let Some(port) = host_port {
        let parsed = port.parse::<u16>().map_err(|_| invalid())?;
        if parsed == 0 { return Err(invalid()); }
    }
    Ok(normalized)
}

/// Scope confirmation is an explicit operator acknowledgement in NOVA's UI.
/// It intentionally does not claim to enforce target boundaries inside the
/// external third-party application.
pub fn validate_authorized_scope(scope: &str, acknowledged: bool) -> io::Result<&str> {
    let scope = scope.trim();
    if scope.is_empty() || scope.len() > 512 || scope.chars().any(char::is_control) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "Enter a short, explicit authorized asset scope (up to 512 characters)."));
    }
    if scope.chars().all(char::is_whitespace) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "The authorized asset scope cannot be blank."));
    }
    if !acknowledged {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "Confirm that you own the target or have written authorization to test it."));
    }
    Ok(scope)
}

#[cfg(test)]
mod tests {
    use super::{SecurityTool, validate_authorized_scope, validate_web_console_url};

    #[test]
    fn all_five_gui_tools_have_unique_names_and_urls() {
        let names: Vec<_> = super::TOOLS.iter().map(|tool| tool.name).collect();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(names.len(), 5);
        assert_eq!(sorted.len(), 5);
        assert!(super::TOOLS.iter().all(|tool| tool.website.starts_with("https://") && tool.documentation.starts_with("https://")));
    }

    #[test]
    fn greenbone_console_url_is_http_or_https_only() {
        assert_eq!(validate_web_console_url("127.0.0.1:9392").unwrap(), "https://127.0.0.1:9392");
        assert_eq!(validate_web_console_url("http://127.0.0.1:9392").unwrap(), "http://127.0.0.1:9392");
        assert!(validate_web_console_url("file:///C:/secret").is_err());
        assert!(validate_web_console_url("javascript:alert(1)").is_err());
        assert!(validate_web_console_url("https://host\n/path").is_err());
        assert!(validate_web_console_url("https://user:pass@example.test").is_err());
        assert!(validate_web_console_url("https://example.test:bad").is_err());
        assert!(validate_web_console_url("https://example.test:0").is_err());
        assert!(validate_web_console_url("https://bad_host.test").is_err());
        assert!(validate_web_console_url("https://[::1]:9392").is_ok());
    }

    #[test]
    fn launch_scope_requires_explicit_acknowledgement() {
        assert!(validate_authorized_scope("192.0.2.10/32", true).is_ok());
        assert!(validate_authorized_scope("192.0.2.10/32", false).is_err());
        assert!(validate_authorized_scope("", true).is_err());
        assert!(validate_authorized_scope("lab\nother", true).is_err());
    }

    #[test]
    fn greenbone_is_a_hosted_web_console_not_a_local_scanner_binary() {
        assert!(!SecurityTool::VulnerabilityManagement.info().requires_local_executable);
    }
}
