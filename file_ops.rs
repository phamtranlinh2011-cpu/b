//! Safe file operations for NOVA Files.
//!
//! Every operation is restricted to the portable user-data root. Links and
//! Windows reparse points are rejected rather than followed. Directory copies
//! are assembled in a private staging directory and published with one rename.

use crate::storage::{
    canonical_user_path, is_link_or_reparse_point, remove_tree_without_following_reparse_points,
    safe_user_path,
};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

static FILE_OP_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn MoveFileW(existing_name: *const u16, new_name: *const u16) -> i32;
}

#[cfg(target_os = "linux")]
#[link(name = "c")]
unsafe extern "C" {
    fn renameat2(old_dirfd: i32, old_path: *const std::ffi::c_char,
                 new_dirfd: i32, new_path: *const std::ffi::c_char, flags: u32) -> i32;
}

pub const PREVIEW_MAX_BYTES: u64 = 256 * 1024;
pub const SEARCH_MAX_RESULTS: usize = 2_000;
pub const SEARCH_MAX_VISITED: usize = 50_000;

/// Lock the containing directory chain while NOVA performs path-based I/O.
/// On Windows this keeps real directory handles open without DELETE sharing;
/// on other platforms the same chain is revalidated with lstat semantics.
#[cfg(windows)]
pub(crate) fn hold_directory_chain(directory: &Path) -> io::Result<crate::windows_process::DirectoryChainGuard> {
    crate::windows_process::hold_real_directory_chain(directory)
}

#[cfg(not(windows))]
pub(crate) fn hold_directory_chain(directory: &Path) -> io::Result<()> {
    let canonical = directory.canonicalize()?;
    let mut ancestors = canonical.ancestors().collect::<Vec<_>>();
    ancestors.reverse();
    for component in ancestors {
        let metadata = fs::symlink_metadata(component)?;
        if !metadata.is_dir() || is_link_or_reparse_point(&metadata) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("NOVA refuses a redirected or non-directory path component: {}", component.display()),
            ));
        }
    }
    if directory.canonicalize()? != canonical {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "Directory path changed during validation."));
    }
    Ok(())
}

pub(crate) fn open_regular_file_for_read(path: &Path) -> io::Result<File> {
    #[cfg(windows)]
    let file = {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_SHARE_READ: u32 = 0x0000_0001;
        const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
        let mut options = OpenOptions::new();
        options
            .read(true)
            .share_mode(FILE_SHARE_READ)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
        options.open(path)?
    };
    #[cfg(all(unix, target_os = "linux"))]
    let file = {
        use std::os::unix::fs::OpenOptionsExt;
        const O_NOFOLLOW: i32 = 0x0002_0000;
        let mut options = OpenOptions::new();
        options.read(true).custom_flags(O_NOFOLLOW);
        options.open(path)?
    };
    #[cfg(all(unix, any(target_os = "macos", target_os = "freebsd", target_os = "openbsd", target_os = "netbsd", target_os = "dragonfly")))]
    let file = {
        use std::os::unix::fs::OpenOptionsExt;
        const O_NOFOLLOW: i32 = 0x0000_0100;
        let mut options = OpenOptions::new();
        options.read(true).custom_flags(O_NOFOLLOW);
        options.open(path)?
    };
    #[cfg(all(unix, not(any(target_os = "linux", target_os = "macos", target_os = "freebsd", target_os = "openbsd", target_os = "netbsd", target_os = "dragonfly"))))]
    let file = File::open(path)?;
    #[cfg(all(not(windows), not(unix)))]
    let file = File::open(path)?;

    let metadata = file.metadata()?;
    if !metadata.is_file() || is_link_or_reparse_point(&metadata) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "NOVA refused to open a non-regular file or a reparse point.",
        ));
    }
    Ok(file)
}

/// Move without replacing an existing destination, even when it appears
/// between unique_destination() and the final publish/rename operation.
pub(crate) fn move_no_replace(source: &Path, destination: &Path) -> io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        let source_wide = source.as_os_str().encode_wide().chain(Some(0)).collect::<Vec<_>>();
        let destination_wide = destination.as_os_str().encode_wide().chain(Some(0)).collect::<Vec<_>>();
        let result = unsafe { MoveFileW(source_wide.as_ptr(), destination_wide.as_ptr()) };
        if result == 0 { Err(io::Error::last_os_error()) } else { Ok(()) }
    }
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::ffi::OsStrExt;
        use std::ffi::CString;
        const AT_FDCWD: i32 = -100;
        const RENAME_NOREPLACE: u32 = 1;
        let source = CString::new(source.as_os_str().as_bytes())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "Source path contains a null byte."))?;
        let destination = CString::new(destination.as_os_str().as_bytes())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "Destination path contains a null byte."))?;
        let result = unsafe { renameat2(AT_FDCWD, source.as_ptr(), AT_FDCWD, destination.as_ptr(), RENAME_NOREPLACE) };
        if result == 0 { Ok(()) } else { Err(io::Error::last_os_error()) }
    }
    #[cfg(all(not(windows), not(target_os = "linux")))]
    {
        // The supported product target is Windows. This fallback still refuses
        // an already-present destination, though only Windows/Linux provide
        // the atomic no-replace primitive used above.
        match fs::symlink_metadata(destination) {
            Ok(_) => return Err(io::Error::new(io::ErrorKind::AlreadyExists, "Destination already exists.")),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        fs::rename(source, destination)
    }
}

