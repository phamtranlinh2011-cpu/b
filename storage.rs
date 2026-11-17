use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
#[cfg(windows)]
use std::ffi::c_void;
use std::env;
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Clone, Debug)]
pub struct AppPaths {
    pub app_dir: PathBuf,
    pub data_dir: PathBuf,
    pub security_dir: PathBuf,
    pub user_dir: PathBuf,
    pub settings_file: PathBuf,
    pub wifi_manifest: PathBuf,
    pub trash_dir: PathBuf,
    pub portable: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub theme: String,
    pub reduce_motion: bool,
    pub accent: String,
    pub pinned_apps: Vec<String>,
    /// Desktop shortcut positions normalized to the desktop work area (0.0..=1.0).
    #[serde(default)]
    pub desktop_icon_positions: BTreeMap<String, [f32; 2]>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: "dark".to_owned(),
            reduce_motion: false,
            accent: "blue".to_owned(),
            pinned_apps: vec![
                "Files".to_owned(), "Terminal".to_owned(), "Editor".to_owned(),
                "Calculator".to_owned(), "Monitor".to_owned(), "Google Chrome".to_owned(),
                "UniKey".to_owned(), "Security Center".to_owned(), "Settings".to_owned(),
                "Trash".to_owned(), "Network Discovery".to_owned(), "Packet Analyzer".to_owned(),
            ],
            desktop_icon_positions: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct WifiManifest {
    profiles: BTreeSet<String>,
    /// Exact Windows profile names for newly-created NOVA profiles. Missing
    /// values are legacy entries using the historical `NOVA::<SSID>` name.
    #[serde(default)]
    profile_names: BTreeMap<String, String>,
}

static ATOMIC_FILE_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[cfg(windows)]
type WinHandle = *mut c_void;

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateFileW(
        file_name: *const u16,
        desired_access: u32,
        share_mode: u32,
        security_attributes: *mut c_void,
        creation_disposition: u32,
        flags_and_attributes: u32,
        template_file: WinHandle,
    ) -> WinHandle;
    fn GetFileInformationByHandle(
        file: WinHandle,
        information: *mut WindowsByHandleFileInformation,
    ) -> i32;
    fn CompareStringOrdinal(
        string1: *const u16,
        count1: i32,
        string2: *const u16,
        count2: i32,
        ignore_case: i32,
    ) -> i32;
    fn CloseHandle(object: WinHandle) -> i32;
}

/// Verify every existing directory component rather than checking only the
/// leaf. `symlink_metadata` follows intermediate links, so checking just the
/// final parent would miss a replaced ancestor directory.
fn ensure_real_directory_chain(path: &Path) -> io::Result<()> {
    let mut ancestors = path.ancestors().collect::<Vec<_>>();
    ancestors.reverse();
    for directory in ancestors {
        if directory.as_os_str().is_empty() {
            continue;
        }
        let metadata = fs::symlink_metadata(directory)?;
        if !metadata.is_dir() || is_link_or_reparse_point(&metadata) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("NOVA refuses to use a redirected or non-directory path component: {}", directory.display()),
            ));
        }
    }
    Ok(())
}


const USER_CONFIG_MAX_BYTES: u64 = 1024 * 1024;

/// Read configuration through a no-follow file handle while the parent chain
/// is held stable. This closes the validation-to-open race for settings and
/// Wi-Fi profile metadata.
fn read_regular_file_bounded(path: &Path, limit: u64) -> io::Result<Option<Vec<u8>>> {
    let parent = path.parent().filter(|parent| !parent.as_os_str().is_empty())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "Configuration file has no parent directory."))?;
    #[cfg(windows)]
    let _parent_guard = crate::windows_process::hold_real_directory_chain(parent)?;
    #[cfg(not(windows))]
    ensure_real_directory_chain(parent)?;

    ensure_regular_file_not_reparse(path)?;
    let file = match crate::file_ops::open_regular_file_for_read(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let length = file.metadata()?.len();
    if length > limit {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "NOVA configuration file exceeds the supported size limit."));
    }
    let mut bytes = Vec::with_capacity(length.min(limit) as usize);
    file.take(limit.saturating_add(1)).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "NOVA configuration file grew beyond the supported size limit."));
    }
    Ok(Some(bytes))
}

/// Replace a user-data file without exposing partially-written contents if
/// NOVA or Windows is interrupted during the write. The temporary file is
/// created in the destination directory so the final rename stays on-volume.
fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path.parent().filter(|parent| !parent.as_os_str().is_empty())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "Target file has no parent directory."))?;
    // Keep the destination directory chain open without DELETE sharing on
    // Windows so a junction/ancestor cannot be swapped during the atomic write.
    #[cfg(windows)]
    let _parent_guard = crate::windows_process::hold_real_directory_chain(parent)?;
    #[cfg(not(windows))]
    ensure_real_directory_chain(parent)?;
    let file_name = path.file_name().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "Target file has no file name.")
    })?;
    ensure_regular_file_not_reparse(path)?;

    for _ in 0..8 {
        let sequence = ATOMIC_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let mut temp_name = std::ffi::OsString::from(".");
        temp_name.push(file_name);
        temp_name.push(format!(".{}.{}.tmp", std::process::id(), sequence));
        let temp_path = parent.join(temp_name);
        let mut file = match OpenOptions::new().write(true).create_new(true).open(&temp_path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        };

        let write_result = (|| {
            file.write_all(bytes)?;
            file.sync_all()?;
            drop(file);
            // Re-check both the target and every parent after the temp write.
            // This narrows the window in which a replaced directory or target
            // link could redirect a user-data write.
            ensure_regular_file_not_reparse(path)?;
            fs::rename(&temp_path, path)
        })();
        if let Err(error) = write_result {
            let _ = fs::remove_file(&temp_path);
            return Err(error);
        }
        return Ok(());
    }

    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "Could not allocate a unique temporary file.",
    ))
}

/// Atomically replace a regular user file after validating its parent chain
/// and refusing symlinks/reparse points. Shared by the built-in Editor and
/// settings/manifests so interrupted writes cannot expose partial contents.
pub(crate) fn atomic_write_user_file(path: &Path, bytes: &[u8]) -> io::Result<()> {
    atomic_write(path, bytes)
}

