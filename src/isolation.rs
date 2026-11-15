//! Disposable Windows Sandbox launch helpers.
//!
//! NOVA itself is a normal Windows desktop process. This module creates
//! Windows Sandbox (.wsb) sessions for explicitly isolated browser/file work.
//! It deliberately exposes no writable host folder and disables clipboard
//! redirection. The only mapped host folders are read-only.

use crate::storage::{canonical_user_path, ensure_real_child, remove_tree_without_following_reparse_points, AppPaths};
use std::io;
#[cfg(windows)]
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static SESSION_COUNTER: AtomicU64 = AtomicU64::new(1);
pub const MAX_SANDBOX_SAMPLE_BYTES: u64 = 512 * 1024 * 1024;

fn is_nova_sandbox_config_name(name: &std::ffi::OsStr) -> bool {
    let Some(name) = name.to_str() else { return false; };
    let name = name.to_ascii_lowercase();
    (name.starts_with("browser-") || name.starts_with("sample-")) && name.ends_with(".wsb")
}

#[cfg(windows)]
pub fn is_available() -> bool {
    crate::windows_process::system_binary("WindowsSandbox.exe")
        .map(|path| path.is_file())
        .unwrap_or(false)
}

#[cfg(not(windows))]
pub fn is_available() -> bool { false }

#[cfg(windows)]
fn create_session_id() -> String {
    let time = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis();
    let counter = SESSION_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{}-{time}-{counter}", std::process::id())
}

#[cfg(windows)]
fn xml_escape(value: &str) -> String {
    value.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
        .replace('"', "&quot;").replace('\'', "&apos;")
}

#[cfg(windows)]
fn base64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let a = chunk[0] as u32;
        let b = chunk.get(1).copied().unwrap_or(0) as u32;
        let c = chunk.get(2).copied().unwrap_or(0) as u32;
        let bits = (a << 16) | (b << 8) | c;
        out.push(TABLE[((bits >> 18) & 0x3f) as usize] as char);
        out.push(TABLE[((bits >> 12) & 0x3f) as usize] as char);
        out.push(if chunk.len() > 1 { TABLE[((bits >> 6) & 0x3f) as usize] as char } else { '=' });
        out.push(if chunk.len() > 2 { TABLE[(bits & 0x3f) as usize] as char } else { '=' });
    }
    out
}

#[cfg(windows)]
fn powershell_encoded_command(script: &str) -> String {
    let bytes: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    base64_encode(&bytes)
}

fn canonical_security_dir(paths: &AppPaths) -> io::Result<PathBuf> {
    let security_root = ensure_real_child(&paths.data_dir, "Security")?;
    if security_root != paths.security_dir.canonicalize()? {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "NOVA Security folder must be the real Security child of NOVA Data; links and redirected paths are blocked.",
        ));
    }
    Ok(security_root)
}

fn canonical_quarantine_dir(paths: &AppPaths) -> io::Result<PathBuf> {
    let security_root = canonical_security_dir(paths)?;
    ensure_real_child(&security_root, "Quarantine")
}

#[cfg(windows)]
fn create_wsb_file(paths: &AppPaths, name: &str, config: &str) -> io::Result<PathBuf> {
    use std::fs::OpenOptions;
    use std::io::Write;
    let security_root = canonical_security_dir(paths)?;
    let path = security_root.join(name);
    let mut file = OpenOptions::new().write(true).create_new(true).open(&path)?;
    if let Err(error) = (|| { file.write_all(config.as_bytes())?; file.sync_all() })() {
        drop(file);
        let _ = std::fs::remove_file(&path);
        return Err(error);
    }
    drop(file);
    Ok(path)
}

#[cfg(windows)]
fn launch_wsb(config_path: &Path) -> io::Result<()> {
    use std::process::Command;
    let sandbox = crate::windows_process::system_binary("WindowsSandbox.exe")?;
    if !sandbox.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "Windows Sandbox is not enabled or is not supported on this Windows edition. Enable the optional Windows Sandbox feature on Windows Pro/Enterprise/Education and enable CPU virtualization.",
        ));
    }
    Command::new(sandbox).arg(config_path).spawn()?;
    Ok(())
}