fn validate_existing_item(root: &Path, candidate: &Path) -> io::Result<(PathBuf, fs::Metadata)> {
    let metadata = fs::symlink_metadata(candidate)?;
    if is_link_or_reparse_point(&metadata) {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "Links and Windows reparse points cannot be copied, moved or previewed."));
    }
    if !metadata.is_file() && !metadata.is_dir() {
        return Err(io::Error::new(io::ErrorKind::Unsupported, "Only regular files and directories are supported."));
    }
    let canonical = canonical_user_path(root, candidate)?;
    let canonical_meta = fs::symlink_metadata(&canonical)?;
    if is_link_or_reparse_point(&canonical_meta) || (!canonical_meta.is_file() && !canonical_meta.is_dir()) {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "The item changed type or became redirected during the operation."));
    }
    Ok((canonical, canonical_meta))
}

fn validate_destination_dir(root: &Path, candidate: &Path) -> io::Result<PathBuf> {
    let metadata = fs::symlink_metadata(candidate)?;
    if !metadata.is_dir() || is_link_or_reparse_point(&metadata) {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "The destination must be a real directory inside NOVA Files."));
    }
    canonical_user_path(root, candidate)
}

fn split_name(name: &str) -> (&str, Option<&str>) {
    let path = Path::new(name);
    match (path.file_stem().and_then(|v| v.to_str()), path.extension().and_then(|v| v.to_str())) {
        (Some(stem), Some(ext)) if !stem.is_empty() => (stem, Some(ext)),
        (Some(stem), _) => (stem, None),
        _ => (name, None),
    }
}

/// Find a collision-free sibling name without ever replacing an existing item.
pub fn unique_destination(directory: &Path, name: &str, copy_style: bool) -> io::Result<PathBuf> {
    if name.is_empty() || name == "." || name == ".." || name.contains('/') || name.contains('\\') {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "A valid single file or folder name is required."));
    }
    let original = directory.join(name);
    match fs::symlink_metadata(&original) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(original),
        Ok(_) => {}
        Err(error) => return Err(error),
    }
    let (stem, extension) = split_name(name);
    for number in 1..=9_999u32 {
        let label = if copy_style {
            if number == 1 { "copy".to_owned() } else { format!("copy {number}") }
        } else {
            number.to_string()
        };
        let candidate_name = match extension {
            Some(ext) => format!("{stem} ({label}).{ext}"),
            None => format!("{stem} ({label})"),
        };
        let candidate = directory.join(candidate_name);
        match fs::symlink_metadata(&candidate) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(candidate),
            Ok(_) => {}
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(io::ErrorKind::AlreadyExists, "Could not find a free destination name."))
}

fn create_staging_directory(directory: &Path) -> io::Result<PathBuf> {
    for _ in 0..128 {
        let sequence = FILE_OP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let candidate = directory.join(format!(".nova-copy-{}-{sequence}.tmp", std::process::id()));
        match fs::create_dir(&candidate) {
            Ok(()) => return Ok(candidate),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "Could not reserve a unique temporary copy directory.",
    ))
}

fn copy_file_contents(source: &Path, destination: &Path, cancel: &AtomicBool, bytes_copied: &AtomicU64) -> io::Result<()> {
    let mut input = open_regular_file_for_read(source)?;
    let mut output = OpenOptions::new().write(true).create_new(true).open(destination)?;
    let copy_result = (|| {
        let mut buffer = [0u8; 256 * 1024];
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(io::Error::new(io::ErrorKind::Interrupted, "File copy was cancelled."));
            }
            let count = input.read(&mut buffer)?;
            if count == 0 { break; }
            output.write_all(&buffer[..count])?;
            bytes_copied.fetch_add(count as u64, Ordering::Relaxed);
        }
        output.sync_all()
    })();
    if let Err(error) = copy_result {
        drop(output);
        let _ = fs::remove_file(destination);
        return Err(error);
    }
    Ok(())
}

