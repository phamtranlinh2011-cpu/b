//! Native Windows file clipboard interoperability (CF_HDROP).
//!
//! NOVA never exports raw file contents into the clipboard. The clipboard holds
//! file paths, as Windows Explorer does. Imports into NOVA are performed by the
//! bounded, no-follow copy pipeline in `file_ops` and never move external data.

use std::io;
use std::path::PathBuf;

#[cfg(windows)]
mod windows_clipboard {
    use super::*;
    use std::ffi::OsStr;
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use std::ptr;

    const CF_HDROP: u32 = 15;
    const GMEM_MOVEABLE: u32 = 0x0002;
    const GMEM_ZEROINIT: u32 = 0x0040;
    const DROPEFFECT_COPY: u32 = 1;
    const DROPEFFECT_MOVE: u32 = 2;
    const MAX_CLIPBOARD_PATH_CHARS: usize = 2_000_000;

    #[repr(C)]
    struct DropFiles {
        p_files: u32,
        x: i32,
        y: i32,
        non_client: i32,
        wide: i32,
    }

    const _: () = assert!(std::mem::size_of::<DropFiles>() == 20);

    #[link(name = "user32")]
    unsafe extern "system" {
        fn OpenClipboard(owner: *mut std::ffi::c_void) -> i32;
        fn GetForegroundWindow() -> *mut std::ffi::c_void;
        fn CloseClipboard() -> i32;
        fn EmptyClipboard() -> i32;
        fn SetClipboardData(format: u32, memory: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
        fn GetClipboardData(format: u32) -> *mut std::ffi::c_void;
        fn IsClipboardFormatAvailable(format: u32) -> i32;
        fn RegisterClipboardFormatW(name: *const u16) -> u32;
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GlobalAlloc(flags: u32, bytes: usize) -> *mut std::ffi::c_void;
        fn GlobalLock(memory: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
        fn GlobalUnlock(memory: *mut std::ffi::c_void) -> i32;
        fn GlobalFree(memory: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
        fn GlobalSize(memory: *mut std::ffi::c_void) -> usize;
    }

    #[link(name = "shell32")]
    unsafe extern "system" {
        fn DragQueryFileW(drop: *mut std::ffi::c_void, index: u32, path: *mut u16, path_chars: u32) -> u32;
    }

    struct ClipboardGuard;
    impl ClipboardGuard {
        fn open() -> io::Result<Self> {
            let owner = unsafe { GetForegroundWindow() };
            if owner.is_null() {
                return Err(io::Error::new(io::ErrorKind::NotFound, "NOVA could not find its foreground window to access the Windows clipboard."));
            }
            if unsafe { OpenClipboard(owner) } == 0 {
                Err(io::Error::last_os_error())
            } else {
                Ok(Self)
            }
        }
    }
    impl Drop for ClipboardGuard {
        fn drop(&mut self) { unsafe { let _ = CloseClipboard(); } }
    }

    fn allocate_global(bytes: &[u8]) -> io::Result<*mut std::ffi::c_void> {
        let memory = unsafe { GlobalAlloc(GMEM_MOVEABLE | GMEM_ZEROINIT, bytes.len()) };
        if memory.is_null() { return Err(io::Error::last_os_error()); }
        let locked = unsafe { GlobalLock(memory) };
        if locked.is_null() {
            let error = io::Error::last_os_error();
            unsafe { let _ = GlobalFree(memory); }
            return Err(error);
        }
        unsafe {
            ptr::copy_nonoverlapping(bytes.as_ptr(), locked.cast::<u8>(), bytes.len());
            let _ = GlobalUnlock(memory);
        }
        Ok(memory)
    }

    pub fn set_files(paths: &[PathBuf], cut: bool) -> io::Result<()> {
        if paths.is_empty() || paths.len() > 2_000 {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "Select between 1 and 2,000 files or folders to copy."));
        }
        let header_size = std::mem::size_of::<DropFiles>();
        let mut wide_paths = Vec::<u16>::new();
        let mut total_path_chars = 0usize;
        for path in paths {
            if !path.is_absolute() {
                return Err(io::Error::new(io::ErrorKind::InvalidInput, "Windows file clipboard requires absolute paths."));
            }
            let encoded = path.as_os_str().encode_wide().collect::<Vec<_>>();
            if encoded.is_empty() || encoded.len() >= 32_767 || encoded.contains(&0) {
                return Err(io::Error::new(io::ErrorKind::InvalidInput, "A selected path is invalid or too long for the Windows clipboard."));
            }
            total_path_chars = total_path_chars.checked_add(encoded.len() + 1)
                .filter(|total| *total <= MAX_CLIPBOARD_PATH_CHARS)
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "The selected paths exceed the 2-million-character Windows clipboard safety limit."))?;
            wide_paths.extend(encoded);
            wide_paths.push(0);
        }
        wide_paths.push(0);
        let mut payload = vec![0u8; header_size + wide_paths.len() * std::mem::size_of::<u16>()];
        let header = DropFiles { p_files: header_size as u32, x: 0, y: 0, non_client: 0, wide: 1 };
        unsafe {
            ptr::write_unaligned(payload.as_mut_ptr().cast::<DropFiles>(), header);
            ptr::copy_nonoverlapping(
                wide_paths.as_ptr().cast::<u8>(),
                payload.as_mut_ptr().add(header_size),
                wide_paths.len() * std::mem::size_of::<u16>(),
            );
        }
        let drop_memory = allocate_global(&payload)?;
        let effect_name: Vec<u16> = OsStr::new("Preferred DropEffect").encode_wide().chain(Some(0)).collect();
        let effect_format = unsafe { RegisterClipboardFormatW(effect_name.as_ptr()) };
        let effect_bytes = (if cut { DROPEFFECT_MOVE } else { DROPEFFECT_COPY }).to_ne_bytes();
        let effect_memory = if effect_format != 0 { allocate_global(&effect_bytes).ok() } else { None };

