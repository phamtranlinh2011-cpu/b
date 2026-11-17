use crate::storage::AppPaths;
use std::ffi::OsString;
use std::fs::{self, File};
use std::path::Path;
#[cfg(not(windows))]
use std::fs::OpenOptions;
use std::io::{self, Write};
#[cfg(windows)]
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const PROFILE_PREFIX: &str = "NOVA::";

#[cfg(windows)]
#[repr(C)]
struct SecurityAttributes {
    length: u32,
    security_descriptor: *mut std::ffi::c_void,
    inherit_handle: i32,
}

#[cfg(windows)]
#[link(name = "advapi32")]
unsafe extern "system" {
    fn ConvertStringSecurityDescriptorToSecurityDescriptorW(
        text: *const u16,
        revision: u32,
        descriptor: *mut *mut std::ffi::c_void,
        size: *mut u32,
    ) -> i32;
}

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateFileW(
        name: *const u16,
        access: u32,
        share: u32,
        attributes: *mut SecurityAttributes,
        disposition: u32,
        flags: u32,
        template: *mut std::ffi::c_void,
    ) -> *mut std::ffi::c_void;
    fn LocalFree(memory: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
}


pub fn validate_ssid(ssid: &str) -> io::Result<()> {
    let bytes = ssid.as_bytes();
    if bytes.is_empty() || bytes.len() > 32 || ssid.chars().any(char::is_control) || ssid.contains('"') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "SSID must be 1–32 UTF-8 bytes and contain no control characters.",
        ));
    }
    Ok(())
}

pub fn scan_networks() -> io::Result<Vec<String>> {
    let output = run_netsh(&["wlan", "show", "networks", "mode=bssid"])?;
    if output.timed_out || output.drain_timed_out || !output.status.success() {
        return Err(io::Error::other(output.combined_text()));
    }

    let text = String::from_utf8_lossy(&output.stdout);
    let mut networks = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        let lower = trimmed.to_ascii_lowercase();
        if lower.starts_with("ssid ") {
            if let Some((_, name)) = trimmed.split_once(':') {
                let name = name.trim();
                if !name.is_empty() && !networks.iter().any(|known| known == name) {
                    networks.push(name.to_owned());
                }
            }
        }
    }
    Ok(networks)
}

/// Request disconnection from the active Windows WLAN network.
pub fn disconnect() -> io::Result<String> {
    let output = run_netsh(&["wlan", "disconnect"])?;
    if output.timed_out || output.drain_timed_out || !output.status.success() {
        return Err(io::Error::other(output.combined_text()));
    }
    Ok("Windows accepted the Wi-Fi disconnect request.".to_owned())
}

pub fn connect(paths: &AppPaths, ssid: &str, password: Option<&str>) -> io::Result<String> {
    validate_ssid(ssid)?;
    if let Some(secret) = password {
        validate_password(secret)?;
    }

    let profile_name = format!("{PROFILE_PREFIX}{ssid}");
    let profile = build_profile(ssid, &profile_name, password);
    let mut temp_guard = create_temp_profile_file()?;
    temp_guard.file_mut().write_all(profile.as_bytes())?;
    temp_guard.file_mut().flush()?;
    temp_guard.file_mut().sync_all()?;
    // Close the writer before launching netsh. This avoids requiring netsh's
    // internal file opener to share DELETE access while the XML is imported.
    temp_guard.close_for_import();

    let import_result = run_netsh_import_profile(&temp_guard.path);
    // TempProfileGuard attempts deletion on every exit path, including errors.
    let import = import_result?;
    drop(temp_guard); // Remove the plaintext XML immediately after netsh imports it.
    if import.timed_out || import.drain_timed_out || !import.status.success() {
        return Err(io::Error::other(import.combined_text()));
    }

    if let Err(error) = paths.add_nova_wifi_profile(ssid) {
        // Do not leave an imported profile unmanaged when the local manifest
        // cannot be updated (for example, a read-only USB drive).
        let _ = run_netsh(&["wlan", "delete", "profile", &format!("name=\"{profile_name}\"")]);
        return Err(error);
    }
    let connect = run_netsh(&[
        "wlan",
        "connect",
        &format!("name=\"{profile_name}\""),
        &format!("ssid=\"{ssid}\""),
    ])?;
    if connect.timed_out || connect.drain_timed_out || !connect.status.success() {
        return Err(io::Error::other(connect.combined_text()));
    }

    let mode = if password.is_some() { "secured" } else { "open" };
    Ok(format!(
        "Windows accepted the connection request for '{ssid}' ({mode} NOVA profile). Connection may take a few seconds."
    ))
}