#[cfg(windows)]
fn validate_browser_url(url: &str) -> io::Result<()> {
    let Some((scheme, remainder)) = url.split_once("://") else {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "Only HTTP and HTTPS addresses are allowed in isolated Chrome."));
    };
    let authority = remainder.split(|ch| ch == '/' || ch == '?' || ch == '#').next().unwrap_or_default();
    if url.len() > 2048
        || !(scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https"))
        || authority.is_empty()
        || authority.contains('@')
        || url.chars().any(|ch| ch.is_control() || ch.is_whitespace() || ch == '"' || ch == '\\')
    {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "The browser address must be a valid-looking HTTP(S) URL without whitespace, user-info or control characters."));
    }
    Ok(())
}

/// Start Chrome inside a clean Windows Sandbox profile. The only host mapping
/// is Chrome's application directory, mounted read-only. User profile, cookies,
/// extensions, downloads and browser cache are created inside the disposable VM.
#[cfg(windows)]
pub fn launch_isolated_chrome(paths: &AppPaths, chrome_exe: &Path, url: &str) -> io::Result<PathBuf> {
    validate_browser_url(url)?;
    let chrome_exe = chrome_exe.canonicalize()?;
    crate::apps::verify_google_chrome_signature(&chrome_exe)?;
    if !chrome_exe.is_file() || !chrome_exe.file_name().is_some_and(|name| name.eq_ignore_ascii_case("chrome.exe")) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "The selected browser executable is not chrome.exe."));
    }
    let chrome_dir = chrome_exe.parent().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "Chrome has no application directory."))?;
    let escaped_host_dir = xml_escape(&chrome_dir.to_string_lossy());
    let safe_url = url.replace('\'', "''");
    let script = format!(
        "$ErrorActionPreference='Stop'; $chrome='C:\\NOVAChrome\\chrome.exe'; $url='{}'; $argUrl='\"' + $url + '\"'; $args=@('--user-data-dir=C:\\NOVAProfile','--no-first-run','--no-default-browser-check','--disable-default-apps',$argUrl); if (Test-Path -LiteralPath $chrome) {{ Start-Process -FilePath $chrome -ArgumentList $args }} else {{ throw 'Read-only mapped Chrome executable is unavailable.' }}",
        safe_url
    );
    let encoded = powershell_encoded_command(&script);
    let config = format!(
        "<Configuration>\n  <VGpu>Disable</VGpu>\n  <Networking>Enable</Networking>\n  <AudioInput>Disable</AudioInput>\n  <VideoInput>Disable</VideoInput>\n  <PrinterRedirection>Disable</PrinterRedirection>\n  <ClipboardRedirection>Disable</ClipboardRedirection>\n  <ProtectedClient>Enable</ProtectedClient>\n  <MappedFolders>\n    <MappedFolder>\n      <HostFolder>{escaped_host_dir}</HostFolder>\n      <SandboxFolder>C:\\NOVAChrome</SandboxFolder>\n      <ReadOnly>true</ReadOnly>\n    </MappedFolder>\n  </MappedFolders>\n  <LogonCommand>\n    <Command>powershell.exe -NoProfile -NonInteractive -EncodedCommand {encoded}</Command>\n  </LogonCommand>\n</Configuration>\n"
    );
    let config_path = create_wsb_file(paths, &format!("Browser-{}.wsb", create_session_id()), &config)?;
    if let Err(error) = launch_wsb(&config_path) {
        let _ = std::fs::remove_file(&config_path);
        return Err(error);
    }
    Ok(config_path)
}

#[cfg(not(windows))]
pub fn launch_isolated_chrome(_paths: &AppPaths, _chrome_exe: &Path, _url: &str) -> io::Result<PathBuf> {
    Err(io::Error::new(io::ErrorKind::Unsupported, "Windows Sandbox integration is available only in the Windows build."))
}