fn copy_tree(
    root: &Path,
    source: &Path,
    destination: &Path,
    depth: usize,
    visited: &mut usize,
    cancel: &AtomicBool,
    progress: &AtomicUsize,
    bytes_copied: &AtomicU64,
) -> io::Result<()> {
    if cancel.load(Ordering::Relaxed) {
        return Err(io::Error::new(io::ErrorKind::Interrupted, "File copy was cancelled."));
    }
    if depth > 64 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "Directory nesting exceeds the 64-level safety limit."));
    }
    *visited = (*visited).saturating_add(1);
    progress.store(*visited, Ordering::Relaxed);
    if *visited > 50_000 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "Directory copy exceeds the 50,000-item safety limit."));
    }
    let (canonical, metadata) = validate_existing_item(root, source)?;
    let parent = canonical.parent().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "Source item has no parent."))?;
    let _parent_guard = hold_directory_chain(parent)?;
    if canonical_user_path(root, &canonical)? != canonical {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "Source path changed before copy."));
    }
    if metadata.is_file() {
        return copy_file_contents(&canonical, destination, cancel, bytes_copied);
    }
    let _directory_guard = hold_directory_chain(&canonical)?;
    fs::create_dir(destination)?;
    for entry in fs::read_dir(&canonical)? {
        if cancel.load(Ordering::Relaxed) {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "File copy was cancelled."));
        }
        let entry = entry?;
        let child_source = entry.path();
        let child_name = entry.file_name();
        let child_meta = fs::symlink_metadata(&child_source)?;
        if is_link_or_reparse_point(&child_meta) {
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, format!("Copy stopped because '{}' is a link or reparse point.", child_name.to_string_lossy())));
        }
        let child_destination = destination.join(child_name);
        copy_tree(root, &child_source, &child_destination, depth + 1, visited, cancel, progress, bytes_copied)?;
    }
    Ok(())
}

/// Copy a file or directory into an existing NOVA directory. Existing names
/// are preserved and the new copy receives a ` (copy)` suffix.
pub fn copy_item(root: &Path, source: &Path, destination_dir: &Path) -> io::Result<PathBuf> {
    let cancel = AtomicBool::new(false);
    let progress = AtomicUsize::new(0);
    let bytes_copied = AtomicU64::new(0);
    copy_item_cancellable(root, source, destination_dir, &cancel, &progress, &bytes_copied)
}

/// Cancellable form used by the File Manager worker. Partial data is kept in
/// a private staging directory and removed before returning Interrupted.
pub fn copy_item_cancellable(
    root: &Path,
    source: &Path,
    destination_dir: &Path,
    cancel: &AtomicBool,
    progress: &AtomicUsize,
    bytes_copied: &AtomicU64,
) -> io::Result<PathBuf> {
    copy_item_cancellable_with_hook(root, source, destination_dir, cancel, progress, bytes_copied, || {})
}

fn copy_item_cancellable_with_hook<F>(
    root: &Path,
    source: &Path,
    destination_dir: &Path,
    cancel: &AtomicBool,
    progress: &AtomicUsize,
    bytes_copied: &AtomicU64,
    after_staging_created: F,
) -> io::Result<PathBuf>
where
    F: FnOnce(),
{
    if cancel.load(Ordering::Relaxed) {
        return Err(io::Error::new(io::ErrorKind::Interrupted, "File copy was cancelled."));
    }
    progress.store(0, Ordering::Relaxed);
    bytes_copied.store(0, Ordering::Relaxed);
    let canonical_root = root.canonicalize()?;
    let (canonical_source, metadata) = validate_existing_item(root, source)?;
    if canonical_source == canonical_root {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "The NOVA workspace root cannot be copied as an item."));
    }
    let destination = validate_destination_dir(root, destination_dir)?;
    let _destination_guard = hold_directory_chain(&destination)?;
    if canonical_user_path(&canonical_root, &destination)? != destination {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "Destination path changed before copy."));
    }
    if metadata.is_dir() && destination.starts_with(&canonical_source) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "A folder cannot be copied into itself or one of its descendants."));
    }
    let name = canonical_source.file_name().and_then(|v| v.to_str())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "The item name is not valid Unicode."))?;
    if name.to_ascii_lowercase().starts_with(".nova-copy-") {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "The .nova-copy- namespace is reserved for temporary copy operations."));
    }
    let final_path = unique_destination(&destination, name, true)?;
    // Reserve an isolated staging directory atomically before writing anything.
    // This avoids deleting a pre-existing item if a generated temp name collides.
    let staging_directory = create_staging_directory(&destination)?;
    let staging_guard = match hold_directory_chain(&staging_directory) {
        Ok(guard) => guard,
        Err(error) => {
            let _ = remove_tree_without_following_reparse_points(&staging_directory);
            return Err(error);
        }
    };
    let staging_item = staging_directory.join("payload");
    after_staging_created();
    let mut visited = 0;
    let result = copy_tree(&canonical_root, &canonical_source, &staging_item, 0, &mut visited, cancel, progress, bytes_copied)
        .and_then(|_| {
            if cancel.load(Ordering::Relaxed) {
                Err(io::Error::new(io::ErrorKind::Interrupted, "File copy was cancelled."))
            } else {
                move_no_replace(&staging_item, &final_path)
            }
        });
    if let Err(error) = result {
        drop(staging_guard);
        let _ = remove_tree_without_following_reparse_points(&staging_directory);
        return Err(error);
    }
    drop(staging_guard);
    // Publishing already succeeded. A leftover empty private staging directory
    // is harmless and hidden from search; do not report the copy as failed if
    // another process briefly prevents this best-effort cleanup.
    let _ = fs::remove_dir(&staging_directory);
    Ok(final_path)
}

