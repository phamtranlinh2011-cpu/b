//! Portable NOVA Trash. Items are moved within the same Data volume so they
//! remain with the USB workspace and can be restored without relying on the
//! host Windows Recycle Bin.

use crate::storage::{canonical_user_path, create_user_directories, ensure_real_child, is_link_or_reparse_point, remove_tree_without_following_reparse_points, safe_user_path, AppPaths};
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TrashEntry {
    pub id: String,
    pub name: String,
    pub original_path: String,
    pub deleted_at: u64,
    pub is_directory: bool,
    pub size_bytes: Option<u64>,
}

fn items_dir(paths: &AppPaths) -> PathBuf {
    paths.trash_dir.join("Items")
}

fn metadata_dir(paths: &AppPaths) -> PathBuf {
    paths.trash_dir.join("Meta")
}

fn metadata_path(paths: &AppPaths, id: &str) -> io::Result<PathBuf> {
    if id.is_empty() || id.len() > 80 || !id.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-') {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "Invalid Trash item identifier."));
    }
    Ok(metadata_dir(paths).join(format!("{id}.json")))
}

fn read_metadata_file(path: &Path) -> io::Result<Vec<u8>> {
    let file = crate::file_ops::open_regular_file_for_read(path)?;
    if file.metadata()?.len() > 64 * 1024 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "Trash metadata exceeds the 64 KiB safety limit."));
    }
    let mut bytes = Vec::new();
    file.take(64 * 1024 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > 64 * 1024 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "Trash metadata grew beyond the 64 KiB safety limit."));
    }
    Ok(bytes)
}

fn item_path(paths: &AppPaths, id: &str) -> io::Result<PathBuf> {
    if id.is_empty() || id.len() > 80 || !id.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-') {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "Invalid Trash item identifier."));
    }
    Ok(items_dir(paths).join(id))
}

fn canonical_trash_item(paths: &AppPaths, id: &str) -> io::Result<PathBuf> {
    let candidate = item_path(paths, id)?;
    let metadata = fs::symlink_metadata(&candidate)?;
    if is_link_or_reparse_point(&metadata) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Links and reparse points are not valid NOVA Trash items.",
        ));
    }
    canonical_user_path(&items_dir(paths), &candidate)
}

fn unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn new_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let sequence = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    format!("{nanos:032x}-{:08x}-{sequence:016x}", std::process::id())
}

fn ensure_dirs(paths: &AppPaths) -> io::Result<()> {
    let trash_root = ensure_real_child(&paths.data_dir, "Trash")?;
    let items = ensure_real_child(&trash_root, "Items")?;
    let metadata = ensure_real_child(&trash_root, "Meta")?;
    if items != items_dir(paths).canonicalize()? || metadata != metadata_dir(paths).canonicalize()? {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "NOVA Trash folders were redirected; access is blocked."));
    }
    Ok(())
}

fn save_entry(paths: &AppPaths, entry: &TrashEntry) -> io::Result<()> {
    ensure_dirs(paths)?;
    let metadata_root = metadata_dir(paths).canonicalize()?;
    let _metadata_guard = crate::file_ops::hold_directory_chain(&metadata_root)?;
    let target = metadata_root.join(format!("{}.json", entry.id));
    let temp = metadata_root.join(format!(".{}.{}.tmp", entry.id, std::process::id()));
    let json = serde_json::to_vec_pretty(entry)
        .map_err(|error| io::Error::other(format!("Could not encode Trash metadata: {error}")))?;
    if json.len() > 64 * 1024 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Trash metadata would exceed the 64 KiB safety limit; the item was not moved.",
        ));
    }

    let metadata_parent = fs::symlink_metadata(&metadata_root)?;
    if !metadata_parent.is_dir() || is_link_or_reparse_point(&metadata_parent) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "NOVA refuses to write Trash metadata through a redirected folder.",
        ));
    }

    let mut owns_temp = false;
    let write_result = (|| {
        let mut file = OpenOptions::new().write(true).create_new(true).open(&temp)?;
        owns_temp = true;
        file.write_all(&json)?;
        file.sync_all()?;
        drop(file);
        crate::file_ops::move_no_replace(&temp, &target)
    })();
    if write_result.is_err() && owns_temp {
        let _ = fs::remove_file(&temp);
    }
    write_result
}