/// Copy one selected NOVA workspace file into a unique staging folder and map
/// only that folder read-only into an offline disposable Windows Sandbox.
/// The original file is never mapped and cannot be changed by the sandbox.
#[cfg(windows)]
pub fn launch_sample_in_sandbox(paths: &AppPaths, source: &Path) -> io::Result<PathBuf> {
    let source_metadata = std::fs::symlink_metadata(source)?;
    if crate::storage::is_link_or_reparse_point(&source_metadata) {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "Links and reparse points cannot be staged for Sandbox inspection."));
    }
    let canonical = canonical_user_path(&paths.user_dir, source)?;
    let metadata = std::fs::metadata(&canonical)?;
    if !metadata.is_file() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "Only a single file can be inspected in Windows Sandbox."));
    }
    if metadata.len() > MAX_SANDBOX_SAMPLE_BYTES {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "Files larger than 512 MiB are not staged for Windows Sandbox."));
    }

    let session_id = create_session_id();
    let quarantine_root = canonical_quarantine_dir(paths)?;
    let sample_dir = quarantine_root.join(&session_id);
    std::fs::create_dir(&sample_dir)?;
    let sample_dir_metadata = std::fs::symlink_metadata(&sample_dir)?;
    if !sample_dir_metadata.is_dir() || crate::storage::is_link_or_reparse_point(&sample_dir_metadata) {
        let _ = remove_tree_without_following_reparse_points(&sample_dir);
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "NOVA sample staging folder was redirected."));
    }
    let extension = canonical.extension().and_then(|value| value.to_str()).unwrap_or("");
    let extension = extension.chars().take(16).filter(|ch| ch.is_ascii_alphanumeric()).collect::<String>();
    let staged_name = if extension.is_empty() { "sample.bin".to_owned() } else { format!("sample.{extension}") };
    let staged_path = sample_dir.join(staged_name);

    let copy_result = (|| {
        let source_file = std::fs::File::open(&canonical)?;
        let opened_metadata = source_file.metadata()?;
        let current_path_metadata = std::fs::symlink_metadata(&canonical)?;
        if !opened_metadata.is_file()
            || opened_metadata.len() > MAX_SANDBOX_SAMPLE_BYTES
            || crate::storage::is_link_or_reparse_point(&current_path_metadata)
            || !current_path_metadata.is_file()
        {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "The selected file changed or is not a regular file suitable for Sandbox staging."));
        }
        let expected_size = opened_metadata.len();
        let mut bounded_source = source_file.take(MAX_SANDBOX_SAMPLE_BYTES + 1);
        let mut staged_file = std::fs::OpenOptions::new().write(true).create_new(true).open(&staged_path)?;
        let copied = io::copy(&mut bounded_source, &mut staged_file)?;
        if copied > MAX_SANDBOX_SAMPLE_BYTES || copied != expected_size {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "The selected file changed while NOVA was staging it."));
        }
        staged_file.sync_all()?;
        Ok(())
    })();
    if let Err(error) = copy_result {
        let _ = remove_tree_without_following_reparse_points(&sample_dir);
        return Err(error);
    }

    let host_folder = match canonical_user_path(&quarantine_root, &sample_dir) {
        Ok(folder) => folder,
        Err(error) => {
            let _ = remove_tree_without_following_reparse_points(&sample_dir);
            return Err(error);
        }
    };
    let escaped_host = xml_escape(&host_folder.to_string_lossy());
    let config = format!(
        "<Configuration>\n  <VGpu>Disable</VGpu>\n  <Networking>Disable</Networking>\n  <AudioInput>Disable</AudioInput>\n  <VideoInput>Disable</VideoInput>\n  <PrinterRedirection>Disable</PrinterRedirection>\n  <ClipboardRedirection>Disable</ClipboardRedirection>\n  <ProtectedClient>Enable</ProtectedClient>\n  <MappedFolders>\n    <MappedFolder>\n      <HostFolder>{escaped_host}</HostFolder>\n      <SandboxFolder>C:\\NOVA_SAMPLE</SandboxFolder>\n      <ReadOnly>true</ReadOnly>\n    </MappedFolder>\n  </MappedFolders>\n  <LogonCommand><Command>explorer.exe C:\\NOVA_SAMPLE</Command></LogonCommand>\n</Configuration>\n"
    );
    let config_path = create_wsb_file(paths, &format!("Sample-{}.wsb", session_id), &config)?;
    if let Err(error) = launch_wsb(&config_path) {
        let _ = std::fs::remove_file(&config_path);
        let _ = remove_tree_without_following_reparse_points(&sample_dir);
        return Err(error);
    }
    Ok(config_path)
}