fn copy_external_tree(
    source: &Path,
    destination: &Path,
    depth: usize,
    visited: &mut usize,
    cancel: &AtomicBool,
    progress: &AtomicUsize,
    bytes_copied: &AtomicU64,
) -> io::Result<()> {
    if cancel.load(Ordering::Relaxed) {
        return Err(io::Error::new(io::ErrorKind::Interrupted, "Clipboard import was cancelled."));
    }
    if depth > 64 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "Directory nesting exceeds the 64-level safety limit."));
    }
    *visited = (*visited).saturating_add(1);
    progress.store(*visited, Ordering::Relaxed);
    if *visited > 50_000 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "Clipboard import exceeds the 50,000-item safety limit."));
    }

    let metadata = fs::symlink_metadata(source)?;
    if is_link_or_reparse_point(&metadata) {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, format!("Clipboard import refused a link or reparse point: {}", source.display())));
    }
    if metadata.is_file() {
        let parent = source.parent().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "External source has no parent directory."))?;
        let _parent_guard = hold_directory_chain(parent)?;
        return copy_file_contents(source, destination, cancel, bytes_copied);
    }
    if !metadata.is_dir() {
        return Err(io::Error::new(io::ErrorKind::Unsupported, "Clipboard import accepts regular files and directories only."));
    }
    let _directory_guard = hold_directory_chain(source)?;
    fs::create_dir(destination)?;
    for entry in fs::read_dir(source)? {
        if cancel.load(Ordering::Relaxed) {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "Clipboard import was cancelled."));
        }
        let entry = entry?;
        let child_source = entry.path();
        let child_metadata = fs::symlink_metadata(&child_source)?;
        if is_link_or_reparse_point(&child_metadata) {
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, format!("Clipboard import stopped because '{}' is a link or reparse point.", entry.file_name().to_string_lossy())));
        }
        copy_external_tree(&child_source, &destination.join(entry.file_name()), depth + 1, visited, cancel, progress, bytes_copied)?;
    }
    Ok(())
}

/// Safely import a regular external file/folder from Windows' CF_HDROP
/// clipboard format into NOVA. External items are always copied, never moved
/// or deleted; all nested reparse points are rejected and bounded staging is
/// published atomically into the NOVA workspace.
pub fn copy_external_item_cancellable(
    root: &Path,
    source: &Path,
    destination_dir: &Path,
    cancel: &AtomicBool,
    progress: &AtomicUsize,
    bytes_copied: &AtomicU64,
) -> io::Result<PathBuf> {
    if cancel.load(Ordering::Relaxed) {
        return Err(io::Error::new(io::ErrorKind::Interrupted, "Clipboard import was cancelled."));
    }
    progress.store(0, Ordering::Relaxed);
    bytes_copied.store(0, Ordering::Relaxed);
    let canonical_root = root.canonicalize()?;
    let source_metadata = fs::symlink_metadata(source)?;
    if is_link_or_reparse_point(&source_metadata) || (!source_metadata.is_file() && !source_metadata.is_dir()) {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "Clipboard source must be a regular file or directory, not a link or special object."));
    }
    let source_parent = source.parent().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "Clipboard source has no parent directory."))?;
    let _source_parent_guard = hold_directory_chain(source_parent)?;
    let canonical_source = source.canonicalize()?;
    let (canonical_source, metadata) = validate_external_item_under_guard(&canonical_source)?;
    let destination = validate_destination_dir(root, destination_dir)?;
    let _destination_guard = hold_directory_chain(&destination)?;
    if metadata.is_dir() && destination.starts_with(&canonical_source) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "A folder cannot be imported into itself or one of its descendants."));
    }
    let name = canonical_source.file_name().and_then(|value| value.to_str())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "Clipboard item name is not valid Unicode."))?;
    if name.to_ascii_lowercase().starts_with(".nova-copy-") {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "The .nova-copy- namespace is reserved for temporary copy operations."));
    }
    let final_path = unique_destination(&destination, name, true)?;
    let staging_directory = create_staging_directory(&destination)?;
    let staging_guard = match hold_directory_chain(&staging_directory) {
        Ok(guard) => guard,
        Err(error) => {
            let _ = remove_tree_without_following_reparse_points(&staging_directory);
            return Err(error);
        }
    };
    let staging_item = staging_directory.join("payload");
    let mut visited = 0usize;
    let result = copy_external_tree(&canonical_source, &staging_item, 0, &mut visited, cancel, progress, bytes_copied)
        .and_then(|_| {
            if cancel.load(Ordering::Relaxed) {
                Err(io::Error::new(io::ErrorKind::Interrupted, "Clipboard import was cancelled."))
            } else {
                move_no_replace(&staging_item, &final_path)
            }
        });
    if let Err(error) = result {
        drop(staging_guard);
        let _ = remove_tree_without_following_reparse_points(&staging_directory);
        return Err(error);
    }
    drop(staging_guard);
    let _ = fs::remove_dir(&staging_directory);
    Ok(final_path)
}