pub fn delete_profile(paths: &AppPaths, ssid: &str) -> io::Result<String> {
    validate_ssid(ssid)?;
    if !paths.is_nova_wifi_profile(ssid)? {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "NOVA has no managed profile for this SSID; refusing to delete another Windows profile.",
        ));
    }

    let profile_name = format!("{PROFILE_PREFIX}{ssid}");
    let output = run_netsh(&["wlan", "delete", "profile", &format!("name=\"{profile_name}\"")])?;
    if output.timed_out || output.drain_timed_out || !output.status.success() {
        return Err(io::Error::other(output.combined_text()));
    }
    paths.remove_nova_wifi_profile(ssid)?;
    Ok(format!("Deleted NOVA-managed Wi-Fi profile for '{ssid}'."))
}

fn validate_password(password: &str) -> io::Result<()> {
    let valid_passphrase = (8..=63).contains(&password.len())
        && password.bytes().all(|byte| (0x20..=0x7E).contains(&byte));
    let valid_hex_key = password.len() == 64 && password.bytes().all(|byte| byte.is_ascii_hexdigit());
    if !valid_passphrase && !valid_hex_key {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "WPA2 password must be 8–63 printable ASCII characters, or a 64-character hexadecimal key.",
        ));
    }
    Ok(())
}