impl AppPaths {
    pub fn discover() -> io::Result<Self> {
        let exe = env::current_exe()?;
        let app_dir = exe
            .parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| io::Error::other("NOVA.exe has no parent directory"))?
            .canonicalize()?;
        let portable = app_dir.join("portable.flag").is_file();

        // Build the data tree one component at a time. create_dir_all follows
        // Windows junctions/reparse points, which could redirect a USB Data
        // folder into the host filesystem before later path checks run.
        let data_dir = if portable {
            ensure_real_child(&app_dir, "Data")?
        } else {
            let base = env::var_os("LOCALAPPDATA")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
                .ok_or_else(|| io::Error::new(
                    io::ErrorKind::NotFound,
                    "LOCALAPPDATA is unavailable; refusing to store NOVA data in a temporary folder.",
                ))?
                .canonicalize()?;
            let app_data = ensure_real_child(&base, "NOVA OS")?;
            ensure_real_child(&app_data, "Data")?
        };

        let users_dir = ensure_real_child(&data_dir, "Users")?;
        let user_dir = ensure_real_child(&users_dir, "Default")?;
        let _desktop = ensure_real_child(&user_dir, "Desktop")?;
        let _documents = ensure_real_child(&user_dir, "Documents")?;
        let _downloads = ensure_real_child(&user_dir, "Downloads")?;
        let settings_dir = ensure_real_child(&data_dir, "Settings")?;
        let _logs = ensure_real_child(&data_dir, "Logs")?;
        let trash_dir = ensure_real_child(&data_dir, "Trash")?;
        let _trash_items = ensure_real_child(&trash_dir, "Items")?;
        let _trash_meta = ensure_real_child(&trash_dir, "Meta")?;
        let security_dir = ensure_real_child(&data_dir, "Security")?;
        let _quarantine = ensure_real_child(&security_dir, "Quarantine")?;

        let settings_file = settings_dir.join("settings.json");
        let wifi_manifest = settings_dir.join("wifi-profiles.json");

        Ok(Self {
            app_dir,
            data_dir,
            security_dir,
            user_dir,
            settings_file,
            wifi_manifest,
            trash_dir,
            portable,
        })
    }

    pub fn load_settings(&self) -> io::Result<Settings> {
        let mut settings = match read_regular_file_bounded(&self.settings_file, USER_CONFIG_MAX_BYTES)? {
            Some(bytes) => serde_json::from_slice::<Settings>(&bytes).map_err(|error| {
                io::Error::new(io::ErrorKind::InvalidData, format!("Settings file is invalid: {error}"))
            })?,
            None => Settings::default(),
        };
        // `system` is a first-class persisted preference. Older versions
        // accidentally normalized it back to dark on every restart.
        if !matches!(settings.theme.as_str(), "dark" | "light" | "system") {
            settings.theme = "dark".to_owned();
        }
        if !matches!(settings.accent.as_str(), "blue" | "violet" | "teal" | "rose" | "amber") {
            settings.accent = "blue".to_owned();
        }
        Ok(settings)
    }

    pub fn save_settings(&self, settings: &Settings) -> io::Result<()> {
        let json = serde_json::to_vec_pretty(settings)
            .map_err(|error| io::Error::other(error.to_string()))?;
        atomic_write(&self.settings_file, &json)
    }

    pub fn is_nova_wifi_profile(&self, ssid: &str) -> io::Result<bool> {
        Ok(self.read_wifi_manifest()?.profiles.contains(ssid))
    }

    pub fn add_nova_wifi_profile_named(&self, ssid: &str, profile_name: &str) -> io::Result<()> {
        if ssid.is_empty() || profile_name.is_empty() {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "SSID and Windows profile name are required."));
        }
        let mut manifest = self.read_wifi_manifest()?;
        manifest.profiles.insert(ssid.to_owned());
        manifest.profile_names.insert(ssid.to_owned(), profile_name.to_owned());
        self.write_wifi_manifest(&manifest)
    }

    pub fn nova_wifi_profile_name(&self, ssid: &str) -> io::Result<Option<String>> {
        let manifest = self.read_wifi_manifest()?;
        if !manifest.profiles.contains(ssid) {
            return Ok(None);
        }
        Ok(manifest.profile_names.get(ssid).cloned())
    }

    pub fn remove_nova_wifi_profile(&self, ssid: &str) -> io::Result<()> {
        let mut manifest = self.read_wifi_manifest()?;
        manifest.profiles.remove(ssid);
        manifest.profile_names.remove(ssid);
        self.write_wifi_manifest(&manifest)
    }

    fn read_wifi_manifest(&self) -> io::Result<WifiManifest> {
        match read_regular_file_bounded(&self.wifi_manifest, USER_CONFIG_MAX_BYTES)? {
            Some(bytes) => serde_json::from_slice::<WifiManifest>(&bytes).map_err(|error| {
                io::Error::new(io::ErrorKind::InvalidData, format!("Wi-Fi profile manifest is invalid: {error}"))
            }),
            None => Ok(WifiManifest::default()),
        }
    }

    fn write_wifi_manifest(&self, manifest: &WifiManifest) -> io::Result<()> {
        let json = serde_json::to_vec_pretty(manifest)
            .map_err(|error| io::Error::other(error.to_string()))?;
        atomic_write(&self.wifi_manifest, &json)
    }
}


/// Refuse to read or replace a settings/manifest file that is redirected by a
/// symlink or Windows reparse point. A missing file is allowed for first-run.
fn ensure_regular_file_not_reparse(path: &Path) -> io::Result<()> {
    let parent = path.parent().filter(|parent| !parent.as_os_str().is_empty())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "Target file has no parent directory."))?;
    ensure_real_directory_chain(parent)?;
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() && !is_link_or_reparse_point(&metadata) => Ok(()),
        Ok(_) => Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("NOVA refuses to read or replace a non-regular or redirected file: {}", path.display()),
        )),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