pub fn list_trash(paths: &AppPaths) -> io::Result<Vec<TrashEntry>> {
    ensure_dirs(paths)?;
    let metadata_root = metadata_dir(paths).canonicalize()?;
    let _metadata_guard = crate::file_ops::hold_directory_chain(&metadata_root)?;
    let mut entries = Vec::new();
    for item in fs::read_dir(&metadata_root)? {
        let item = item?;
        let metadata = fs::symlink_metadata(item.path())?;
        if !metadata.is_file() || is_link_or_reparse_point(&metadata)
            || item.path().extension().and_then(|ext| ext.to_str()) != Some("json")
        {
            continue;
        }
        if metadata.len() > 64 * 1024 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "Trash metadata exceeds the 64 KiB safety limit."));
        }
        let bytes = match read_metadata_file(&item.path()) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        let entry: TrashEntry = serde_json::from_slice(&bytes).map_err(|error| {
            io::Error::new(io::ErrorKind::InvalidData, format!("Trash metadata is invalid: {error}"))
        })?;
        let expected_name = format!("{}.json", entry.id);
        if item.file_name().to_string_lossy() != expected_name {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "Trash metadata identifier does not match its file name."));
        }
        match canonical_trash_item(paths, &entry.id) {
            Ok(_) => entries.push(entry),
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) if error.kind() == io::ErrorKind::PermissionDenied => continue,
            Err(error) => return Err(error),
        }
    }
    entries.sort_by(|a, b| b.deleted_at.cmp(&a.deleted_at).then_with(|| a.name.cmp(&b.name)));
    Ok(entries)
}

/// Move a NOVA Files item into the portable Trash. The item must be strictly
/// below Data/Users/Default; the workspace root itself can never be trashed.
pub fn move_to_trash(paths: &AppPaths, source: &Path) -> io::Result<TrashEntry> {
    ensure_dirs(paths)?;
    let canonical_root = paths.user_dir.canonicalize()?;
    let source_meta = fs::symlink_metadata(source)?;
    if is_link_or_reparse_point(&source_meta) {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "Links and reparse points cannot be moved to NOVA Trash."));
    }
    if !source_meta.is_file() && !source_meta.is_dir() {
        return Err(io::Error::new(io::ErrorKind::Unsupported, "Only regular files and directories can be moved to NOVA Trash."));
    }
    let canonical_source = canonical_user_path(&canonical_root, source)?;
    if canonical_source == canonical_root {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "The NOVA workspace root cannot be moved to Trash."));
    }
    let original_path = canonical_source
        .strip_prefix(&canonical_root)
        .map_err(|_| io::Error::new(io::ErrorKind::PermissionDenied, "Item is outside NOVA workspace."))?
        .to_str()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "NOVA Trash requires valid Unicode file paths."))?
        .to_owned();
    let name = canonical_source
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "Item name is not valid Unicode."))?
        .to_owned();

    let id = new_id();
    let _validated_destination = item_path(paths, &id)?;
    let source_parent = canonical_source.parent().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "Trash source has no parent directory."))?;
    let items_root = items_dir(paths).canonicalize()?;
    let destination = items_root.join(&id);
    let _source_guard = crate::file_ops::hold_directory_chain(source_parent)?;
    let _items_guard = crate::file_ops::hold_directory_chain(&items_root)?;
    if canonical_user_path(&canonical_root, &canonical_source)? != canonical_source {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "Trash source changed before move."));
    }
    match fs::symlink_metadata(&destination) {
        Ok(_) => return Err(io::Error::new(io::ErrorKind::AlreadyExists, "A Trash item already uses this identifier.")),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let entry = TrashEntry {
        id: id.clone(),
        name,
        original_path,
        deleted_at: unix_seconds(),
        is_directory: source_meta.is_dir(),
        size_bytes: source_meta.is_file().then_some(source_meta.len()),
    };

    // Both directories live below the same NOVA Data root, so rename stays on
    // the same volume and does not copy potentially large files.
    crate::file_ops::move_no_replace(&canonical_source, &destination)?;
    if let Err(error) = save_entry(paths, &entry) {
        return match crate::file_ops::move_no_replace(&destination, &canonical_source) {
            Ok(()) => Err(error),
            Err(rollback) => Err(io::Error::other(format!(
                "Could not save Trash metadata ({error}); rollback also failed ({rollback}). The item remains in NOVA Trash/Items/{id}.",
            ))),
        };
    }
    Ok(entry)
}