        let _clipboard = match ClipboardGuard::open() {
            Ok(guard) => guard,
            Err(error) => {
                unsafe { let _ = GlobalFree(drop_memory); if let Some(memory) = effect_memory { let _ = GlobalFree(memory); } }
                return Err(error);
            }
        };
        if unsafe { EmptyClipboard() } == 0 {
            let error = io::Error::last_os_error();
            unsafe { let _ = GlobalFree(drop_memory); if let Some(memory) = effect_memory { let _ = GlobalFree(memory); } }
            return Err(error);
        }
        if unsafe { SetClipboardData(CF_HDROP, drop_memory) }.is_null() {
            let error = io::Error::last_os_error();
            unsafe { let _ = GlobalFree(drop_memory); if let Some(memory) = effect_memory { let _ = GlobalFree(memory); } }
            return Err(error);
        }
        if let (Some(format), Some(memory)) = (Some(effect_format).filter(|value| *value != 0), effect_memory) {
            if unsafe { SetClipboardData(format, memory) }.is_null() {
                unsafe { let _ = GlobalFree(memory); }
                // CF_HDROP is already installed and remains useful as Copy.
            }
        }
        Ok(())
    }

    pub fn get_files() -> io::Result<Option<(Vec<PathBuf>, bool)>> {
        let _clipboard = ClipboardGuard::open()?;
        if unsafe { IsClipboardFormatAvailable(CF_HDROP) } == 0 { return Ok(None); }
        let drop_handle = unsafe { GetClipboardData(CF_HDROP) };
        if drop_handle.is_null() { return Err(io::Error::last_os_error()); }
        if unsafe { GlobalSize(drop_handle) } < std::mem::size_of::<DropFiles>() {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "Windows clipboard returned an invalid CF_HDROP payload."));
        }
        let count = unsafe { DragQueryFileW(drop_handle, u32::MAX, ptr::null_mut(), 0) } as usize;
        if count == 0 { return Ok(None); }
        if count > 2_000 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "Windows clipboard contains more than 2,000 items; narrow the selection first."));
        }
        let mut paths = Vec::with_capacity(count);
        let mut total_path_chars = 0usize;
        for index in 0..count {
            let length = unsafe { DragQueryFileW(drop_handle, index as u32, ptr::null_mut(), 0) } as usize;
            if length == 0 || length >= 32_767 {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "Windows clipboard contains an invalid or overlong path."));
            }
            total_path_chars = total_path_chars.checked_add(length + 1)
                .filter(|total| *total <= MAX_CLIPBOARD_PATH_CHARS)
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Windows clipboard paths exceed the 2-million-character safety limit."))?;
            let mut wide = vec![0u16; length + 1];
            let copied = unsafe { DragQueryFileW(drop_handle, index as u32, wide.as_mut_ptr(), wide.len() as u32) } as usize;
            if copied != length {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "Windows clipboard path changed while it was being read."));
            }
            wide.truncate(copied);
            let path = PathBuf::from(std::ffi::OsString::from_wide(&wide));
            if !path.is_absolute() { return Err(io::Error::new(io::ErrorKind::InvalidData, "Windows clipboard contains a relative path.")); }
            paths.push(path);
        }
        let effect_name: Vec<u16> = OsStr::new("Preferred DropEffect").encode_wide().chain(Some(0)).collect();
        let effect_format = unsafe { RegisterClipboardFormatW(effect_name.as_ptr()) };
        let cut = if effect_format != 0 {
            let effect_memory = unsafe { GetClipboardData(effect_format) };
            if effect_memory.is_null() || unsafe { GlobalSize(effect_memory) } < std::mem::size_of::<u32>() {
                false
            } else {
                let locked = unsafe { GlobalLock(effect_memory) };
                if locked.is_null() { false } else {
                    let effect = unsafe { ptr::read_unaligned(locked.cast::<u32>()) };
                    unsafe { let _ = GlobalUnlock(effect_memory); }
                    effect & DROPEFFECT_MOVE != 0
                }
            }
        } else { false };
        Ok(Some((paths, cut)))
    }
}

#[cfg(windows)]
pub fn set_files(paths: &[PathBuf], cut: bool) -> io::Result<()> {
    windows_clipboard::set_files(paths, cut)
}

#[cfg(windows)]
pub fn get_files() -> io::Result<Option<(Vec<PathBuf>, bool)>> {
    windows_clipboard::get_files()
}

#[cfg(not(windows))]
pub fn set_files(_paths: &[PathBuf], _cut: bool) -> io::Result<()> {
    Err(io::Error::new(io::ErrorKind::Unsupported, "Windows file clipboard is only available in the Windows build of NOVA."))
}

#[cfg(not(windows))]
pub fn get_files() -> io::Result<Option<(Vec<PathBuf>, bool)>> { Ok(None) }