/// Create or open a single direct child directory without following a symlink
/// or Windows reparse point. The canonical child must remain directly below
/// the canonical parent; this is used before any portable Data tree is touched.
pub(crate) fn ensure_real_child(parent: &Path, name: &str) -> io::Result<PathBuf> {
    if name.is_empty() || name == "." || name == ".." || (name.contains('/') || name.contains('\\')) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "Invalid NOVA data folder component."));
    }
    ensure_real_directory_chain(parent)?;
    let canonical_parent = parent.canonicalize()?;
    let candidate = canonical_parent.join(name);
    match fs::symlink_metadata(&candidate) {
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            match fs::create_dir(&candidate) {
                Ok(()) => {}
                Err(create_error) if create_error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(create_error) => return Err(create_error),
            }
        }
        Err(error) => return Err(error),
    }
    let metadata = fs::symlink_metadata(&candidate)?;
    if !metadata.is_dir() || is_link_or_reparse_point(&metadata) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("NOVA data folder '{}' must be a real directory; links and reparse points are blocked.", candidate.display()),
        ));
    }
    let canonical = candidate.canonicalize()?;
    if canonical.parent() != Some(canonical_parent.as_path()) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "NOVA data folder resolves outside its expected parent.",
        ));
    }
    Ok(canonical)
}

pub(crate) fn is_link_or_reparse_point(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
        return metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0;
    }
    #[cfg(not(windows))]
    {
        false
    }
}

/// Remove a link/reparse-point entry without following its target.
///
/// On Windows, `symlink_metadata().is_dir()` is not a reliable way to decide
/// whether a reparse point is directory-shaped: it can report false for a
/// junction or directory symlink. `RemoveFileW` then fails with
/// ERROR_ACCESS_DENIED (5). Use the DIRECTORY attribute from the link's own
/// metadata to select `RemoveDirectoryW` (`remove_dir`) instead.
fn remove_link_or_reparse_point(path: &Path, metadata: &fs::Metadata) -> io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x0010;
        if metadata.file_attributes() & FILE_ATTRIBUTE_DIRECTORY != 0 {
            return fs::remove_dir(path);
        }
        return fs::remove_file(path);
    }
    #[cfg(not(windows))]
    {
        if metadata.is_dir() {
            fs::remove_dir(path)
        } else {
            fs::remove_file(path)
        }
    }
}

/// Remove a tree without traversing symlinks or Windows reparse points.
/// Reparse-point directories are removed as links, never enumerated. The walk
/// is iterative so an adversarially deep directory tree cannot exhaust the
/// Rust call stack during permanent deletion or quarantine cleanup.
pub(crate) fn remove_tree_without_following_reparse_points(path: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if is_link_or_reparse_point(&metadata) {
        return remove_link_or_reparse_point(path, &metadata);
    }
    if metadata.is_file() {
        return fs::remove_file(path);
    }
    if !metadata.is_dir() {
        return Err(io::Error::new(io::ErrorKind::Unsupported, "Unsupported filesystem object; refusing recursive deletion."));
    }

    let mut pending = vec![path.to_path_buf()];
    let mut directories_postorder = Vec::new();
    while let Some(directory) = pending.pop() {
        // Lock this directory without following a final reparse point before
        // enumerating it. The handle prevents the directory itself from being
        // replaced while its children are classified and removed.
        #[cfg(windows)]
        let _directory_guard = crate::windows_process::hold_real_directory_chain(&directory)?;
        #[cfg(not(windows))]
        ensure_real_directory_chain(&directory)?;
        let directory_metadata = fs::symlink_metadata(&directory)?;
        if is_link_or_reparse_point(&directory_metadata) {
            remove_link_or_reparse_point(&directory, &directory_metadata)?;
            continue;
        }
        if !directory_metadata.is_dir() {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "Directory changed type during safe deletion."));
        }
        directories_postorder.push(directory.clone());
        for entry in fs::read_dir(&directory)? {
            let child = entry?.path();
            let child_metadata = fs::symlink_metadata(&child)?;
            if is_link_or_reparse_point(&child_metadata) {
                remove_link_or_reparse_point(&child, &child_metadata)?;
            } else if child_metadata.is_dir() {
                pending.push(child);
            } else if child_metadata.is_file() {
                fs::remove_file(child)?;
            } else {
                return Err(io::Error::new(io::ErrorKind::Unsupported, "Unsupported nested filesystem object; refusing recursive deletion."));
            }
        }
    }
    for directory in directories_postorder.into_iter().rev() {
        fs::remove_dir(directory)?;
    }
    Ok(())
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

/// Resolve an existing path only after checking every path component for
/// symlinks/junctions/reparse points. Containment is verified using directory
/// identity on Windows rather than comparing path strings: Windows can expose
/// the same directory through DOS, verbatim, case-varied, short-name or volume
/// spellings, and `Path::strip_prefix` is a lexical (case-sensitive) compare.
/// The original caller-visible chain is inspected before identity checks so
/// canonicalization cannot hide a redirected component.
#[cfg(windows)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct WindowsFileIdentity {
    volume_serial: u32,
    file_index: u64,
}

#[cfg(windows)]
#[repr(C)]
#[derive(Default)]
struct WindowsFileTime {
    _low: u32,
    _high: u32,
}

#[cfg(windows)]
#[repr(C)]
#[derive(Default)]
struct WindowsByHandleFileInformation {
    attributes: u32,
    _creation_time: WindowsFileTime,
    _last_access_time: WindowsFileTime,
    _last_write_time: WindowsFileTime,
    volume_serial: u32,
    _file_size_high: u32,
    _file_size_low: u32,
    _number_of_links: u32,
    file_index_high: u32,
    file_index_low: u32,
}

/// Query stable volume/file identity without following a final reparse point.
/// This is used only for containment verification; all caller-visible
/// ancestor components are separately checked above.
#[cfg(windows)]
fn windows_path_identity(path: &Path) -> io::Result<(WindowsFileIdentity, u32)> {
    use std::os::windows::ffi::OsStrExt;

    const INVALID_HANDLE_VALUE: WinHandle = -1isize as WinHandle;
    const FILE_READ_ATTRIBUTES: u32 = 0x0080;
    const FILE_SHARE_READ: u32 = 0x0001;
    const FILE_SHARE_WRITE: u32 = 0x0002;
    const FILE_SHARE_DELETE: u32 = 0x0004;
    const OPEN_EXISTING: u32 = 3;
    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;

    let wide_path = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    // SAFETY: `wide_path` is NUL-terminated and remains alive for the call.
    // The returned handle is checked and closed below.
    let handle = unsafe {
        CreateFileW(
            wide_path.as_ptr(),
            FILE_READ_ATTRIBUTES,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            std::ptr::null_mut(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            std::ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }

    let mut information = WindowsByHandleFileInformation::default();
    // SAFETY: `handle` is valid and `information` is writable for this call.
    let succeeded = unsafe { GetFileInformationByHandle(handle, &mut information) } != 0;
    let query_error = (!succeeded).then(io::Error::last_os_error);
    // SAFETY: this function owns the handle returned by CreateFileW.
    let _ = unsafe { CloseHandle(handle) };
    if let Some(error) = query_error {
        return Err(error);
    }
    if information.attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "NOVA refused a reparse point while verifying file identity.",
        ));
    }
    let file_index = (u64::from(information.file_index_high) << 32)
        | u64::from(information.file_index_low);
    if file_index == 0 || information.volume_serial == 0 {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "The filesystem did not provide a stable volume/file identity for NOVA path validation.",
        ));
    }
    Ok((
        WindowsFileIdentity {
            volume_serial: information.volume_serial,
            file_index,
        },
        information.attributes,
    ))
}