#[cfg(not(windows))]
pub fn launch_sample_in_sandbox(_paths: &AppPaths, _source: &Path) -> io::Result<PathBuf> {
    Err(io::Error::new(io::ErrorKind::Unsupported, "Windows Sandbox integration is available only in the Windows build."))
}

pub fn quarantine_item_count(paths: &AppPaths) -> io::Result<usize> {
    let security_root = canonical_security_dir(paths)?;
    let root = canonical_quarantine_dir(paths)?;
    let staged_entries = std::fs::read_dir(root)?.collect::<io::Result<Vec<_>>>()?;
    let mut count = staged_entries.len();
    // Count only NOVA-generated launch configurations; unrelated user .wsb
    // files in Security are not part of this cleanup workspace.
    for entry in std::fs::read_dir(&security_root)? {
        let entry = entry?;
        let metadata = std::fs::symlink_metadata(entry.path())?;
        if metadata.is_file()
            && !crate::storage::is_link_or_reparse_point(&metadata)
            && is_nova_sandbox_config_name(&entry.file_name())
        {
            count += 1;
        }
    }
    Ok(count)
}

/// Remove staged sample copies and NOVA-created Sandbox launch configs. Call
/// this only after all Windows Sandbox windows have been closed; original
/// workspace files and the installed Chrome application are never touched.
pub fn clear_quarantine(paths: &AppPaths) -> io::Result<usize> {
    let security_root = canonical_security_dir(paths)?;
    let root = canonical_quarantine_dir(paths)?;
    let entries = std::fs::read_dir(&root)?.collect::<io::Result<Vec<_>>>()?;
    let mut removed = 0;
    for entry in entries {
        remove_tree_without_following_reparse_points(&entry.path())?;
        removed += 1;
    }
    for entry in std::fs::read_dir(&security_root)? {
        let entry = entry?;
        if !is_nova_sandbox_config_name(&entry.file_name()) {
            continue;
        }
        let metadata = std::fs::symlink_metadata(entry.path())?;
        if metadata.is_file() && !crate::storage::is_link_or_reparse_point(&metadata) {
            std::fs::remove_file(entry.path())?;
            removed += 1;
        }
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use crate::storage::AppPaths;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_paths() -> AppPaths {
        let unique = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let data = std::env::temp_dir().join(format!("nova-isolation-test-{}-{unique}", std::process::id()));
        let security = data.join("Security");
        let user = data.join("Users/Default");
        fs::create_dir_all(security.join("Quarantine")).unwrap();
        fs::create_dir_all(&user).unwrap();
        AppPaths {
            app_dir: data.clone(),
            data_dir: data.clone(),
            security_dir: security,
            user_dir: user,
            settings_file: data.join("Settings/settings.json"),
            wifi_manifest: data.join("Settings/wifi-profiles.json"),
            trash_dir: data.join("Trash"),
            portable: true,
        }
    }

    #[test]
    fn clear_quarantine_preserves_unrelated_wsb_files() {
        let paths = test_paths();
        let unrelated = paths.security_dir.join("MySandbox.wsb");
        let generated = paths.security_dir.join("Browser-session.wsb");
        fs::write(&unrelated, "user-owned config").unwrap();
        fs::write(&generated, "nova config").unwrap();

        assert_eq!(super::quarantine_item_count(&paths).unwrap(), 1);
        assert_eq!(super::clear_quarantine(&paths).unwrap(), 1);
        assert!(unrelated.exists());
        assert!(!generated.exists());
        let _ = fs::remove_dir_all(paths.data_dir);
    }

    #[test]
    fn clear_quarantine_removes_staging_and_configs_but_not_original_file() {
        let paths = test_paths();
        let original = paths.user_dir.join("original.txt");
        fs::write(&original, "keep original").unwrap();
        let staged_dir = paths.security_dir.join("Quarantine/session-1");
        fs::create_dir_all(&staged_dir).unwrap();
        fs::write(staged_dir.join("sample.bin"), b"sample").unwrap();
        fs::write(paths.security_dir.join("Browser-session.wsb"), "https://example.invalid").unwrap();

        assert_eq!(super::quarantine_item_count(&paths).unwrap(), 2);
        assert_eq!(super::clear_quarantine(&paths).unwrap(), 2);
        assert_eq!(super::quarantine_item_count(&paths).unwrap(), 0);
        assert_eq!(fs::read_to_string(original).unwrap(), "keep original");
        let _ = fs::remove_dir_all(paths.data_dir);
    }

    #[cfg(unix)]
    #[test]
    fn quarantine_rejects_a_symlink_to_an_external_folder() {
        use std::os::unix::fs::symlink;
        let paths = test_paths();
        let external = paths.data_dir.parent().unwrap().join(format!("nova-isolation-external-{}", std::process::id()));
        fs::create_dir_all(&external).unwrap();
        let keep = external.join("keep.txt");
        fs::write(&keep, "do not delete").unwrap();
        let quarantine = paths.security_dir.join("Quarantine");
        fs::remove_dir(&quarantine).unwrap();
        symlink(&external, &quarantine).unwrap();

        assert!(super::quarantine_item_count(&paths).is_err());
        assert!(super::clear_quarantine(&paths).is_err());
        assert_eq!(fs::read_to_string(&keep).unwrap(), "do not delete");

        let _ = fs::remove_file(&quarantine);
        let _ = fs::remove_dir_all(external);
        let _ = fs::remove_dir_all(paths.data_dir);
    }

    #[test]
    fn only_nova_named_sandbox_configs_are_selected_for_cleanup() {
        use super::is_nova_sandbox_config_name;
        use std::ffi::OsStr;
        assert!(is_nova_sandbox_config_name(OsStr::new("Browser-123.wsb")));
        assert!(is_nova_sandbox_config_name(OsStr::new("Sample-123.wsb")));
        assert!(is_nova_sandbox_config_name(OsStr::new("browser-123.WSB")));
        assert!(!is_nova_sandbox_config_name(OsStr::new("MySandbox.wsb")));
        assert!(!is_nova_sandbox_config_name(OsStr::new("Browser-123.txt")));
        assert!(!is_nova_sandbox_config_name(OsStr::new("Browser-.wsb.bak")));
    }

    #[cfg(windows)]
    use super::{base64_encode, powershell_encoded_command, xml_escape};

    #[cfg(windows)]
    #[test]
    fn isolated_chrome_accepts_only_http_and_https_urls() {
        use super::validate_browser_url;
        assert!(validate_browser_url("https://example.com/path?q=1").is_ok());
        assert!(validate_browser_url("HTTP://example.com").is_ok());
        for unsafe_url in [
            "file:///C:/Windows/win.ini",
            "javascript:alert(1)",
            "--user-data-dir=C:/Temp",
            "https://",
            "https://user@example.com",
            "https://example.com/a b",
            "https://example.com\\path",
        ] {
            assert!(validate_browser_url(unsafe_url).is_err(), "accepted {unsafe_url}");
        }
    }

    #[cfg(windows)]
    #[test]
    fn xml_escape_protects_windows_paths() {
        assert_eq!(xml_escape(r"C:\NOVA & Data\Chrome"), r"C:\NOVA &amp; Data\Chrome");
    }

    #[cfg(windows)]
    #[test]
    fn base64_encoder_handles_padding() {
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
    }

    #[cfg(windows)]
    #[test]
    fn powershell_command_is_encoded_as_utf16le_base64() {
        let encoded = powershell_encoded_command("Write-Output 'Tiếng Việt'");
        assert!(!encoded.contains(' '));
        assert!(encoded.ends_with('=' ) || encoded.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '+' || ch == '/'));
    }
    #[cfg(unix)]
    #[test]
    fn clear_quarantine_does_not_follow_nested_symlinks() {
        use std::os::unix::fs::symlink;
        let paths = test_paths();
        let external = paths.data_dir.parent().unwrap().join(format!("nova-isolation-nested-external-{}", std::process::id()));
        fs::create_dir_all(&external).unwrap();
        fs::write(external.join("keep.txt"), "must survive").unwrap();
        let staged = paths.security_dir.join("Quarantine/session-linked");
        fs::create_dir_all(&staged).unwrap();
        symlink(&external, staged.join("external-link")).unwrap();

        super::clear_quarantine(&paths).unwrap();
        assert_eq!(fs::read_to_string(external.join("keep.txt")).unwrap(), "must survive");
        let _ = fs::remove_dir_all(external);
        let _ = fs::remove_dir_all(paths.data_dir);
    }

}
