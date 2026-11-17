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
    use std::process::Command;
    use std::time::Duration;

    if !chrome.is_file() {
        return Err(io::Error::new(io::ErrorKind::NotFound, "Google Chrome executable was not found."));
    }
    let powershell = crate::windows_process::system_binary(r"WindowsPowerShell\v1.0\powershell.exe")?;
    // Keep the path out of PowerShell source text and preserve Windows Unicode
    // paths by passing it through the process environment.
    let script = "$ErrorActionPreference='Stop'; $sig=Get-AuthenticodeSignature -LiteralPath $env:NOVA_VERIFY_PATH; if ($sig.Status -eq 'Valid') { $name=$sig.SignerCertificate.GetNameInfo([System.Security.Cryptography.X509Certificates.X509NameType]::SimpleName, $false); if ($name -in @('Google LLC','Google Inc.')) { exit 0 } }; exit 17";
    let mut command = Command::new(powershell);
    command
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .env("NOVA_VERIFY_PATH", chrome);
    let output = crate::windows_process::run_captured(command, Duration::from_secs(15))?;
    if output.timed_out || output.drain_timed_out {
        return Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "Chrome publisher verification timed out. Check Windows certificate services and try again.",
        ));
    }
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
pub fn launch_chrome(chrome: &std::path::Path) -> io::Result<()> {
    use std::process::Command;
    if !chrome.is_file() {
        return Err(io::Error::new(io::ErrorKind::NotFound, "Google Chrome executable was not found."));
    }
    let launch_guard = crate::windows_process::hold_verified_executable(chrome)?;
    let executable = launch_guard.path();
    verify_google_chrome_signature(executable)?;
    // Keep the executable and its parent directories locked against replacement
    // from publisher verification until Windows has created the process.
    // Launch the user's installed Chrome with its normal Windows profile,
    // extensions, bookmarks, cookies, native frame and default new-tab page.
    Command::new(executable).arg("--new-window").spawn()?;
    Ok(())
}

#[cfg(not(windows))]
pub fn launch_chrome(_chrome: &std::path::Path) -> io::Result<()> {
    Err(io::Error::new(io::ErrorKind::Unsupported, "Google Chrome integration is available in the Windows build of NOVA."))
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


    #[test]
    fn trusted_google_publisher_names_are_explicit() {
        assert!(super::is_trusted_google_publisher("Google LLC"));
        assert!(super::is_trusted_google_publisher("Google Inc."));
        assert!(!super::is_trusted_google_publisher("Google Lookalike"));
        assert!(!super::is_trusted_google_publisher("Unknown Publisher"));
    }




}