#[cfg(windows)]
fn windows_directory_identity(path: &Path) -> io::Result<WindowsFileIdentity> {
    const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x0010;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;

    let (identity, attributes) = windows_path_identity(path)?;
    if attributes & FILE_ATTRIBUTE_DIRECTORY == 0
        || attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "NOVA refused a redirected or non-directory path while verifying its location.",
        ));
    }
    Ok(identity)
}

/// Convert the path spellings returned by Windows APIs to a comparable DOS/UNC
/// form. `canonicalize()` may return a verbatim (`\\?\`) path for one
/// argument and a DOS path for another even when both refer to the same tree.
#[cfg(windows)]
fn windows_comparison_path(path: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;

    let mut wide = path.as_os_str().encode_wide().collect::<Vec<_>>();
    for unit in &mut wide {
        if *unit == b'/' as u16 {
            *unit = b'\\' as u16;
        }
    }

    // `\\?\UNC\server\share` -> `\\server\share`.
    const VERBATIM_UNC: [u16; 8] = [92, 92, 63, 92, 85, 78, 67, 92];
    if wide.starts_with(&VERBATIM_UNC) {
        let mut normalized = vec![92, 92];
        normalized.extend_from_slice(&wide[VERBATIM_UNC.len()..]);
        return normalized;
    }

    // `\\?\C:\path` -> `C:\path`; also accept the NT DOS-device spelling.
    const VERBATIM_DOS: [u16; 4] = [92, 92, 63, 92];
    const NT_DOS: [u16; 4] = [92, 63, 63, 92];
    if wide.starts_with(&VERBATIM_DOS) || wide.starts_with(&NT_DOS) {
        return wide[4..].to_vec();
    }
    wide
}

/// Return a component-boundary-checked relative path using Windows ordinal,
/// case-insensitive comparison after normalizing DOS/verbatim/UNC prefixes.
/// Reparse points are rejected by the caller before this helper is reached.
#[cfg(windows)]
fn windows_spelling_relative_path(root: &Path, candidate: &Path) -> Option<PathBuf> {
    use std::os::windows::ffi::OsStringExt;

    let root = windows_comparison_path(root);
    let candidate = windows_comparison_path(candidate);
    if root.len() > candidate.len() || root.len() > i32::MAX as usize {
        return None;
    }

    // CompareStringOrdinal(..., TRUE) applies Windows ordinal case folding
    // without changing the UTF-16 length used to extract the original suffix.
    let equal = unsafe {
        CompareStringOrdinal(
            candidate.as_ptr(),
            root.len() as i32,
            root.as_ptr(),
            root.len() as i32,
            1,
        )
    } == 2; // CSTR_EQUAL
    if !equal {
        return None;
    }
    if candidate.len() == root.len() {
        return Some(PathBuf::new());
    }

    let root_has_separator = root.last().is_some_and(|unit| *unit == b'\\' as u16);
    if !root_has_separator && candidate[root.len()] != b'\\' as u16 {
        return None;
    }
    let relative_start = if root_has_separator {
        root.len()
    } else {
        root.len() + 1
    };
    Some(PathBuf::from(std::ffi::OsString::from_wide(&candidate[relative_start..])))
}

/// Build a relative path from `root` to an already-canonical candidate.
/// On Windows, normalize DOS/verbatim spellings first and then use a file-ID
/// ancestor walk as a fallback for short-name or volume aliases.
#[cfg(windows)]
fn relative_path_from_canonical_root(
    root: &Path,
    candidate: &Path,
) -> io::Result<Option<PathBuf>> {
    if let Some(relative) = windows_spelling_relative_path(root, candidate) {
        return Ok(Some(relative));
    }
    // Keep the inexpensive common path when the canonical spellings already
    // match. The handle-identity walk is the fallback for Windows aliases.
    if let Ok(relative) = candidate.strip_prefix(root) {
        return Ok(Some(relative.to_path_buf()));
    }

    let root_identity = windows_directory_identity(root)?;
    let candidate_metadata = fs::metadata(candidate)?;
    let candidate_is_directory = candidate_metadata.is_dir();
    let mut directory = if candidate_is_directory {
        candidate.to_path_buf()
    } else {
        candidate
            .parent()
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "The selected file has no parent directory.",
                )
            })?
            .to_path_buf()
    };

    let mut directory_names = Vec::new();
    loop {
        if windows_directory_identity(&directory)? == root_identity {
            break;
        }
        let Some(name) = directory.file_name() else {
            return Ok(None);
        };
        directory_names.push(name.to_os_string());
        let Some(parent) = directory.parent() else {
            return Ok(None);
        };
        if parent == directory.as_path() {
            return Ok(None);
        }
        directory = parent.to_path_buf();
    }

    directory_names.reverse();
    let mut relative = PathBuf::new();
    for name in directory_names {
        relative.push(name);
    }
    if !candidate_is_directory {
        let name = candidate.file_name().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "The selected file has no name.",
            )
        })?;
        relative.push(name);
    }
    Ok(Some(relative))
}

#[cfg(not(windows))]
fn relative_path_from_canonical_root(
    root: &Path,
    candidate: &Path,
) -> io::Result<Option<PathBuf>> {
    Ok(candidate.strip_prefix(root).ok().map(Path::to_path_buf))
}