fn validate_external_item_under_guard(source: &Path) -> io::Result<(PathBuf, fs::Metadata)> {
    let metadata = fs::symlink_metadata(source)?;
    if is_link_or_reparse_point(&metadata) || (!metadata.is_file() && !metadata.is_dir()) {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "Clipboard source changed type or became redirected during validation."));
    }
    let canonical = source.canonicalize()?;
    if canonical != source {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "Clipboard source path changed during validation."));
    }
    Ok((canonical, metadata))
}

/// Move an item within the NOVA workspace without overwriting an existing name.
pub fn move_item(root: &Path, source: &Path, destination_dir: &Path) -> io::Result<PathBuf> {
    let canonical_root = root.canonicalize()?;
    let (canonical_source, _) = validate_existing_item(root, source)?;
    if canonical_source == canonical_root {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "The NOVA workspace root cannot be moved."));
    }
    let destination = validate_destination_dir(root, destination_dir)?;
    let source_parent = canonical_source.parent().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "Source item has no parent."))?;
    let _source_parent_guard = hold_directory_chain(source_parent)?;
    let _destination_guard = hold_directory_chain(&destination)?;
    if canonical_user_path(&canonical_root, &canonical_source)? != canonical_source
        || canonical_user_path(&canonical_root, &destination)? != destination
    {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "Source or destination path changed before move."));
    }
    if destination == canonical_source.parent().unwrap_or(Path::new("")) {
        return Ok(canonical_source);
    }
    if canonical_source.is_dir() && destination.starts_with(&canonical_source) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "A folder cannot be moved into itself or one of its descendants."));
    }
    let name = canonical_source.file_name().and_then(|v| v.to_str())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "The item name is not valid Unicode."))?;
    let target = unique_destination(&destination, name, false)?;
    move_no_replace(&canonical_source, &target)?;
    Ok(target)
}

/// Rename an item in its current directory. The new name must be one component
/// and an existing sibling is never overwritten.
pub fn rename_item(root: &Path, source: &Path, new_name: &str) -> io::Result<PathBuf> {
    if new_name.trim().is_empty()
        || new_name != new_name.trim()
        || new_name.contains('/')
        || new_name.contains('\\')
        || new_name.to_ascii_lowercase().starts_with(".nova-copy-")
    {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "Enter a valid name. The reserved .nova-copy- prefix cannot be used."));
    }
    let canonical_root = root.canonicalize()?;
    let (canonical_source, _) = validate_existing_item(root, source)?;
    if canonical_source == canonical_root {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "The NOVA workspace root cannot be renamed."));
    }
    let parent = canonical_source.parent().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "The selected item has no parent folder."))?;
    let _parent_guard = hold_directory_chain(parent)?;
    if canonical_user_path(&canonical_root, &canonical_source)? != canonical_source {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "Source path changed before rename."));
    }
    let relative_parent = parent.strip_prefix(&canonical_root)
        .map_err(|_| io::Error::new(io::ErrorKind::PermissionDenied, "The selected item's parent is outside NOVA Files."))?;
    let relative = relative_parent.join(new_name);
    let target = safe_user_path(&canonical_root, &relative.to_string_lossy())?;
    if target == canonical_source { return Ok(canonical_source); }
    match fs::symlink_metadata(&target) {
        Ok(_) => return Err(io::Error::new(io::ErrorKind::AlreadyExists, "An item with that name already exists.")),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    move_no_replace(&canonical_source, &target)?;
    Ok(target)
}

