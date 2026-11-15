//! Host application discovery and launchers.
//! Google Chrome remains an optional host application; NOVA never installs it.

use std::io;
use std::path::PathBuf;

#[cfg(windows)]
pub fn find_chrome() -> Option<PathBuf> {
    use std::env;
    let mut candidates = Vec::new();
    for variable in ["PROGRAMFILES", "PROGRAMFILES(X86)", "PROGRAMW6432", "LOCALAPPDATA"] {
        if let Some(base) = env::var_os(variable).filter(|value| !value.is_empty()) {
            let base = PathBuf::from(base);
            candidates.push(base.join("Google/Chrome/Application/chrome.exe"));
        }
    }
    if let Ok(exe) = env::current_exe() {
        if let Some(folder) = exe.parent() {
            candidates.push(folder.join("Apps/GoogleChrome/Application/chrome.exe"));
            candidates.push(folder.join("Apps/Chrome/Application/chrome.exe"));
        }
    }
    candidates.into_iter().find(|candidate| candidate.is_file())
}

#[cfg(not(windows))]
pub fn find_chrome() -> Option<PathBuf> {
    None
}


#[cfg(test)]
fn is_trusted_google_publisher(name: &str) -> bool {
    ["Google LLC", "Google Inc."]
        .iter()
        .any(|trusted| name.trim().eq_ignore_ascii_case(trusted))
}

#[cfg(windows)]
pub(crate) fn verify_google_chrome_signature(chrome: &std::path::Path) -> io::Result<()> {
    use std::os::windows::process::CommandExt;
    use std::process::Command;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let powershell = crate::windows_process::system_binary(r"WindowsPowerShell\v1.0\powershell.exe")?;
    let literal = chrome.to_string_lossy().replace('\'', "''");
    let script = format!(
        "$ErrorActionPreference='Stop'; $sig=Get-AuthenticodeSignature -LiteralPath '{}'; if ($sig.Status -eq 'Valid') {{ $name=$sig.SignerCertificate.GetNameInfo([System.Security.Cryptography.X509Certificates.X509NameType]::SimpleName, $false); if ($name -in @('Google LLC','Google Inc.')) {{ exit 0 }} }}; exit 17",
        literal
    );
    let output = Command::new(powershell)
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .creation_flags(CREATE_NO_WINDOW)
        .output()?;
    if output.status.success() {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Chrome launch blocked: the executable does not have a valid Google LLC/Google Inc. Authenticode signature.",
        ))
    }
}

#[cfg(windows)]
pub fn launch_chrome(chrome: &std::path::Path, url: &str) -> io::Result<()> {
    use std::process::Command;
    if !chrome.is_file() {
        return Err(io::Error::new(io::ErrorKind::NotFound, "Google Chrome executable was not found."));
    }
    verify_google_chrome_signature(chrome)?;
    let url = normalized_browser_target(url)?;
    Command::new(chrome).arg("--new-window").arg(url).spawn()?;
    Ok(())
}

#[cfg(not(windows))]
pub fn launch_chrome(_chrome: &std::path::Path, _url: &str) -> io::Result<()> {
    Err(io::Error::new(io::ErrorKind::Unsupported, "Google Chrome integration is available in the Windows build of NOVA."))
}

pub fn normalized_browser_target(value: &str) -> io::Result<String> {
    let trimmed = value.trim();
    if let Some((scheme, rest)) = trimmed.split_once("://") {
        let normalized = format!("{}://{rest}", scheme.to_ascii_lowercase());
        validate_browser_target(&normalized)?;
        Ok(normalized)
    } else {
        let candidate = format!("https://{trimmed}");
        validate_browser_target(&candidate)?;
        Ok(candidate)
    }
}

pub fn validate_browser_target(value: &str) -> io::Result<&str> {
    let value = value.trim();
    if value.is_empty() || value.len() > 2048 || value.chars().any(char::is_whitespace) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "Enter a valid web address or Chrome internal URL without spaces."));
    }
    let scheme = value.split_once("://").map(|(scheme, _)| scheme.to_ascii_lowercase()).unwrap_or_default();
    let allowed = matches!(scheme.as_str(), "http" | "https" | "chrome" | "chrome-extension");
    if !allowed {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Only http://, https://, chrome:// and chrome-extension:// addresses are supported.",
        ));
    }
    let host_or_path = value.split_once("://").map(|(_, rest)| rest).unwrap_or_default();
    if host_or_path.is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "The address is missing its host or Chrome page."));
    }
    Ok(value)
}

#[cfg(windows)]
pub fn open_chrome_download_page() -> io::Result<()> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    let operation: Vec<u16> = OsStr::new("open").encode_wide().chain(Some(0)).collect();
    let target: Vec<u16> = OsStr::new("https://www.google.com/chrome/").encode_wide().chain(Some(0)).collect();
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
    // ShellExecute returns a value greater than 32 on success.
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            operation.as_ptr(),
            target.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            1,
        ) as isize
    };
    if result <= 32 {
        return Err(io::Error::other(format!("Windows could not open the Chrome download page (ShellExecute code {result}).")));
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn open_chrome_download_page() -> io::Result<()> {
    Err(io::Error::new(io::ErrorKind::Unsupported, "Chrome download page launch is available on Windows."))
}

#[cfg(test)]
mod tests {
    use super::{normalized_browser_target, validate_browser_target};

    #[test]
    fn trusted_google_publisher_names_are_explicit() {
        assert!(super::is_trusted_google_publisher("Google LLC"));
        assert!(super::is_trusted_google_publisher("Google Inc."));
        assert!(!super::is_trusted_google_publisher("Google Lookalike"));
        assert!(!super::is_trusted_google_publisher("Unknown Publisher"));
    }

    #[test]
    fn browser_target_accepts_web_and_chrome_internal_urls() {
        assert_eq!(validate_browser_target("https://example.com").unwrap(), "https://example.com");
        assert_eq!(validate_browser_target("chrome://settings").unwrap(), "chrome://settings");
        assert_eq!(normalized_browser_target("example.com").unwrap(), "https://example.com");
        assert_eq!(normalized_browser_target("HTTPS://example.com").unwrap(), "https://example.com");
    }

    #[test]
    fn browser_target_rejects_untrusted_schemes_and_control_characters() {
        assert!(validate_browser_target("file:///C:/secret.txt").is_err());
        assert!(validate_browser_target("javascript:alert(1)").is_err());
        assert!(validate_browser_target("https://example.com\n--incognito").is_err());
        assert!(validate_browser_target("").is_err());
    }
}