fn build_profile(ssid: &str, profile_name: &str, password: Option<&str>) -> String {
    let ssid_xml = xml_escape(ssid);
    let name_xml = xml_escape(profile_name);
    match password {
        Some(secret) => {
            let key_type = if secret.len() == 64 && secret.chars().all(|ch| ch.is_ascii_hexdigit()) {
                "networkKey"
            } else {
                "passPhrase"
            };
            format!(
                r#"<?xml version="1.0"?>
<WLANProfile xmlns="http://www.microsoft.com/networking/WLAN/profile/v1">
  <name>{name_xml}</name>
  <SSIDConfig><SSID><name>{ssid_xml}</name></SSID></SSIDConfig>
  <connectionType>ESS</connectionType><connectionMode>auto</connectionMode>
  <MSM><security><authEncryption><authentication>WPA2PSK</authentication><encryption>AES</encryption><useOneX>false</useOneX></authEncryption>
  <sharedKey><keyType>{key_type}</keyType><protected>false</protected><keyMaterial>{}</keyMaterial></sharedKey></security></MSM>
</WLANProfile>"#,
                xml_escape(secret)
            )
        },
        None => format!(
            r#"<?xml version="1.0"?>
<WLANProfile xmlns="http://www.microsoft.com/networking/WLAN/profile/v1">
  <name>{name_xml}</name>
  <SSIDConfig><SSID><name>{ssid_xml}</name></SSID></SSIDConfig>
  <connectionType>ESS</connectionType><connectionMode>auto</connectionMode>
  <MSM><security><authEncryption><authentication>open</authentication><encryption>none</encryption><useOneX>false</useOneX></authEncryption></security></MSM>
</WLANProfile>"#
        ),
    }
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

struct TempProfileGuard {
    path: std::path::PathBuf,
    file: Option<File>,
}

impl TempProfileGuard {
    fn file_mut(&mut self) -> &mut File {
        self.file.as_mut().expect("temporary profile file is already closed")
    }

    fn close_for_import(&mut self) {
        drop(self.file.take());
    }
}

impl Drop for TempProfileGuard {
    fn drop(&mut self) {
        // Remove after netsh has exited. If NOVA crashes before this runs, the
        // restrictive file DACL protects the orphan until startup cleanup.
        drop(self.file.take());
        let _ = fs::remove_file(&self.path);
    }
}

/// Build netsh's `filename="..."` token without converting a Windows path
/// through UTF-8/lossy text. This preserves non-ASCII user profile paths.
fn netsh_filename_argument(path: &Path) -> OsString {
    let mut argument = OsString::from("filename=\"");
    argument.push(path.as_os_str());
    argument.push("\"");
    argument
}

fn temp_profile_path() -> io::Result<std::path::PathBuf> {
    static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_nanos();
    let sequence = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    Ok(std::env::temp_dir().join(format!(
        "nova-wlan-{}-{stamp}-{sequence}.xml",
        std::process::id()
    )))
}

const STALE_TEMP_PROFILE_AGE: Duration = Duration::from_secs(10 * 60);

/// Best-effort cleanup for protected WLAN XML files left by an abnormal exit.
/// Only regular files matching NOVA's exact generated filename format and
/// older than the maximum expected WLAN operation window are eligible.
pub fn cleanup_stale_temp_profiles() -> io::Result<usize> {
    let temp_dir = std::env::temp_dir();
    let now = SystemTime::now();
    let mut removed = 0usize;

    for entry in fs::read_dir(temp_dir)? {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        if !is_nova_temp_profile_name(&entry.file_name()) {
            continue;
        }
        let metadata = match fs::symlink_metadata(entry.path()) {
            Ok(metadata) if metadata.file_type().is_file() => metadata,
            _ => continue,
        };
        let is_stale = metadata
            .modified()
            .ok()
            .and_then(|modified| now.duration_since(modified).ok())
            .is_some_and(|age| age >= STALE_TEMP_PROFILE_AGE);
        if is_stale && fs::remove_file(entry.path()).is_ok() {
            removed += 1;
        }
    }
    Ok(removed)
}

fn is_nova_temp_profile_name(name: &std::ffi::OsStr) -> bool {
    let Some(name) = name.to_str() else { return false; };
    let Some(rest) = name.strip_prefix("nova-wlan-") else { return false; };
    let mut parts = rest.split('-');
    let (Some(pid), Some(timestamp), Some(sequence_xml), None) =
        (parts.next(), parts.next(), parts.next(), parts.next()) else {
            return false;
        };
    let Some(sequence) = sequence_xml.strip_suffix(".xml") else { return false; };
    !pid.is_empty()
        && pid.bytes().all(|byte| byte.is_ascii_digit())
        && !timestamp.is_empty()
        && timestamp.bytes().all(|byte| byte.is_ascii_digit())
        && !sequence.is_empty()
        && sequence.bytes().all(|byte| byte.is_ascii_digit())
}

fn create_temp_profile_file() -> io::Result<TempProfileGuard> {
    for _ in 0..8 {
        let path = temp_profile_path()?;
        #[cfg(windows)]
        let created = create_restricted_temp_file(&path);
        #[cfg(not(windows))]
        let created = OpenOptions::new().write(true).create_new(true).open(&path);

        match created {
            Ok(file) => return Ok(TempProfileGuard { path, file: Some(file) }),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "Could not allocate a unique temporary WLAN profile file.",
    ))
}

/// Create a TEMP file with a protected DACL for the file owner, SYSTEM and
/// local Administrators. It is deliberately not held with DELETE access while
/// `netsh` imports it, avoiding share-mode conflicts with the WLAN importer.
#[cfg(windows)]
fn create_restricted_temp_file(path: &std::path::Path) -> io::Result<File> {
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::FromRawHandle;
    use std::ptr;


    const SDDL_REVISION_1: u32 = 1;
    const GENERIC_WRITE: u32 = 0x4000_0000;
    const FILE_SHARE_READ: u32 = 0x0000_0001;
    const FILE_SHARE_WRITE: u32 = 0x0000_0002;
    const FILE_SHARE_DELETE: u32 = 0x0000_0004;
    const CREATE_NEW: u32 = 1;
    const FILE_ATTRIBUTE_TEMPORARY: u32 = 0x0000_0100;

    // OW = Owner Rights; SY = Local System; BA = Built-in Administrators.
    // D:P protects the DACL from inheriting broader permissions from TEMP.
    let sddl: Vec<u16> = "D:P(A;;FA;;;OW)(A;;FA;;;SY)(A;;FA;;;BA)\0".encode_utf16().collect();
    let mut descriptor = ptr::null_mut();
    let converted = unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            SDDL_REVISION_1,
            &mut descriptor,
            ptr::null_mut(), // The descriptor size is optional in the Win32 API.
        )
    };
    if converted == 0 {
        return Err(io::Error::last_os_error());
    }

    let mut security = SecurityAttributes {
        length: std::mem::size_of::<SecurityAttributes>() as u32,
        security_descriptor: descriptor,
        inherit_handle: 0,
    };
    let name: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let handle = unsafe {
        CreateFileW(
            name.as_ptr(),
            GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            &mut security,
            CREATE_NEW,
            FILE_ATTRIBUTE_TEMPORARY,
            ptr::null_mut(),
        )
    };
    let creation_error = if handle == (-1isize as *mut std::ffi::c_void) {
        Some(io::Error::last_os_error())
    } else {
        None
    };
    unsafe { let _ = LocalFree(descriptor); }

    if let Some(error) = creation_error {
        return Err(error);
    }
    // SAFETY: CreateFileW returned a valid, uniquely-owned file handle.
    Ok(unsafe { File::from_raw_handle(handle) })
}

#[cfg(windows)]
fn run_netsh_import_profile(path: &Path) -> io::Result<crate::windows_process::CapturedOutput> {
    let mut command = Command::new(crate::windows_process::system_binary("netsh.exe")?);
    command.args(["wlan", "add", "profile"]);
    command.arg(netsh_filename_argument(path));
    command.arg("user=current");
    crate::windows_process::run_captured(command, Duration::from_secs(20))
}

