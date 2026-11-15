//! Portable integration with the official UniKey Windows Vietnamese input app.
//!
//! The official helper is embedded into NOVA at compile time so a replaced
//! PowerShell file on the USB cannot change the code behind the Download action.
//! It fetches the official x64 ZIP, verifies Authenticode, and places UniKeyNT.exe
//! under Apps/UniKey on the USB drive.

use std::io;
use std::path::{Path, PathBuf};

pub const OFFICIAL_DOWNLOAD_PAGE: &str = "https://www.unikey.org/download.html";

// Embed the helper into NOVA so a modified PowerShell file on the USB cannot
// replace the code executed by the Download button.
const DOWNLOAD_HELPER: &str = include_str!("../scripts/Download-UniKey.ps1");

pub fn find_portable(app_dir: &Path) -> Option<PathBuf> {
    let root = app_dir.canonicalize().ok()?;
    let candidates = [
        root.join("Apps/UniKey/UniKeyNT.exe"),
        root.join("Apps/UniKey/UniKey.exe"),
        root.join("UniKeyNT.exe"),
    ];
    candidates.into_iter().find(|path| {
        let Ok(canonical) = path.canonicalize() else { return false; };
        if !canonical.starts_with(&root) || !canonical.is_file() { return false; }

        // Check each existing component rather than trusting canonicalization
        // alone; a USB junction must not silently redirect the companion.
        let mut current = path.as_path();
        loop {
            let Ok(metadata) = std::fs::symlink_metadata(current) else { return false; };
            if crate::storage::is_link_or_reparse_point(&metadata) { return false; }
            if current == root.as_path() { break; }
            let Some(parent) = current.parent() else { return false; };
            current = parent;
        }
        true
    }).and_then(|path| path.canonicalize().ok())
}

#[cfg(windows)]
pub fn verify_official_signature(path: &Path) -> io::Result<()> {
    use std::process::Command;
    use std::time::Duration;

    if !path.is_file() {
        return Err(io::Error::new(io::ErrorKind::NotFound, "UniKey executable was not found."));
    }
    let powershell = crate::windows_process::system_binary("WindowsPowerShell\\v1.0\\powershell.exe")?;
    // Pass the path as an environment value instead of interpolating it into
    // PowerShell source. This preserves Unicode and avoids script injection
    // through unusual file names.
    let script = "$ErrorActionPreference='Stop'; $sig=Get-AuthenticodeSignature -LiteralPath $env:NOVA_VERIFY_PATH; if ($sig.Status -eq 'Valid' -and $sig.SignerCertificate.GetNameInfo([System.Security.Cryptography.X509Certificates.X509NameType]::SimpleName, $false) -ceq 'PHAM KIM LONG') { exit 0 }; exit 17";
    let mut command = Command::new(powershell);
    command
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .env("NOVA_VERIFY_PATH", path);
    let output = crate::windows_process::run_captured(command, Duration::from_secs(15))?;
    if output.timed_out || output.drain_timed_out {
        return Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "UniKey signature verification timed out. Check Windows certificate services and try again.",
        ));
    }
    if output.status.success() {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "UniKey signature is invalid or is not issued to PHAM KIM LONG. Do not run this file.",
        ))
    }
}

#[cfg(not(windows))]
pub fn verify_official_signature(_path: &Path) -> io::Result<()> {
    Err(io::Error::new(io::ErrorKind::Unsupported, "UniKey integration is available in the Windows build of NOVA."))
}

#[cfg(windows)]
pub fn launch_verified(path: &Path) -> io::Result<std::process::Child> {
    use std::process::Command;
    verify_official_signature(path)?;
    let working_directory = path.parent().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "UniKey executable has no parent directory.")
    })?;
    // Keep UniKey's relative configuration/resource files beside the USB copy
    // instead of inheriting NOVA's potentially unrelated working directory.
    Command::new(path).current_dir(working_directory).spawn()
}

#[cfg(not(windows))]
pub fn launch_verified(_path: &Path) -> io::Result<std::process::Child> {
    Err(io::Error::new(io::ErrorKind::Unsupported, "UniKey integration is available in the Windows build of NOVA."))
}

