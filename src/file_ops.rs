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
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static FILE_OP_SEQUENCE: AtomicU64 = AtomicU64::new(1);
pub const PREVIEW_MAX_BYTES: u64 = 256 * 1024;
pub const SEARCH_MAX_RESULTS: usize = 2_000;
pub const SEARCH_MAX_VISITED: usize = 50_000;

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

fn staging_path(directory: &Path) -> PathBuf {
    let sequence = FILE_OP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    directory.join(format!(".nova-copy-{}-{sequence}.tmp", std::process::id()))
}

fn copy_file_contents(source: &Path, destination: &Path) -> io::Result<()> {
    let mut input = File::open(source)?;
    let mut output = OpenOptions::new().write(true).create_new(true).open(destination)?;
    if let Err(error) = io::copy(&mut input, &mut output) {
        drop(output);
        let _ = fs::remove_file(destination);
        return Err(error);
    }
    if let Err(error) = output.sync_all() {
        drop(output);
        let _ = fs::remove_file(destination);
        return Err(error);
    }
    Ok(())
}

fn copy_tree(root: &Path, source: &Path, destination: &Path, depth: usize, visited: &mut usize) -> io::Result<()> {
    if depth > 64 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "Directory nesting exceeds the 64-level safety limit."));
    }
    *visited = (*visited).saturating_add(1);
    if *visited > 50_000 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "Directory copy exceeds the 50,000-item safety limit."));
    }
    let (canonical, metadata) = validate_existing_item(root, source)?;
    if metadata.is_file() {
        return copy_file_contents(&canonical, destination);
    }
    fs::create_dir(destination)?;
    for entry in fs::read_dir(&canonical)? {
        let entry = entry?;
        let child_source = entry.path();
        let child_name = entry.file_name();
        let child_meta = fs::symlink_metadata(&child_source)?;
        if is_link_or_reparse_point(&child_meta) {
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, format!("Copy stopped because '{}' is a link or reparse point.", child_name.to_string_lossy())));
        }
        let child_destination = destination.join(child_name);
        copy_tree(root, &child_source, &child_destination, depth + 1, visited)?;
    }
    Ok(())
}

/// Copy a file or directory into an existing NOVA directory. Existing names
/// are preserved and the new copy receives a ` (copy)` suffix.
pub fn copy_item(root: &Path, source: &Path, destination_dir: &Path) -> io::Result<PathBuf> {
    let canonical_root = root.canonicalize()?;
    let (canonical_source, metadata) = validate_existing_item(&canonical_root, source)?;
    if canonical_source == canonical_root {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "The NOVA workspace root cannot be copied as an item."));
    }
    let destination = validate_destination_dir(&canonical_root, destination_dir)?;
    if metadata.is_dir() && destination.starts_with(&canonical_source) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "A folder cannot be copied into itself or one of its descendants."));
    }
    let name = canonical_source.file_name().and_then(|v| v.to_str())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "The item name is not valid Unicode."))?;
    if name.to_ascii_lowercase().starts_with(".nova-copy-") {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "The .nova-copy- namespace is reserved for temporary copy operations."));
    }
    let final_path = unique_destination(&destination, name, true)?;
    let staging = staging_path(&destination);
    let mut visited = 0;
    let result = copy_tree(&canonical_root, &canonical_source, &staging, 0, &mut visited)
        .and_then(|_| fs::rename(&staging, &final_path));
    if let Err(error) = result {
        if fs::symlink_metadata(&staging).is_ok() {
            let _ = remove_tree_without_following_reparse_points(&staging);
        }
        return Err(error);
    }
    Ok(final_path)
}

/// Move an item within the NOVA workspace without overwriting an existing name.
pub fn move_item(root: &Path, source: &Path, destination_dir: &Path) -> io::Result<PathBuf> {
    let canonical_root = root.canonicalize()?;
    let (canonical_source, _) = validate_existing_item(&canonical_root, source)?;
    if canonical_source == canonical_root {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "The NOVA workspace root cannot be moved."));
    }
    let destination = validate_destination_dir(&canonical_root, destination_dir)?;
    if destination == canonical_source.parent().unwrap_or(Path::new("")) {
        return Ok(canonical_source);
    }
    if canonical_source.is_dir() && destination.starts_with(&canonical_source) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "A folder cannot be moved into itself or one of its descendants."));
    }
    let name = canonical_source.file_name().and_then(|v| v.to_str())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "The item name is not valid Unicode."))?;
    let target = unique_destination(&destination, name, false)?;
    fs::rename(&canonical_source, &target)?;
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
    let (canonical_source, _) = validate_existing_item(&canonical_root, source)?;
    if canonical_source == canonical_root {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "The NOVA workspace root cannot be renamed."));
    }
    let parent = canonical_source.parent().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "The selected item has no parent folder."))?;
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
    fs::rename(&canonical_source, &target)?;
    Ok(target)
}

/// Read a small UTF-8/UTF-16 text preview, rejecting oversized and binary files.
pub fn read_text_preview(root: &Path, path: &Path) -> io::Result<String> {
    let (canonical, metadata) = validate_existing_item(root, path)?;
    if !metadata.is_file() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "Select a file to preview."));
    }
    if metadata.len() > PREVIEW_MAX_BYTES {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "Preview is limited to 256 KiB. Open the file in Editor for supported text files."));
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    File::open(canonical)?.take(PREVIEW_MAX_BYTES + 1).read_to_end(&mut bytes)?;
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
    let canonical_root = root.canonicalize()?;
    let canonical_start = canonical_user_path(&canonical_root, start)?;
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
        let entries = fs::read_dir(&directory)?;
        for entry in entries {
            let entry = entry?;
            visited += 1;
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
    use super::{copy_item, move_item, read_text_preview, rename_item, search_user_files, unique_destination};
    use std::fs;
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
    fn unique_destination_preserves_multiple_extensions_and_unicode() {
        let temp = TestDir::new();
        let root = &temp.0;
        fs::write(root.join("ảnh.tar.gz"), "x").unwrap();
        let path = unique_destination(root, "ảnh.tar.gz", true).unwrap();
        assert_eq!(path.file_name().unwrap(), "ảnh.tar (copy).gz");
    }
}