fn unique_restore_target(root: &Path, relative: &str) -> io::Result<PathBuf> {
    let target = safe_user_path(root, relative)?;
    match fs::symlink_metadata(&target) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(target),
        Ok(_) => {}
        Err(error) => return Err(error),
    }
    let parent = Path::new(relative).parent().unwrap_or_else(|| Path::new(""));
    let file_name = Path::new(relative)
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "Invalid original file name."))?;
    let path = Path::new(file_name);
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or(file_name);
    let extension = path.extension().and_then(|s| s.to_str());

    for number in 1..=9999 {
        let candidate_name = match extension {
            Some(ext) => format!("{stem} (restored {number}).{ext}"),
            None => format!("{stem} (restored {number})"),
        };
        let candidate_relative = if parent.as_os_str().is_empty() {
            candidate_name
        } else {
            parent.join(candidate_name).to_string_lossy().into_owned()
        };
        let candidate = safe_user_path(root, &candidate_relative)?;
        match fs::symlink_metadata(&candidate) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(candidate),
            Ok(_) => {}
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(io::ErrorKind::AlreadyExists, "Could not find a free name for the restored item."))
}

pub fn restore_from_trash(paths: &AppPaths, id: &str) -> io::Result<PathBuf> {
    ensure_dirs(paths)?;
    let _validated_metadata_path = metadata_path(paths, id)?;
    let metadata_root = metadata_dir(paths).canonicalize()?;
    let _metadata_guard = crate::file_ops::hold_directory_chain(&metadata_root)?;
    let metadata = metadata_root.join(format!("{id}.json"));
    let metadata_info = fs::symlink_metadata(&metadata)?;
    if !metadata_info.is_file() || is_link_or_reparse_point(&metadata_info) || metadata_info.len() > 64 * 1024 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "Trash metadata is not a valid regular file within the 64 KiB limit."));
    }
    let bytes = read_metadata_file(&metadata)?;
    let entry: TrashEntry = serde_json::from_slice(&bytes)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, format!("Trash metadata is invalid: {error}")))?;
    if entry.id != id {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "Trash item identifier does not match its metadata file."));
    }
    let source = canonical_trash_item(paths, id)?;
    let source_metadata = fs::symlink_metadata(&source)?;
    if source_metadata.is_dir() != entry.is_directory {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "Trash item type does not match its metadata."));
    }
    let target = unique_restore_target(&paths.user_dir, &entry.original_path)?;
    let parent = target.parent().unwrap_or(&paths.user_dir);
    let canonical_parent = create_user_directories(&paths.user_dir, parent)?;
    let file_name = target.file_name().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "Restore target has no file name."))?;
    let final_target = canonical_parent.join(file_name);
    let items_root = items_dir(paths).canonicalize()?;
    let _items_guard = crate::file_ops::hold_directory_chain(&items_root)?;
    let _destination_guard = crate::file_ops::hold_directory_chain(&canonical_parent)?;
    if canonical_trash_item(paths, id)? != source
        || canonical_user_path(&paths.user_dir, &canonical_parent)? != canonical_parent
    {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "Trash source or restore destination changed before restore."));
    }

    crate::file_ops::move_no_replace(&source, &final_target)?;
    if let Err(error) = fs::remove_file(&metadata) {
        return match crate::file_ops::move_no_replace(&final_target, &source) {
            Ok(()) => Err(error),
            Err(rollback) => Err(io::Error::other(format!(
                "Could not remove Trash metadata ({error}); rollback also failed ({rollback}). The restored item is at {}.",
                final_target.display(),
            ))),
        };
    }
    Ok(final_target)
}

fn remove_item(path: &Path) -> io::Result<()> {
    // A folder can contain a junction even when its top-level entry is normal.
    // Recurse manually so a permanent delete never follows nested reparse points.
    remove_tree_without_following_reparse_points(path)
}

