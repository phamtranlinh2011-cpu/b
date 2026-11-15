use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct AppPaths {
    pub user_dir: PathBuf,
    pub settings_file: PathBuf,
    pub wifi_manifest: PathBuf,
    pub portable: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Settings {
    pub theme: String,
    pub reduce_motion: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: "dark".to_owned(),
            reduce_motion: false,
        }
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct WifiManifest {
    profiles: BTreeSet<String>,
}

impl AppPaths {
    pub fn discover() -> io::Result<Self> {
        let exe = env::current_exe()?;
        let app_dir = exe
            .parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| io::Error::other("NOVA.exe has no parent directory"))?;
        let portable = app_dir.join("portable.flag").is_file();

        let data_dir = if portable {
            app_dir.join("Data")
        } else {
            let base = env::var_os("LOCALAPPDATA")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
                .ok_or_else(|| io::Error::new(
                    io::ErrorKind::NotFound,
                    "LOCALAPPDATA is unavailable; refusing to store NOVA data in a temporary folder.",
                ))?;
            base.join("NOVA OS").join("Data")
        };

        let user_dir = data_dir.join("Users").join("Default");
        let settings_file = data_dir.join("Settings").join("settings.json");
        let wifi_manifest = data_dir.join("Settings").join("wifi-profiles.json");

        let directories = vec![
            data_dir.clone(),
            user_dir.clone(),
            user_dir.join("Desktop"),
            user_dir.join("Documents"),
            user_dir.join("Downloads"),
            data_dir.join("Settings"),
            data_dir.join("Logs"),
        ];
        for directory in directories {
            fs::create_dir_all(directory)?;
        }

        Ok(Self {
            user_dir,
            settings_file,
            wifi_manifest,
            portable,
        })
    }

    pub fn load_settings(&self) -> io::Result<Settings> {
        let mut settings = match fs::read_to_string(&self.settings_file) {
            Ok(text) => serde_json::from_str::<Settings>(&text).map_err(|error| {
                io::Error::new(io::ErrorKind::InvalidData, format!("Settings file is invalid: {error}"))
            })?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => Settings::default(),
            Err(error) => return Err(error),
        };
        if settings.theme != "dark" && settings.theme != "light" {
            settings.theme = "dark".to_owned();
        }
        Ok(settings)
    }

    pub fn save_settings(&self, settings: &Settings) -> io::Result<()> {
        let json = serde_json::to_string_pretty(settings)
            .map_err(|error| io::Error::other(error.to_string()))?;
        fs::write(&self.settings_file, json)
    }

    pub fn is_nova_wifi_profile(&self, ssid: &str) -> io::Result<bool> {
        Ok(self.read_wifi_manifest()?.profiles.contains(ssid))
    }

    pub fn add_nova_wifi_profile(&self, ssid: &str) -> io::Result<()> {
        let mut manifest = self.read_wifi_manifest()?;
        manifest.profiles.insert(ssid.to_owned());
        self.write_wifi_manifest(&manifest)
    }

    pub fn remove_nova_wifi_profile(&self, ssid: &str) -> io::Result<()> {
        let mut manifest = self.read_wifi_manifest()?;
        manifest.profiles.remove(ssid);
        self.write_wifi_manifest(&manifest)
    }

    fn read_wifi_manifest(&self) -> io::Result<WifiManifest> {
        match fs::read_to_string(&self.wifi_manifest) {
            Ok(text) => serde_json::from_str::<WifiManifest>(&text).map_err(|error| {
                io::Error::new(io::ErrorKind::InvalidData, format!("Wi-Fi profile manifest is invalid: {error}"))
            }),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(WifiManifest::default()),
            Err(error) => Err(error),
        }
    }

    fn write_wifi_manifest(&self, manifest: &WifiManifest) -> io::Result<()> {
        let json = serde_json::to_string_pretty(manifest)
            .map_err(|error| io::Error::other(error.to_string()))?;
        fs::write(&self.wifi_manifest, json)
    }
}

/// Resolve a user-provided path inside NOVA's user area only.
/// Absolute paths and parent traversal are rejected.
pub fn safe_user_path(root: &Path, relative: &str) -> io::Result<PathBuf> {
    // Do not trim the whole path: doing so would silently turn an invalid
    // Windows filename such as "trailing " into the different valid name
    // "trailing" before component validation can reject it.
    if relative.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "A relative file or folder path is required.",
        ));
    }
    let raw = Path::new(relative);
    if raw.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Use a relative path inside NOVA Files.",
        ));
    }

    let mut clean = PathBuf::new();
    for component in raw.components() {
        match component {
            std::path::Component::Normal(part) => {
                validate_user_component(part)?;
                clean.push(part);
            }
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir | std::path::Component::RootDir | std::path::Component::Prefix(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "Path traversal is not allowed in NOVA Files.",
                ));
            }
        }
    }
    Ok(root.join(clean))
}

/// Resolve an existing filesystem path and reject links/reparse points that
/// resolve outside NOVA's user-data root. Both paths are canonicalized so
/// Windows junctions and symlinks are checked against their final target.
pub fn canonical_user_path(root: &Path, candidate: &Path) -> io::Result<PathBuf> {
    let canonical_root = root.canonicalize()?;
    let canonical_candidate = candidate.canonicalize()?;
    if !canonical_candidate.starts_with(&canonical_root) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Access outside NOVA user folder is blocked.",
        ));
    }
    Ok(canonical_candidate)
}