#[cfg(not(windows))]
fn run_netsh_import_profile(_path: &Path) -> io::Result<crate::windows_process::CapturedOutput> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "Windows WLAN commands are available only in the Windows build of NOVA.",
    ))
}

#[cfg(windows)]
fn run_netsh(args: &[&str]) -> io::Result<crate::windows_process::CapturedOutput> {
    let mut command = Command::new(crate::windows_process::system_binary("netsh.exe")?);
    command.args(args);
    crate::windows_process::run_captured(command, Duration::from_secs(20))
}

#[cfg(not(windows))]
fn run_netsh(_args: &[&str]) -> io::Result<crate::windows_process::CapturedOutput> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "Windows WLAN commands are available only in the Windows build.",
    ))
}

#[cfg(test)]
mod tests {
    use super::{build_profile, create_temp_profile_file, is_nova_temp_profile_name, validate_password, validate_ssid};
    use std::fs;
    use std::io::Write;

    #[test]
    fn xml_escapes_ssid_and_password() {
        let xml = build_profile("Home & Lab", "NOVA::Home & Lab", Some("pass&word123"));
        assert!(xml.contains("Home &amp; Lab"));
        assert!(xml.contains("pass&amp;word123"));
    }

    #[test]
    fn profile_xml_preserves_vietnamese_ssid_and_escapes_xml_metacharacters() {
        let xml = build_profile("Wi-Fi Việt & Lab", "NOVA::Wi-Fi Việt & Lab", Some("password123"));
        assert!(xml.contains("Wi-Fi Việt &amp; Lab"));
        assert!(xml.contains("NOVA::Wi-Fi Việt &amp; Lab"));
        assert!(xml.contains("<keyMaterial>password123</keyMaterial>"));
    }

    #[test]
    fn validates_wpa2_password_length() {
        assert!(validate_password("12345678").is_ok());
        assert!(validate_password("short").is_err());
        assert!(validate_password("pass\nword123").is_err());
        assert!(validate_password("pässword123").is_err());
        // A 64-character hexadecimal string is a valid raw WPA2 PSK,
        // not an overlong passphrase. Non-hex 64-character values are invalid.
        assert!(validate_password(&"a".repeat(64)).is_ok());
        assert!(validate_password(&"g".repeat(64)).is_err());
        assert!(validate_password(&"a1".repeat(32)).is_ok());
    }

    #[test]
    fn temp_profile_guard_closes_and_removes_file_on_drop() {
        let mut guard = create_temp_profile_file().unwrap();
        let path = guard.path.clone();
        guard.file_mut().write_all(b"temporary").unwrap();
        guard.file_mut().flush().unwrap();
        assert!(path.exists());
        // Verify the handoff closes the writer before the importer opens it.
        guard.close_for_import();
        assert_eq!(fs::read(&path).unwrap(), b"temporary");
        drop(guard);
        assert!(!path.exists());
    }

    #[test]
    fn netsh_filename_argument_preserves_unicode_paths() {
        let path = std::path::Path::new(r#"C:\Users\Nguyễn Văn A\AppData\Local\Temp\nova-wlan-1-123-0.xml"#);
        assert_eq!(super::netsh_filename_argument(path).to_string_lossy(), r#"filename="C:\Users\Nguyễn Văn A\AppData\Local\Temp\nova-wlan-1-123-0.xml""#);
    }

    #[test]
    fn stale_profile_name_matcher_accepts_only_generated_names() {
        assert!(is_nova_temp_profile_name(std::ffi::OsStr::new("nova-wlan-123-456789-0.xml")));
        assert!(!is_nova_temp_profile_name(std::ffi::OsStr::new("nova-wlan-not-a-profile.xml")));
        assert!(!is_nova_temp_profile_name(std::ffi::OsStr::new("other-file.xml")));
    }

    #[test]
    fn temp_profile_guard_removes_file_when_dropped_with_handle_open() {
        let guard = create_temp_profile_file().unwrap();
        let path = guard.path.clone();
        assert!(path.exists());
        drop(guard);
        assert!(!path.exists());
    }

    #[test]
    fn validates_ssid_byte_length() {
        assert!(validate_ssid("Home WiFi").is_ok());
        assert!(validate_ssid("").is_err());
        assert!(validate_ssid(&"a".repeat(33)).is_err());
        assert!(validate_ssid("bad\nssid").is_err());
        assert!(validate_ssid("bad\"ssid").is_err());
    }
}