/// Read a small UTF-8/UTF-16 text preview, rejecting oversized and binary files.
pub fn read_text_preview(root: &Path, path: &Path) -> io::Result<String> {
    let (canonical, metadata) = validate_existing_item(root, path)?;
    let parent = canonical.parent().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "File has no parent directory."))?;
    let _parent_guard = hold_directory_chain(parent)?;
    if canonical_user_path(root, &canonical)? != canonical {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "File path changed before preview."));
    }
    if !metadata.is_file() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "Select a file to preview."));
    }
    if metadata.len() > PREVIEW_MAX_BYTES {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "Preview is limited to 256 KiB. Open the file in Editor for supported text files."));
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    open_regular_file_for_read(&canonical)?.take(PREVIEW_MAX_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > PREVIEW_MAX_BYTES {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "The file grew beyond the 256 KiB preview limit while it was being read."));
    }
    crate::unicode::decode_text_file(&bytes)
}

#[derive(Debug)]
pub struct SearchResult {
    pub paths: Vec<PathBuf>,
    pub truncated: bool,
}

/// Bounded recursive search that skips hidden redirectors and never follows links.
pub fn search_user_files(root: &Path, start: &Path, query: &str) -> io::Result<SearchResult> {
    let cancel = AtomicBool::new(false);
    search_user_files_cancellable(root, start, query, &cancel)
}

/// Search with a cooperative cancellation token checked between every
/// directory entry, so closing/cancelling a large search does not strand a worker.
pub fn search_user_files_cancellable(
    root: &Path,
    start: &Path,
    query: &str,
    cancel: &AtomicBool,
) -> io::Result<SearchResult> {
    let progress = AtomicUsize::new(0);
    search_user_files_with_control(root, start, query, cancel, &progress)
}

pub fn search_user_files_with_control(
    root: &Path,
    start: &Path,
    query: &str,
    cancel: &AtomicBool,
    progress: &AtomicUsize,
) -> io::Result<SearchResult> {
    if cancel.load(Ordering::Relaxed) {
        return Err(io::Error::new(io::ErrorKind::Interrupted, "File search was cancelled."));
    }
    let canonical_root = root.canonicalize()?;
    let canonical_start = canonical_user_path(root, start)?;
    if !canonical_start.is_dir() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "Search must start in a folder."));
    }
    let needle = query.trim().to_lowercase();
    if needle.is_empty() {
        return Ok(SearchResult { paths: Vec::new(), truncated: false });
    }
    let mut pending = vec![canonical_start];
    let mut paths = Vec::new();
    let mut visited = 0usize;
    let mut truncated = false;
    while let Some(directory) = pending.pop() {
        if cancel.load(Ordering::Relaxed) {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "File search was cancelled."));
        }
        let _directory_guard = hold_directory_chain(&directory)?;
        if canonical_user_path(&canonical_root, &directory)? != directory {
            continue;
        }
        let entries = fs::read_dir(&directory)?;
        for entry in entries {
            if cancel.load(Ordering::Relaxed) {
                return Err(io::Error::new(io::ErrorKind::Interrupted, "File search was cancelled."));
            }
            let entry = entry?;
            visited += 1;
            progress.store(visited, Ordering::Relaxed);
            if visited > SEARCH_MAX_VISITED {
                truncated = true;
                break;
            }
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_lowercase();
            // Never descend into transient staging trees from an active copy.
            if name.starts_with(".nova-copy-") { continue; }
            let metadata = match fs::symlink_metadata(&path) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) if error.kind() == io::ErrorKind::PermissionDenied => continue,
                Err(error) => return Err(error),
            };
            if is_link_or_reparse_point(&metadata) || (!metadata.is_dir() && !metadata.is_file()) {
                continue;
            }
            let canonical = match canonical_user_path(&canonical_root, &path) {
                Ok(path) => path,
                Err(_) => continue,
            };
            if metadata.is_dir() {
                pending.push(canonical.clone());
            }
            if name.contains(&needle) {
                paths.push(canonical);
                if paths.len() >= SEARCH_MAX_RESULTS {
                    truncated = true;
                    break;
                }
            }
        }
        if truncated { break; }
    }
    paths.sort_by(|a, b| a.to_string_lossy().to_lowercase().cmp(&b.to_string_lossy().to_lowercase()));
    Ok(SearchResult { paths, truncated })
}

#[cfg(test)]
mod tests {
    use super::{copy_item, move_item, move_no_replace, read_text_preview, rename_item, search_user_files, unique_destination, FILE_OP_SEQUENCE};
    use std::fs;
    use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TestDir(std::path::PathBuf);
    impl TestDir {
        fn new() -> Self {
            let id = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
            let path = std::env::temp_dir().join(format!("nova-file-ops-{}-{id}", std::process::id()));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for TestDir { fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); } }

