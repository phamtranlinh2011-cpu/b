use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct AppPaths {
    pub app_dir: PathBuf,
    pub data_dir: PathBuf,
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
                .map(PathBuf::from)
                .unwrap_or_else(env::temp_dir);
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
            app_dir,
            data_dir,
            user_dir,
            settings_file,
            wifi_manifest,
            portable,
        })
    }

    pub fn load_settings(&self) -> Settings {
        fs::read_to_string(&self.settings_file)
            .ok()
            .and_then(|text| serde_json::from_str::<Settings>(&text).ok())
            .unwrap_or_default()
    }

    pub fn save_settings(&self, settings: &Settings) -> io::Result<()> {
        let json = serde_json::to_string_pretty(settings)
            .map_err(|error| io::Error::other(error.to_string()))?;
        fs::write(&self.settings_file, json)
    }

    pub fn is_nova_wifi_profile(&self, ssid: &str) -> bool {
        self.read_wifi_manifest().profiles.contains(ssid)
    }

    pub fn add_nova_wifi_profile(&self, ssid: &str) -> io::Result<()> {
        let mut manifest = self.read_wifi_manifest();
        manifest.profiles.insert(ssid.to_owned());
        self.write_wifi_manifest(&manifest)
    }

    pub fn remove_nova_wifi_profile(&self, ssid: &str) -> io::Result<()> {
        let mut manifest = self.read_wifi_manifest();
        manifest.profiles.remove(ssid);
        self.write_wifi_manifest(&manifest)
    }

    fn read_wifi_manifest(&self) -> WifiManifest {
        fs::read_to_string(&self.wifi_manifest)
            .ok()
            .and_then(|text| serde_json::from_str::<WifiManifest>(&text).ok())
            .unwrap_or_default()
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
    let raw = Path::new(relative.trim());
    if raw.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Use a relative path inside NOVA Files.",
        ));
    }

    let mut clean = PathBuf::new();
    for component in raw.components() {
        match component {
            std::path::Component::Normal(part) => clean.push(part),
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
}