/// Validate and return both the resolved path and its root-relative spelling.
/// The relative spelling is reconstructed from verified components, not from
/// an untrusted lexical prefix comparison.
pub(crate) fn canonical_user_path_with_relative(
    root: &Path,
    candidate: &Path,
) -> io::Result<(PathBuf, PathBuf)> {
    ensure_real_directory_chain(root)?;

    if candidate
        .components()
        .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Path traversal is not allowed in NOVA Files.",
        ));
    }

    let root_metadata = fs::symlink_metadata(root)?;
    if !root_metadata.is_dir() || is_link_or_reparse_point(&root_metadata) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "NOVA user folder is not a real directory.",
        ));
    }

    let candidate_parent = candidate
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "The selected path has no parent directory.",
            )
        })?;
    ensure_real_directory_chain(candidate_parent)?;

    let candidate_metadata = fs::symlink_metadata(candidate)?;
    if is_link_or_reparse_point(&candidate_metadata) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("NOVA refuses redirected path components: {}", candidate.display()),
        ));
    }
    if !candidate_metadata.is_dir() && !candidate_metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Only regular files and directories are supported in NOVA Files.",
        ));
    }

    let canonical_root = root.canonicalize()?;
    let canonical_candidate = candidate.canonicalize()?;
    let relative = relative_path_from_canonical_root(&canonical_root, &canonical_candidate)?
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Access outside NOVA user folder is blocked.",
            )
        })?;

    // Re-check every resolved component from the verified root. The original
    // caller-visible chain remains mandatory because canonicalization follows
    // reparse points and would otherwise erase evidence of them.
    let mut current = canonical_root.clone();
    for component in relative.components() {
        match component {
            std::path::Component::Normal(part) => current.push(part),
            std::path::Component::CurDir => continue,
            std::path::Component::ParentDir
            | std::path::Component::RootDir
            | std::path::Component::Prefix(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "Path traversal is not allowed in NOVA Files.",
                ));
            }
        }
        let metadata = fs::symlink_metadata(&current)?;
        if is_link_or_reparse_point(&metadata) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("NOVA refuses redirected path components: {}", current.display()),
            ));
        }
        if !metadata.is_dir() && !metadata.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "Only regular files and directories are supported in NOVA Files.",
            ));
        }
    }

    let verified_candidate = current.canonicalize()?;
    #[cfg(windows)]
    let candidate_unchanged = windows_path_identity(&verified_candidate)?.0
        == windows_path_identity(&canonical_candidate)?.0;
    #[cfg(not(windows))]
    let candidate_unchanged = verified_candidate == canonical_candidate;
    if !candidate_unchanged {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "The selected path changed during validation.",
        ));
    }
    Ok((canonical_candidate, relative))
}

pub fn canonical_user_path(root: &Path, candidate: &Path) -> io::Result<PathBuf> {
    canonical_user_path_with_relative(root, candidate).map(|(canonical, _)| canonical)
}

