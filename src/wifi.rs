use crate::storage::AppPaths;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

const PROFILE_PREFIX: &str = "NOVA::";

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
    if !output.status.success() {
        return Err(io::Error::other(format_output(&output.stdout, &output.stderr)));
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

pub fn connect(paths: &AppPaths, ssid: &str, password: Option<&str>) -> io::Result<String> {
    validate_ssid(ssid)?;
    if let Some(secret) = password {
        validate_password(secret)?;
    }

    let profile_name = format!("{PROFILE_PREFIX}{ssid}");
    let profile = build_profile(ssid, &profile_name, password);
    let temp_path = temp_profile_path()?;
    let temp_guard = TempProfileGuard(temp_path.clone());

    let write_result = (|| -> io::Result<()> {
        let mut file = OpenOptions::new().write(true).create_new(true).open(&temp_path)?;
        file.write_all(profile.as_bytes())?;
        file.flush()?;
        Ok(())
    })();
    if let Err(error) = write_result {
        let _ = fs::remove_file(&temp_path);
        return Err(error);
    }

    let path_arg = temp_path.to_string_lossy().to_string();
    let import_result = run_netsh(&[
        "wlan",
        "add",
        "profile",
        &format!("filename=\"{path_arg}\""),
        "user=current",
    ]);
    // TempProfileGuard retries deletion on every exit path, including errors.
    let import = import_result?;
    drop(temp_guard); // Remove the plaintext XML immediately after netsh imports it.
    if !import.status.success() {
        return Err(io::Error::other(format_output(&import.stdout, &import.stderr)));
    }

    paths.add_nova_wifi_profile(ssid)?;
    let connect = run_netsh(&[
        "wlan",
        "connect",
        &format!("name=\"{profile_name}\""),
        &format!("ssid=\"{ssid}\""),
    ])?;
    if !connect.status.success() {
        return Err(io::Error::other(format_output(&connect.stdout, &connect.stderr)));
    }

    let mode = if password.is_some() { "secured" } else { "open" };
    Ok(format!(
        "Windows accepted the connection request for '{ssid}' ({mode} NOVA profile). Connection may take a few seconds."
    ))
}

pub fn delete_profile(paths: &AppPaths, ssid: &str) -> io::Result<String> {
    validate_ssid(ssid)?;
    if !paths.is_nova_wifi_profile(ssid) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "NOVA has no managed profile for this SSID; refusing to delete another Windows profile.",
        ));
    }

    let profile_name = format!("{PROFILE_PREFIX}{ssid}");
    let output = run_netsh(&["wlan", "delete", "profile", &format!("name=\"{profile_name}\"")])?;
    if !output.status.success() {
        return Err(io::Error::other(format_output(&output.stdout, &output.stderr)));
    }
    paths.remove_nova_wifi_profile(ssid)?;
    Ok(format!("Deleted NOVA-managed Wi-Fi profile for '{ssid}'."))
}

fn validate_password(password: &str) -> io::Result<()> {
    let valid_length = (8..=63).contains(&password.len());
    let valid_hex_key = password.len() == 64 && password.chars().all(|ch| ch.is_ascii_hexdigit());
    if !valid_length && !valid_hex_key {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "WPA2 password must be 8–63 characters, or a 64-character hexadecimal key.",
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

struct TempProfileGuard(std::path::PathBuf);

impl Drop for TempProfileGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn temp_profile_path() -> io::Result<std::path::PathBuf> {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_nanos();
    Ok(std::env::temp_dir().join(format!("nova-wlan-{}-{stamp}.xml", std::process::id())))
}

#[cfg(windows)]
fn run_netsh(args: &[&str]) -> io::Result<std::process::Output> {
    Command::new("netsh.exe")
        .args(args)
        .stdin(Stdio::null())
        .output()
}

#[cfg(not(windows))]
fn run_netsh(_args: &[&str]) -> io::Result<std::process::Output> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "Windows WLAN commands are available only in the Windows build.",
    ))
}

fn format_output(stdout: &[u8], stderr: &[u8]) -> String {
    let mut combined = String::from_utf8_lossy(stdout).trim().to_owned();
    let err = String::from_utf8_lossy(stderr).trim().to_owned();
    if !err.is_empty() {
        if !combined.is_empty() {
            combined.push_str("\n");
        }
        combined.push_str(&err);
    }
    if combined.is_empty() {
        "Windows WLAN returned an error without details.".to_owned()
    } else {
        combined
    }
}

#[cfg(test)]
mod tests {
    use super::{build_profile, validate_password, validate_ssid, TempProfileGuard};

    #[test]
    fn xml_escapes_ssid_and_password() {
        let xml = build_profile("Home & Lab", "NOVA::Home & Lab", Some("pass&word123"));
        assert!(xml.contains("Home &amp; Lab"));
        assert!(xml.contains("pass&amp;word123"));
    }

    #[test]
    fn validates_wpa2_password_length() {
        assert!(validate_password("12345678").is_ok());
        assert!(validate_password("short").is_err());
        assert!(validate_password(&"a".repeat(64)).is_err());
        assert!(validate_password(&"a1".repeat(32)).is_ok());
    }

    #[test]
    fn temp_profile_guard_removes_file_on_drop() {
        let path = std::env::temp_dir().join(format!("nova-temp-guard-{}.xml", std::process::id()));
        std::fs::write(&path, "temporary").unwrap();
        let guard = TempProfileGuard(path.clone());
        assert!(path.exists());
        drop(guard);
        assert!(!path.exists());
    }

    #[test]
    fn validates_ssid_byte_length() {
        assert!(validate_ssid("Home WiFi").is_ok());
        assert!(validate_ssid("").is_err());
        assert!(validate_ssid(&"a".repeat(33)).is_err());
    }
}