pub fn delete_permanently(paths: &AppPaths, id: &str) -> io::Result<()> {
    ensure_dirs(paths)?;
    let _validated_metadata_path = metadata_path(paths, id)?;
    let metadata_root = metadata_dir(paths).canonicalize()?;
    let _metadata_guard = crate::file_ops::hold_directory_chain(&metadata_root)?;
    let metadata = metadata_root.join(format!("{id}.json"));
    let metadata_info = fs::symlink_metadata(&metadata)?;
    if !metadata_info.is_file() || is_link_or_reparse_point(&metadata_info) || metadata_info.len() > 64 * 1024 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "Trash metadata is not a valid regular metadata file."));
    }
    let entry: TrashEntry = serde_json::from_slice(&read_metadata_file(&metadata)?)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, format!("Trash metadata is invalid: {error}")))?;
    if entry.id != id {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "Trash item identifier does not match its metadata file."));
    }
    let source = canonical_trash_item(paths, id)?;
    let source_metadata = fs::symlink_metadata(&source)?;
    if source_metadata.is_dir() != entry.is_directory {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "Trash item type does not match its metadata."));
    }
    remove_item(&source)?;
    match fs::remove_file(metadata) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

pub fn empty_trash(paths: &AppPaths) -> io::Result<usize> {
    let entries = list_trash(paths)?;
    let mut removed = 0usize;
    let mut first_error = None;
    for entry in entries {
        match delete_permanently(paths, &entry.id) {
            Ok(()) => removed += 1,
            Err(error) if first_error.is_none() => first_error = Some(error),
            Err(_) => {}
        }
    }
    if let Some(error) = first_error {
        Err(io::Error::new(error.kind(), format!("Emptied {removed} item(s), but some items could not be removed: {error}")))
    } else {
        Ok(removed)
    }
}

#[cfg(test)]
mod tests {
    use super::{delete_permanently, empty_trash, list_trash, move_to_trash, restore_from_trash, save_entry, metadata_dir, TrashEntry};
    use crate::storage::AppPaths;
    use std::fs;

    fn paths(label: &str) -> (AppPaths, std::path::PathBuf) {
        let base = std::env::temp_dir().join(format!("nova-trash-{label}-{}", std::process::id()));
        let user_dir = base.join("Data/Users/Default");
        let trash_dir = base.join("Data/Trash");
        fs::create_dir_all(&user_dir).unwrap();
        let paths = AppPaths {
            app_dir: base.clone(),
            data_dir: base.join("Data"),
            security_dir: base.join("Data/Security"),
            user_dir: user_dir.clone(),
            settings_file: base.join("Data/Settings/settings.json"),
            wifi_manifest: base.join("Data/Settings/wifi.json"),
            trash_dir,
            portable: true,
        };
        (paths, base)
    }