#[cfg(windows)]
fn base64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let a = chunk[0] as u32;
        let b = chunk.get(1).copied().unwrap_or(0) as u32;
        let c = chunk.get(2).copied().unwrap_or(0) as u32;
        let bits = (a << 16) | (b << 8) | c;
        output.push(TABLE[((bits >> 18) & 0x3f) as usize] as char);
        output.push(TABLE[((bits >> 12) & 0x3f) as usize] as char);
        output.push(if chunk.len() > 1 { TABLE[((bits >> 6) & 0x3f) as usize] as char } else { '=' });
        output.push(if chunk.len() > 2 { TABLE[(bits & 0x3f) as usize] as char } else { '=' });
    }
    output
}

#[cfg(windows)]
fn powershell_encoded_command(script: &str) -> String {
    let bytes: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    base64_encode(&bytes)
}

#[cfg(windows)]
pub fn start_download_helper(app_dir: &Path) -> io::Result<std::process::Child> {
    use std::process::Command;
    let powershell = crate::windows_process::system_binary("WindowsPowerShell\\v1.0\\powershell.exe")?;
    let encoded = powershell_encoded_command(DOWNLOAD_HELPER);
    Command::new(powershell)
        .args(["-NoProfile", "-NonInteractive", "-EncodedCommand", &encoded])
        .env("NOVA_APP_ROOT", app_dir)
        .spawn()
}

#[cfg(not(windows))]
pub fn start_download_helper(_app_dir: &Path) -> io::Result<std::process::Child> {
    Err(io::Error::new(io::ErrorKind::Unsupported, "UniKey download helper is available on Windows."))
}

#[cfg(windows)]
pub fn open_official_download_page() -> io::Result<()> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    let operation: Vec<u16> = OsStr::new("open").encode_wide().chain(Some(0)).collect();
    let target: Vec<u16> = OsStr::new(OFFICIAL_DOWNLOAD_PAGE).encode_wide().chain(Some(0)).collect();
    #[link(name = "shell32")]
    unsafe extern "system" {
        fn ShellExecuteW(window: *mut std::ffi::c_void, operation: *const u16, file: *const u16,
            parameters: *const u16, directory: *const u16, show_command: i32) -> *mut std::ffi::c_void;
    }
    let result = unsafe { ShellExecuteW(std::ptr::null_mut(), operation.as_ptr(), target.as_ptr(),
        std::ptr::null(), std::ptr::null(), 1) as isize };
    if result <= 32 { return Err(io::Error::other(format!("Could not open official UniKey site (ShellExecute code {result})."))); }
    Ok(())
}

#[cfg(not(windows))]
pub fn open_official_download_page() -> io::Result<()> {
    Err(io::Error::new(io::ErrorKind::Unsupported, "UniKey website launch is available on Windows."))
}

#[cfg(test)]
mod tests {
    use super::find_portable;
    use std::fs;

    #[test]
    fn finds_only_known_portable_unikey_locations() {
        let root = std::env::temp_dir().join(format!("nova-unikey-test-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let target = root.join("Apps/UniKey/UniKeyNT.exe");
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(&target, b"test placeholder").unwrap();
        assert_eq!(find_portable(&root), Some(target));
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn refuses_unikey_below_a_symlinked_apps_directory() {
        use std::os::unix::fs::symlink;
        let root = std::env::temp_dir().join(format!("nova-unikey-link-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let real_apps = root.join("RealApps/UniKey");
        let linked_apps = root.join("Apps");
        std::fs::create_dir_all(&real_apps).unwrap();
        std::fs::write(real_apps.join("UniKeyNT.exe"), b"placeholder").unwrap();
        symlink(root.join("RealApps"), &linked_apps).unwrap();
        assert_eq!(find_portable(&root), None);
        let _ = fs::remove_file(&linked_apps);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn does_not_find_missing_unikey() {
        let root = std::env::temp_dir().join(format!("nova-unikey-missing-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        assert_eq!(find_portable(&root), None);
    }

    #[cfg(windows)]
    #[test]
    fn powershell_encoding_is_utf16le_base64() {
        let encoded = super::powershell_encoded_command("Write-Output 'Tiếng Việt'");
        assert!(!encoded.contains(' '));
        assert!(encoded.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '+' || ch == '/' || ch == '='));
    }

}