    #[test]
    fn copy_file_creates_unique_copy_without_overwriting() {
        let temp = TestDir::new();
        let root = &temp.0;
        let source = root.join("note.txt");
        fs::write(&source, "hello").unwrap();
        let first = copy_item(root, &source, root).unwrap();
        let second = copy_item(root, &source, root).unwrap();
        assert_eq!(fs::read_to_string(first).unwrap(), "hello");
        assert_eq!(second.file_name().unwrap().to_string_lossy(), "note (copy 2).txt");
        assert_eq!(fs::read_to_string(root.join("note.txt")).unwrap(), "hello");
    }

    #[test]
    fn copy_directory_is_staged_and_preserves_nested_content() {
        let temp = TestDir::new();
        let root = &temp.0;
        let source = root.join("Folder");
        fs::create_dir_all(source.join("Nested")).unwrap();
        fs::write(source.join("Nested/note.txt"), "nested").unwrap();
        let result = copy_item(root, &source, root).unwrap();
        assert_eq!(fs::read_to_string(result.join("Nested/note.txt")).unwrap(), "nested");
        assert!(fs::read_dir(root).unwrap().all(|entry| !entry.unwrap().file_name().to_string_lossy().starts_with(".nova-copy-")));
    }

    #[test]
    fn copy_and_move_reject_a_descendant_destination() {
        let temp = TestDir::new();
        let root = &temp.0;
        let source = root.join("Folder");
        fs::create_dir(&source).unwrap();
        assert!(copy_item(root, &source, &source).is_err());
        assert!(move_item(root, &source, &source).is_err());
    }

    #[test]
    fn move_uses_a_unique_name_and_preserves_source_contents() {
        let temp = TestDir::new();
        let root = &temp.0;
        let source_dir = root.join("source");
        let dest_dir = root.join("dest");
        fs::create_dir_all(&source_dir).unwrap();
        fs::create_dir_all(&dest_dir).unwrap();
        let source = source_dir.join("note.txt");
        fs::write(&source, "move me").unwrap();
        fs::write(dest_dir.join("note.txt"), "keep me").unwrap();
        let moved = move_item(root, &source, &dest_dir).unwrap();
        assert_eq!(moved.file_name().unwrap().to_string_lossy(), "note (1).txt");
        assert_eq!(fs::read_to_string(dest_dir.join("note.txt")).unwrap(), "keep me");
        assert_eq!(fs::read_to_string(moved).unwrap(), "move me");
    }

    #[test]
    fn rename_rejects_invalid_names_and_overwrite() {
        let temp = TestDir::new();
        let root = &temp.0;
        let source = root.join("old.txt");
        fs::write(&source, "old").unwrap();
        fs::write(root.join("taken.txt"), "keep").unwrap();
        assert!(rename_item(root, &source, "taken.txt").is_err());
        assert!(rename_item(root, &source, "../bad.txt").is_err());
        let renamed = rename_item(root, &source, "new.txt").unwrap();
        assert_eq!(fs::read_to_string(renamed).unwrap(), "old");
    }

    #[test]
    fn preview_decodes_utf8_and_rejects_oversized_files() {
        let temp = TestDir::new();
        let root = &temp.0;
        let source = root.join("viet.txt");
        fs::write(&source, "Tiếng Việt").unwrap();
        assert_eq!(read_text_preview(root, &source).unwrap(), "Tiếng Việt");
        fs::write(root.join("large.txt"), vec![b'x'; 300 * 1024]).unwrap();
        assert!(read_text_preview(root, &root.join("large.txt")).is_err());
    }

    #[test]
    fn recursive_search_is_bounded_and_returns_nested_matches() {
        let temp = TestDir::new();
        let root = &temp.0;
        fs::create_dir_all(root.join("a/b")).unwrap();
        fs::write(root.join("a/b/target.txt"), "x").unwrap();
        fs::write(root.join("root-target.log"), "x").unwrap();
        let results = search_user_files(root, root, "target").unwrap();
        assert_eq!(results.paths.len(), 2);
        assert!(!results.truncated);
    }

    #[test]
    fn recursive_search_skips_copy_staging_directories() {
        let temp = TestDir::new();
        let root = &temp.0;
        fs::create_dir_all(root.join(".nova-copy-123-1.tmp/nested")).unwrap();
        fs::write(root.join(".nova-copy-123-1.tmp/nested/secret-target.txt"), "x").unwrap();
        fs::write(root.join("visible-target.txt"), "x").unwrap();
        let result = search_user_files(root, root, "target").unwrap();
        assert_eq!(result.paths.len(), 1);
        assert_eq!(result.paths[0].file_name().unwrap(), "visible-target.txt");
    }