    #[test]
    fn item_can_be_moved_to_trash_and_restored() {
        let (paths, base) = paths("restore");
        let original = paths.user_dir.join("note.txt");
        fs::write(&original, "portable note").unwrap();
        let entry = move_to_trash(&paths, &original).unwrap();
        assert!(!original.exists());
        assert_eq!(list_trash(&paths).unwrap().len(), 1);
        let restored = restore_from_trash(&paths, &entry.id).unwrap();
        assert_eq!(fs::read_to_string(restored).unwrap(), "portable note");
        assert!(list_trash(&paths).unwrap().is_empty());
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn restore_never_overwrites_existing_files() {
        let (paths, base) = paths("collision");
        let original = paths.user_dir.join("note.txt");
        fs::write(&original, "old").unwrap();
        let entry = move_to_trash(&paths, &original).unwrap();
        fs::write(&original, "new").unwrap();
        let restored = restore_from_trash(&paths, &entry.id).unwrap();
        assert_eq!(fs::read_to_string(&original).unwrap(), "new");
        assert_eq!(fs::read_to_string(restored).unwrap(), "old");
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn directory_can_be_moved_to_trash_and_restored_with_contents() {
        let (paths, base) = paths("directory-restore");
        let folder = paths.user_dir.join("Project");
        fs::create_dir_all(folder.join("src")).unwrap();
        fs::write(folder.join("src/main.rs"), "fn main() {}").unwrap();
        let entry = move_to_trash(&paths, &folder).unwrap();
        assert!(entry.is_directory);
        assert!(!folder.exists());
        let restored = restore_from_trash(&paths, &entry.id).unwrap();
        assert_eq!(fs::read_to_string(restored.join("src/main.rs")).unwrap(), "fn main() {}");
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn trash_rejects_workspace_root_and_permanent_delete_works() {
        let (paths, base) = paths("delete");
        assert!(move_to_trash(&paths, &paths.user_dir).is_err());
        let original = paths.user_dir.join("delete-me.txt");
        fs::write(&original, "x").unwrap();
        let entry = move_to_trash(&paths, &original).unwrap();
        delete_permanently(&paths, &entry.id).unwrap();
        assert!(list_trash(&paths).unwrap().is_empty());
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn restore_rejects_oversized_metadata_before_reading_it() {
        let (paths, base) = paths("oversized-metadata");
        let original = paths.user_dir.join("note.txt");
        fs::write(&original, "keep").unwrap();
        let entry = move_to_trash(&paths, &original).unwrap();
        let metadata = super::metadata_path(&paths, &entry.id).unwrap();
        fs::write(&metadata, vec![b' '; 64 * 1024 + 1]).unwrap();

        assert!(restore_from_trash(&paths, &entry.id).is_err());
        assert!(super::item_path(&paths, &entry.id).unwrap().exists());
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn restore_rejects_metadata_identifier_mismatch() {
        let (paths, base) = paths("metadata-id-mismatch");
        let original = paths.user_dir.join("note.txt");
        fs::write(&original, "safe").unwrap();
        let entry = move_to_trash(&paths, &original).unwrap();
        let metadata = super::metadata_path(&paths, &entry.id).unwrap();
        let mut corrupt = entry.clone();
        corrupt.id = "deadbeef".to_owned();
        fs::write(&metadata, serde_json::to_vec(&corrupt).unwrap()).unwrap();
        assert!(restore_from_trash(&paths, &entry.id).is_err());
        assert!(super::delete_permanently(&paths, &entry.id).is_err());
        assert!(super::item_path(&paths, &entry.id).unwrap().exists());
        let _ = fs::remove_dir_all(base);
    }

    #[cfg(unix)]
    #[test]
    fn trash_rejects_symlinked_items_directory_without_touching_external_data() {
        use std::os::unix::fs::symlink;
        let (paths, base) = paths("symlink-items");
        super::ensure_dirs(&paths).unwrap();
        let external = base.join("outside");
        fs::create_dir_all(&external).unwrap();
        fs::write(external.join("keep.txt"), "keep").unwrap();
        fs::remove_dir(paths.trash_dir.join("Items")).unwrap();
        symlink(&external, paths.trash_dir.join("Items")).unwrap();
        assert!(list_trash(&paths).is_err());
        assert_eq!(fs::read_to_string(external.join("keep.txt")).unwrap(), "keep");
        let _ = fs::remove_file(paths.trash_dir.join("Items"));
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn metadata_temp_collision_is_preserved_when_reservation_fails() {
        let (paths, base) = paths("metadata-temp-collision");
        super::ensure_dirs(&paths).unwrap();
        let entry = TrashEntry {
            id: "deadbeef".to_owned(),
            name: "note.txt".to_owned(),
            original_path: "note.txt".to_owned(),
            deleted_at: 1,
            is_directory: false,
            size_bytes: Some(4),
        };
        let temp = metadata_dir(&paths).join(format!(".{}.{}.tmp", entry.id, std::process::id()));
        fs::write(&temp, "pre-existing temp belongs to someone else").unwrap();
        assert!(save_entry(&paths, &entry).is_err());
        assert_eq!(fs::read_to_string(&temp).unwrap(), "pre-existing temp belongs to someone else");
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn empty_trash_removes_all_items() {
        let (paths, base) = paths("empty");
        for name in ["one.txt", "two.txt"] {
            let file = paths.user_dir.join(name);
            fs::write(file, "x").unwrap();
        }
        for name in ["one.txt", "two.txt"] {
            let file = paths.user_dir.join(name);
            move_to_trash(&paths, &file).unwrap();
        }
        assert_eq!(empty_trash(&paths).unwrap(), 2);
        assert!(list_trash(&paths).unwrap().is_empty());
        let _ = fs::remove_dir_all(base);
    }
    #[cfg(unix)]
    #[test]
    fn permanent_delete_does_not_follow_nested_symlinks() {
        use std::os::unix::fs::symlink;
        let (paths, base) = paths("nested-link-delete");
        let external = base.join("outside");
        fs::create_dir_all(&external).unwrap();
        fs::write(external.join("keep.txt"), "must survive").unwrap();
        let folder = paths.user_dir.join("folder-with-link");
        fs::create_dir_all(&folder).unwrap();
        symlink(&external, folder.join("external-link")).unwrap();

        let entry = move_to_trash(&paths, &folder).unwrap();
        delete_permanently(&paths, &entry.id).unwrap();
        assert_eq!(fs::read_to_string(external.join("keep.txt")).unwrap(), "must survive");
        let _ = fs::remove_dir_all(base);
    }

}