/// Reject Windows-invalid filenames and reserved DOS device names even when
/// running tests on a non-Windows host. This keeps paths portable and safe.
fn validate_user_component(component: &std::ffi::OsStr) -> io::Result<()> {
    let Some(name) = component.to_str() else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "File and folder names must be valid UTF-8.",
        ));
    };
    if name.is_empty()
        || name.encode_utf16().count() > 255
        || name.ends_with(' ')
        || name.ends_with('.')
        || name.chars().any(|ch| ch.is_control() || "<>:\"/\\|?*".contains(ch))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "The file or folder name contains characters that Windows does not allow.",
        ));
    }

    let device_stem = name
        .split('.')
        .next()
        .unwrap_or(name)
        .trim_end_matches(&[' ', '.'][..])
        .to_ascii_uppercase();
    let reserved = matches!(
        device_stem.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$" | "CLOCK$"
    ) || ["COM", "LPT"].iter().any(|&prefix| {
        device_stem.strip_prefix(prefix).is_some_and(|suffix| {
            matches!(suffix, "¹" | "²" | "³")
                || (suffix.len() == 1 && suffix.as_bytes()[0].is_ascii_digit() && suffix != "0")
        })
    });
    if reserved {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Windows reserved device names cannot be used as files or folders.",
        ));
    }
    Ok(())
}

/// Create missing parent directories one component at a time, canonicalizing
/// after each operation so existing links/junctions cannot redirect creation
/// outside NOVA's user-data root.
pub fn create_user_directories(root: &Path, candidate: &Path) -> io::Result<PathBuf> {
    let canonical_root = root.canonicalize()?;
    let relative = candidate.strip_prefix(root).map_err(|_| {
        io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Directory path is outside NOVA user data.",
        )
    })?;
    let mut current = canonical_root.clone();

    for component in relative.components() {
        let std::path::Component::Normal(part) = component else {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Directory path contains an unsafe component.",
            ));
        };
        validate_user_component(part)?;
        let next = current.join(part);
        match fs::symlink_metadata(&next) {
            Ok(_) => {
                let canonical = canonical_user_path(&canonical_root, &next)?;
                if !canonical.is_dir() {
                    return Err(io::Error::new(
                        io::ErrorKind::NotADirectory,
                        "A path component is not a directory.",
                    ));
                }
                current = canonical;
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                match fs::create_dir(&next) {
                    Ok(()) => {}
                    Err(create_error) if create_error.kind() == io::ErrorKind::AlreadyExists => {}
                    Err(create_error) => return Err(create_error),
                }
                let canonical = canonical_user_path(&canonical_root, &next)?;
                if !canonical.is_dir() {
                    return Err(io::Error::new(
                        io::ErrorKind::NotADirectory,
                        "A created path component is not a directory.",
                    ));
                }
                current = canonical;
            }
            Err(error) => return Err(error),
        }
    }
    Ok(current)
}

#[cfg(test)]
mod tests {
    use super::{canonical_user_path, safe_user_path};
    use std::path::Path;

    #[test]
    fn safe_user_path_keeps_relative_paths_inside_root() {
        let resolved = safe_user_path(Path::new("C:/NOVA/Data/Users/Default"), "Documents/note.txt").unwrap();
        assert!(resolved.ends_with("Documents/note.txt"));
    }

    #[test]
    fn canonical_user_path_accepts_descendants_and_rejects_external_paths() {
        let base = std::env::temp_dir().join(format!("nova-path-test-{}", std::process::id()));
        let root = base.join("root");
        let outside = base.join("outside");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        assert!(canonical_user_path(&root, &root).is_ok());
        assert!(canonical_user_path(&root, &outside).is_err());
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn safe_user_path_rejects_parent_traversal_and_absolute_paths() {
        let root = Path::new("C:/NOVA/Data/Users/Default");
        assert!(safe_user_path(root, "../../Windows/win.ini").is_err());
        assert!(safe_user_path(root, "C:/Windows/win.ini").is_err());
    }

    #[test]
    fn safe_user_path_rejects_windows_reserved_names_and_invalid_chars() {
        let root = Path::new("C:/NOVA/Data/Users/Default");
        for unsafe_name in ["CON", "con.txt", "LPT1.log", "COM¹", "LPT³.log", "bad:name.txt", "trailing.", "trailing "] {
            assert!(safe_user_path(root, unsafe_name).is_err(), "accepted {unsafe_name}");
        }
        assert!(safe_user_path(root, "Documents/normal-file.txt").is_ok());
        assert!(safe_user_path(root, "").is_err());
        assert!(safe_user_path(root, "   ").is_err());
        assert!(safe_user_path(root, &"a".repeat(256)).is_err());
    }

    #[test]
    fn create_user_directories_creates_nested_folders_below_root() {
        let base = std::env::temp_dir().join(format!("nova-create-dir-test-{}", std::process::id()));
        let root = base.join("root");
        let nested = root.join("Documents").join("Drafts");
        std::fs::create_dir_all(&root).unwrap();
        let created = super::create_user_directories(&root, &nested).unwrap();
        assert!(created.is_dir());
        assert!(created.starts_with(root.canonicalize().unwrap()));
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn corrupted_wifi_manifest_is_not_treated_as_empty() {
        let base = std::env::temp_dir().join(format!("nova-manifest-test-{}", std::process::id()));
        let settings = base.join("Settings");
        std::fs::create_dir_all(&settings).unwrap();
        let paths = super::AppPaths {
            user_dir: base.join("Users/Default"),
            settings_file: settings.join("settings.json"),
            wifi_manifest: settings.join("wifi-profiles.json"),
            portable: true,
        };
        assert!(!paths.is_nova_wifi_profile("Home WiFi").unwrap());
        std::fs::write(&paths.wifi_manifest, "{ invalid json").unwrap();
        assert!(paths.is_nova_wifi_profile("Home WiFi").is_err());
        let _ = std::fs::remove_dir_all(base);
    }
}