    #[test]
    fn rename_rejects_reserved_copy_staging_names() {
        let temp = TestDir::new();
        let root = &temp.0;
        let source = root.join("ordinary.txt");
        fs::write(&source, "safe").unwrap();
        assert!(rename_item(root, &source, ".NOVA-COPY-backup").is_err());
        assert!(fs::exists(&source).unwrap());
    }

    #[test]
    fn atomic_publish_refuses_to_replace_a_destination_created_after_name_selection() {
        let temp = TestDir::new();
        let root = &temp.0;
        let staged = root.join("staged.txt");
        let target = unique_destination(root, "target.txt", false).unwrap();
        // Simulate another process creating the selected destination after
        // unique_destination() returned but before the staged item is moved.
        fs::write(&target, "keep").unwrap();
        fs::write(&staged, "new").unwrap();
        let error = move_no_replace(&staged, &target).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read_to_string(&staged).unwrap(), "new");
        assert_eq!(fs::read_to_string(&target).unwrap(), "keep");
    }

    #[test]
    fn unique_destination_preserves_multiple_extensions_and_unicode() {
        let temp = TestDir::new();
        let root = &temp.0;
        fs::write(root.join("ảnh.tar.gz"), "x").unwrap();
        let path = unique_destination(root, "ảnh.tar.gz", true).unwrap();
        assert_eq!(path.file_name().unwrap(), "ảnh.tar (copy).gz");
    }
    #[test]
    fn cancelled_search_stops_before_visiting_files() {
        let temp = TestDir::new();
        let root = &temp.0;
        fs::write(root.join("note.txt"), "hello").unwrap();
        let cancel = AtomicBool::new(true);
        let error = super::search_user_files_cancellable(root, root, "note", &cancel).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::Interrupted);
    }

    #[test]
    fn cancelled_copy_does_not_leave_a_staging_directory() {
        let temp = TestDir::new();
        let root = &temp.0;
        let source = root.join("note.txt");
        fs::write(&source, "hello").unwrap();
        let cancel = AtomicBool::new(false);
        let progress = AtomicUsize::new(0);
        let bytes_copied = std::sync::atomic::AtomicU64::new(0);
        let error = super::copy_item_cancellable_with_hook(
            root, &source, root, &cancel, &progress, &bytes_copied,
            || cancel.store(true, std::sync::atomic::Ordering::Relaxed),
        ).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::Interrupted);
        assert!(fs::read_dir(root).unwrap().all(|entry| !entry.unwrap().file_name().to_string_lossy().starts_with(".nova-copy-")));
        assert_eq!(fs::read_to_string(source).unwrap(), "hello");
    }


    #[cfg(unix)]
    #[test]
    fn external_clipboard_import_rejects_nested_symlinks_and_cleans_staging() {
        use std::os::unix::fs::symlink;
        let temp = TestDir::new();
        let workspace = temp.0.join("workspace");
        let source = temp.0.join("external");
        let outside = temp.0.join("outside");
        fs::create_dir_all(&workspace).unwrap();
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("keep.txt"), b"preserve me").unwrap();
        symlink(&outside, source.join("redirect")).unwrap();
        let cancel = AtomicBool::new(false);
        let progress = AtomicUsize::new(0);
        let bytes = AtomicU64::new(0);
        let error = super::copy_external_item_cancellable(&workspace, &source, &workspace, &cancel, &progress, &bytes).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
        assert_eq!(fs::read(outside.join("keep.txt")).unwrap(), b"preserve me");
        assert!(fs::read_dir(&workspace).unwrap().all(|entry| !entry.unwrap().file_name().to_string_lossy().starts_with(".nova-copy-")));
    }

    #[test]
    fn external_clipboard_copy_imports_without_modifying_the_source() {
        let base = std::env::temp_dir().join(format!("nova-external-copy-{}-{}", std::process::id(), FILE_OP_SEQUENCE.fetch_add(1, Ordering::Relaxed)));
        let root = base.join("NOVA");
        let source = base.join("Outside");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&source).unwrap();
        std::fs::write(source.join("readme.txt"), b"external data").unwrap();
        let cancel = AtomicBool::new(false);
        let progress = AtomicUsize::new(0);
        let bytes = AtomicU64::new(0);
        let copied = super::copy_external_item_cancellable(&root, &source.join("readme.txt"), &root, &cancel, &progress, &bytes).unwrap();
        assert_eq!(std::fs::read(&copied).unwrap(), b"external data");
        assert_eq!(std::fs::read(source.join("readme.txt")).unwrap(), b"external data");
        let _ = std::fs::remove_dir_all(base);
    }

}