/// Return whether an existing real directory is the same as or below another
/// existing real directory. Errors remain distinct from a normal `false` result
/// so callers cannot accidentally ignore a reparse-point or I/O failure.
pub(crate) fn canonical_directory_contains(root: &Path, candidate: &Path) -> io::Result<bool> {
    ensure_real_directory_chain(root)?;
    ensure_real_directory_chain(candidate)?;

    let root_metadata = fs::symlink_metadata(root)?;
    let candidate_metadata = fs::symlink_metadata(candidate)?;
    if !root_metadata.is_dir()
        || is_link_or_reparse_point(&root_metadata)
        || !candidate_metadata.is_dir()
        || is_link_or_reparse_point(&candidate_metadata)
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Directory containment requires real, non-redirected directories.",
        ));
    }

    let canonical_root = root.canonicalize()?;
    let canonical_candidate = candidate.canonicalize()?;
    Ok(relative_path_from_canonical_root(&canonical_root, &canonical_candidate)?.is_some())
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
    ensure_real_directory_chain(root)?;
    if candidate
        .components()
        .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Directory path contains an unsafe parent component.",
        ));
    }
    let canonical_root = root.canonicalize()?;

    // The target may not exist yet. Walk upward to the nearest existing real
    // directory, prove that directory belongs to NOVA by file identity, then
    // append the missing names. This avoids lexical prefix failures for DOS,
    // verbatim and case-varied Windows spellings without permitting escapes.
    let mut missing_components = Vec::new();
    let mut existing_directory = candidate.to_path_buf();
    loop {
        match fs::symlink_metadata(&existing_directory) {
            Ok(metadata) => {
                if !metadata.is_dir() || is_link_or_reparse_point(&metadata) {
                    return Err(io::Error::new(
                        io::ErrorKind::NotADirectory,
                        "An existing path component is not a real directory.",
                    ));
                }
                break;
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let name = existing_directory.file_name().ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        "Directory path is outside NOVA user data.",
                    )
                })?;
                missing_components.push(name.to_os_string());
                existing_directory = existing_directory
                    .parent()
                    .ok_or_else(|| {
                        io::Error::new(
                            io::ErrorKind::PermissionDenied,
                            "Directory path is outside NOVA user data.",
                        )
                    })?
                    .to_path_buf();
            }
            Err(error) => return Err(error),
        }
    }

    let (_, mut relative) = canonical_user_path_with_relative(root, &existing_directory)?;
    for name in missing_components.into_iter().rev() {
        validate_user_component(&name)?;
        relative.push(name);
    }

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
    use super::{canonical_user_path, canonical_user_path_with_relative, safe_user_path, Settings};
    use std::path::{Path, PathBuf};

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
    fn canonical_user_path_accepts_mixed_canonical_and_lexical_spellings() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let base = std::env::temp_dir().join(format!("nova-path-spelling-{}-{stamp}", std::process::id()));
        let root = base.join("root");
        let file = root.join("note.txt");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(&file, "safe").unwrap();

        let canonical_root = root.canonicalize().unwrap();
        let canonical_file = file.canonicalize().unwrap();
        assert_eq!(canonical_user_path(&canonical_root, &file).unwrap(), canonical_file);
        assert_eq!(canonical_user_path(&root, &canonical_file).unwrap(), canonical_file);
        let (resolved, relative) = canonical_user_path_with_relative(&root, &canonical_file).unwrap();
        assert_eq!(resolved, canonical_file);
        assert_eq!(relative, Path::new("note.txt"));

        let _ = std::fs::remove_dir_all(base);
    }

    #[cfg(windows)]
    #[test]
    fn windows_spelling_relative_path_accepts_dos_and_verbatim_prefixes_but_rejects_siblings() {
        let root = Path::new(r"C:\NOVA\Data\Users\Default");
        let dos_candidate = Path::new(r"C:\NOVA\Data\Users\Default\Documents\note.txt");
        let verbatim_root = Path::new(r"\\?\C:\NOVA\Data\Users\Default");
        let verbatim_candidate = Path::new(r"\\?\C:\NOVA\Data\Users\Default\Documents\note.txt");
        let sibling = Path::new(r"C:\NOVA\Data\Users\Default-Backup\note.txt");

        assert_eq!(super::windows_spelling_relative_path(root, verbatim_candidate), Some(PathBuf::from(r"Documents\note.txt")));
        assert_eq!(super::windows_spelling_relative_path(verbatim_root, dos_candidate), Some(PathBuf::from(r"Documents\note.txt")));
        assert_eq!(super::windows_spelling_relative_path(root, sibling), None);
    }

    #[cfg(windows)]
    #[test]
    fn canonical_user_path_accepts_case_varied_windows_spellings() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let base = std::env::temp_dir().join(format!("nova-path-case-{}-{stamp}", std::process::id()));
        let root = base.join("Root");
        let file = root.join("Documents").join("Note.txt");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "safe").unwrap();

        // Windows normally resolves these spellings to the same directory
        // objects even though a lexical prefix comparison is case-sensitive.
        let alternate_root = PathBuf::from(root.to_string_lossy().to_uppercase());
        let alternate_file = alternate_root.join("DOCUMENTS").join("NOTE.TXT");
        let identity_relative = super::relative_path_from_canonical_root(
            &root.canonicalize().unwrap(),
            &alternate_file,
        )
        .unwrap()
        .unwrap();
        let expected_relative = Path::new("Documents").join("Note.txt");
        assert!(identity_relative
            .to_string_lossy()
            .eq_ignore_ascii_case(&expected_relative.to_string_lossy()));
        let (resolved, relative) = canonical_user_path_with_relative(&root, &alternate_file).unwrap();
        assert_eq!(resolved, file.canonicalize().unwrap());
        assert!(relative
            .to_string_lossy()
            .eq_ignore_ascii_case(&expected_relative.to_string_lossy()));

        // Also exercise the Win32 extended-length path spelling. It refers to
        // the same file object but is not lexically prefixed by the DOS root.
        let to_extended_path = |path: &Path| {
            let text = path.to_string_lossy();
            if text.starts_with(r"\\?\") {
                PathBuf::from(text.as_ref())
            } else if let Some(unc_path) = text.strip_prefix(r"\\") {
                PathBuf::from(format!(r"\\?\UNC\{unc_path}"))
            } else {
                PathBuf::from(format!(r"\\?\{text}"))
            }
        };
        let extended_file = to_extended_path(&file);
        let extended_root = to_extended_path(&root);
        let (extended_resolved, extended_relative) =
            canonical_user_path_with_relative(&root, &extended_file).unwrap();
        assert_eq!(extended_resolved, file.canonicalize().unwrap());
        assert!(extended_relative
            .to_string_lossy()
            .eq_ignore_ascii_case(&expected_relative.to_string_lossy()));
        assert_eq!(
            super::windows_directory_identity(&root).unwrap(),
            super::windows_directory_identity(&extended_root).unwrap()
        );

        let _ = std::fs::remove_dir_all(base);
    }

    #[cfg(unix)]
    #[test]
    fn canonical_user_path_rejects_nested_symlink_even_when_target_is_inside_root() {
        use std::os::unix::fs::symlink;
        let base = std::env::temp_dir().join(format!("nova-inner-link-test-{}", std::process::id()));
        let root = base.join("root");
        let real = root.join("real");
        let link = root.join("linked");
        std::fs::create_dir_all(&real).unwrap();
        std::fs::write(real.join("note.txt"), "inside").unwrap();
        symlink(&real, &link).unwrap();
        assert!(canonical_user_path(&root, &link.join("note.txt")).is_err());
        let _ = std::fs::remove_file(&link);
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

    #[cfg(unix)]
    #[test]
    fn app_data_directory_creation_rejects_symlink_escape_before_writing() {
        use std::os::unix::fs::symlink;
        let base = std::env::temp_dir().join(format!("nova-data-tree-symlink-{}", std::process::id()));
        let app = base.join("usb");
        let outside = base.join("outside");
        std::fs::create_dir_all(&app).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        symlink(&outside, app.join("Data")).unwrap();
        assert!(super::ensure_real_child(&app, "Data").is_err());
        assert!(!outside.join("Users").exists());
        let _ = std::fs::remove_file(app.join("Data"));
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn ensure_real_child_creates_only_directories_below_parent() {
        let base = std::env::temp_dir().join(format!("nova-safe-tree-{}", std::process::id()));
        std::fs::create_dir_all(&base).unwrap();
        let child = super::ensure_real_child(&base, "Data").unwrap();
        assert_eq!(child.parent(), Some(base.canonicalize().unwrap().as_path()));
        let _ = std::fs::remove_dir_all(base);
    }

    #[cfg(windows)]
    #[test]
    fn windows_junctions_are_rejected_and_safe_delete_never_follows_them() {
        use std::os::windows::fs::MetadataExt;
        use std::process::Command;
        use std::time::{SystemTime, UNIX_EPOCH};

        let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let base = std::env::temp_dir().join(format!("nova-junction-test-{}-{nonce}", std::process::id()));
        let root = base.join("root");
        let outside = base.join("outside");
        let nested = root.join("nested");
        let junction = root.join("redirected-data");
        let nested_junction = nested.join("redirected-data");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("keep.txt"), "must survive").unwrap();

        let cmd = crate::windows_process::system_binary("cmd.exe").unwrap();
        let create_junction = |link: &std::path::Path| {
            let output = Command::new(&cmd)
                .args(["/D", "/C", "mklink", "/J"])
                .arg(link)
                .arg(&outside)
                .output()
                .expect("cmd.exe must be available on Windows");
            assert!(
                output.status.success(),
                "Could not create junction for Windows integration test: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        };
        create_junction(&junction);
        create_junction(&nested_junction);

        // Confirm the fixture is a directory reparse point. On Windows,
        // Metadata::is_dir() may not report true for this link; the directory
        // attribute is the authoritative discriminator for RemoveDirectoryW.
        let junction_metadata = std::fs::symlink_metadata(&junction).unwrap();
        let attrs = junction_metadata.file_attributes();
        const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x0010;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
        assert_ne!(attrs & FILE_ATTRIBUTE_DIRECTORY, 0, "Junction fixture must carry FILE_ATTRIBUTE_DIRECTORY");
        assert_ne!(attrs & FILE_ATTRIBUTE_REPARSE_POINT, 0, "Junction fixture must carry FILE_ATTRIBUTE_REPARSE_POINT");

        assert!(super::ensure_real_child(&root, "redirected-data").is_err());
        super::remove_tree_without_following_reparse_points(&junction).unwrap();
        assert!(!junction.exists());

        // Also exercise the recursive walk's child-reparse-point path.
        super::remove_tree_without_following_reparse_points(&nested).unwrap();
        assert!(!nested.exists());
        assert_eq!(std::fs::read_to_string(outside.join("keep.txt")).unwrap(), "must survive");

        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn wifi_manifest_round_trips_exact_profile_names_and_reads_legacy_entries() {
        let base = std::env::temp_dir().join(format!("nova-wifi-manifest-{}-{}", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let settings = base.join("Settings");
        std::fs::create_dir_all(&settings).unwrap();
        let paths = super::AppPaths {
            app_dir: base.clone(),
            data_dir: base.clone(),
            security_dir: base.join("Security"),
            user_dir: base.join("Users/Default"),
            settings_file: settings.join("settings.json"),
            wifi_manifest: settings.join("wifi-profiles.json"),
            trash_dir: base.join("Trash"),
            portable: true,
        };

        paths.add_nova_wifi_profile_named("Home WiFi", "NOVA::Home WiFi::session-123").unwrap();
        assert!(paths.is_nova_wifi_profile("Home WiFi").unwrap());
        assert_eq!(paths.nova_wifi_profile_name("Home WiFi").unwrap().as_deref(), Some("NOVA::Home WiFi::session-123"));
        paths.remove_nova_wifi_profile("Home WiFi").unwrap();
        assert!(!paths.is_nova_wifi_profile("Home WiFi").unwrap());

        // Older manifests have only `profiles`; the new optional map must not
        // break deletion of the historical `NOVA::<SSID>` profile name.
        std::fs::write(&paths.wifi_manifest, r#"{"profiles":["Legacy WiFi"]}"#).unwrap();
        assert!(paths.is_nova_wifi_profile("Legacy WiFi").unwrap());
        assert_eq!(paths.nova_wifi_profile_name("Legacy WiFi").unwrap(), None);
        let _ = std::fs::remove_dir_all(base);
    }

    #[cfg(unix)]
    #[test]
    fn settings_and_manifest_symlinks_are_rejected() {
        use std::os::unix::fs::symlink;
        let base = std::env::temp_dir().join(format!("nova-config-symlink-{}", std::process::id()));
        let settings = base.join("Settings");
        let external = base.join("outside.json");
        std::fs::create_dir_all(&settings).unwrap();
        std::fs::write(&external, r#"{"theme":"light"}"#).unwrap();
        let paths = super::AppPaths {
            app_dir: base.clone(),
            data_dir: base.clone(),
            security_dir: base.join("Security"),
            user_dir: base.join("Users/Default"),
            settings_file: settings.join("settings.json"),
            wifi_manifest: settings.join("wifi-profiles.json"),
            trash_dir: base.join("Trash"),
            portable: true,
        };
        symlink(&external, &paths.settings_file).unwrap();
        assert!(paths.load_settings().is_err());
        std::fs::remove_file(&paths.settings_file).unwrap();
        symlink(&external, &paths.wifi_manifest).unwrap();
        assert!(paths.is_nova_wifi_profile("Home").is_err());
        assert_eq!(std::fs::read_to_string(&external).unwrap(), r#"{"theme":"light"}"#);
        let _ = std::fs::remove_dir_all(base);
    }

    #[cfg(unix)]
    #[test]
    fn atomic_settings_write_rejects_a_symlinked_parent_directory() {
        use std::os::unix::fs::symlink;
        let base = std::env::temp_dir().join(format!("nova-settings-parent-link-{}", std::process::id()));
        let external = base.join("external");
        let redirected = base.join("Settings");
        std::fs::create_dir_all(&external).unwrap();
        symlink(&external, &redirected).unwrap();
        let paths = super::AppPaths {
            app_dir: base.clone(),
            data_dir: base.clone(),
            security_dir: base.join("Security"),
            user_dir: base.join("Users/Default"),
            settings_file: redirected.join("settings.json"),
            wifi_manifest: redirected.join("wifi-profiles.json"),
            trash_dir: base.join("Trash"),
            portable: true,
        };

        assert!(paths.save_settings(&super::Settings::default()).is_err());
        assert!(!external.join("settings.json").exists());
        let _ = std::fs::remove_file(&redirected);
        let _ = std::fs::remove_dir_all(base);
    }

    #[cfg(unix)]
    #[test]
    fn user_directory_creation_rejects_a_symlinked_ancestor() {
        use std::os::unix::fs::symlink;
        let base = std::env::temp_dir().join(format!("nova-user-ancestor-link-{}", std::process::id()));
        let external = base.join("external");
        let alias = base.join("Alias");
        std::fs::create_dir_all(external.join("Users/Default")).unwrap();
        symlink(&external, &alias).unwrap();
        let root = alias.join("Users/Default");

        assert!(super::create_user_directories(&root, &root.join("Documents")).is_err());
        assert!(!external.join("Users/Default/Documents").exists());
        let _ = std::fs::remove_file(&alias);
        let _ = std::fs::remove_dir_all(base);
    }

    #[cfg(unix)]
    #[test]
    fn create_user_directories_rejects_a_symlinked_root() {
        use std::os::unix::fs::symlink;
        let base = std::env::temp_dir().join(format!("nova-user-root-link-{}", std::process::id()));
        let external = base.join("external");
        let redirected = base.join("Users");
        std::fs::create_dir_all(&external).unwrap();
        symlink(&external, &redirected).unwrap();

        assert!(super::create_user_directories(&redirected, &redirected.join("Documents")).is_err());
        assert!(!external.join("Documents").exists());
        let _ = std::fs::remove_file(&redirected);
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn corrupted_wifi_manifest_is_not_treated_as_empty() {
        let base = std::env::temp_dir().join(format!("nova-manifest-test-{}", std::process::id()));
        let settings = base.join("Settings");
        std::fs::create_dir_all(&settings).unwrap();
        let paths = super::AppPaths {
            app_dir: base.clone(),
            data_dir: base.clone(),
            security_dir: base.join("Security"),
            user_dir: base.join("Users/Default"),
            settings_file: settings.join("settings.json"),
            wifi_manifest: settings.join("wifi-profiles.json"),
            trash_dir: base.join("Trash"),
            portable: true,
        };
        assert!(!paths.is_nova_wifi_profile("Home WiFi").unwrap());
        std::fs::write(&paths.wifi_manifest, "{ invalid json").unwrap();
        assert!(paths.is_nova_wifi_profile("Home WiFi").is_err());
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn legacy_settings_default_desktop_positions_to_empty() {
        let legacy = r#"{"theme":"dark","reduce_motion":false,"accent":"blue"}"#;
        let settings: Settings = serde_json::from_str(legacy).unwrap();
        assert!(settings.desktop_icon_positions.is_empty());
    }

    #[test]
    fn desktop_icon_positions_round_trip_in_settings() {
        let mut settings = Settings::default();
        settings.desktop_icon_positions.insert("Files".to_owned(), [0.25, 0.75]);
        let encoded = serde_json::to_vec(&settings).unwrap();
        let decoded: Settings = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(decoded.desktop_icon_positions.get("Files"), Some(&[0.25, 0.75]));
    }

    #[test]
    fn settings_are_atomically_replaced_and_old_settings_remain_compatible() {
        let base = std::env::temp_dir().join(format!("nova-settings-atomic-{}", std::process::id()));
        let settings_dir = base.join("Settings");
        std::fs::create_dir_all(&settings_dir).unwrap();
        let paths = super::AppPaths {
            app_dir: base.clone(),
            data_dir: base.clone(),
            security_dir: base.join("Security"),
            user_dir: base.join("Users/Default"),
            settings_file: settings_dir.join("settings.json"),
            wifi_manifest: settings_dir.join("wifi-profiles.json"),
            trash_dir: base.join("Trash"),
            portable: true,
        };
        // Version 0.3.5 settings did not contain an accent field.
        std::fs::write(&paths.settings_file, r#"{"theme":"light","reduce_motion":true}"#).unwrap();
        let loaded = paths.load_settings().unwrap();
        assert_eq!(loaded.theme, "light");
        assert!(loaded.reduce_motion);
        assert_eq!(loaded.accent, "blue");

        let next = super::Settings {
            theme: "dark".to_owned(),
            reduce_motion: false,
            accent: "violet".to_owned(),
            pinned_apps: vec!["Files".to_owned(), "Terminal".to_owned()],
            desktop_icon_positions: std::collections::BTreeMap::new(),
        };
        paths.save_settings(&next).unwrap();
        let saved = paths.load_settings().unwrap();
        assert_eq!(saved.accent, "violet");
        assert_eq!(saved.pinned_apps, vec!["Files".to_owned(), "Terminal".to_owned()]);
        assert!(!std::fs::read_dir(&settings_dir).unwrap().any(|entry| {
            entry.ok().is_some_and(|entry| entry.file_name().to_string_lossy().ends_with(".tmp"))
        }));
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn system_theme_preference_survives_settings_load() {
        let base = std::env::temp_dir().join(format!("nova-settings-system-theme-{}", std::process::id()));
        let settings_dir = base.join("Settings");
        std::fs::create_dir_all(&settings_dir).unwrap();
        let paths = super::AppPaths {
            app_dir: base.clone(), data_dir: base.clone(), security_dir: base.join("Security"),
            user_dir: base.join("Users/Default"), settings_file: settings_dir.join("settings.json"),
            wifi_manifest: settings_dir.join("wifi-profiles.json"), trash_dir: base.join("Trash"), portable: true,
        };
        std::fs::write(&paths.settings_file, r#"{"theme":"system","accent":"teal"}"#).unwrap();
        let loaded = paths.load_settings().unwrap();
        assert_eq!(loaded.theme, "system");
        assert_eq!(loaded.accent, "teal");
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn unknown_theme_preference_falls_back_to_dark() {
        let base = std::env::temp_dir().join(format!("nova-settings-invalid-theme-{}", std::process::id()));
        let settings_dir = base.join("Settings");
        std::fs::create_dir_all(&settings_dir).unwrap();
        let paths = super::AppPaths {
            app_dir: base.clone(), data_dir: base.clone(), security_dir: base.join("Security"),
            user_dir: base.join("Users/Default"), settings_file: settings_dir.join("settings.json"),
            wifi_manifest: settings_dir.join("wifi-profiles.json"), trash_dir: base.join("Trash"), portable: true,
        };
        std::fs::write(&paths.settings_file, r#"{"theme":"neon"}"#).unwrap();
        assert_eq!(paths.load_settings().unwrap().theme, "dark");
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn atomic_user_file_write_replaces_existing_content_without_temp_leaks() {
        let base = std::env::temp_dir().join(format!("nova-editor-atomic-{}", std::process::id()));
        std::fs::create_dir_all(&base).unwrap();
        let target = base.join("note.txt");
        std::fs::write(&target, "old content").unwrap();
        super::atomic_write_user_file(&target, "new content".as_bytes()).unwrap();
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "new content");
        assert!(!std::fs::read_dir(&base).unwrap().any(|entry| {
            entry.ok().is_some_and(|entry| entry.file_name().to_string_lossy().ends_with(".tmp"))
        }));
        let _ = std::fs::remove_dir_all(base);
    }


    #[cfg(unix)]
    #[test]
    fn atomic_user_file_write_refuses_symlink_targets() {
        use std::os::unix::fs::symlink;
        let base = std::env::temp_dir().join(format!("nova-editor-atomic-link-{}", std::process::id()));
        std::fs::create_dir_all(&base).unwrap();
        let original = base.join("original.txt");
        let link = base.join("linked.txt");
        std::fs::write(&original, "keep me").unwrap();
        symlink(&original, &link).unwrap();
        assert!(super::atomic_write_user_file(&link, b"replace me").is_err());
        assert_eq!(std::fs::read_to_string(&original).unwrap(), "keep me");
        let _ = std::fs::remove_dir_all(base);
    }

}
