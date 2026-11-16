//! Portable integration with the official UniKey Windows Vietnamese input app.
//!
//! UniKey is bundled into each portable package by the Windows release script.
//! Runtime discovery and launch always verify the official Authenticode publisher.

use std::io;
use std::path::{Path, PathBuf};

pub fn find_portable(app_dir: &Path) -> Option<PathBuf> {
    let root = app_dir.canonicalize().ok()?;
    let candidates = [
        root.join("Apps/UniKey/UniKeyNT.exe"),
        root.join("Apps/UniKey/UniKey.exe"),
        root.join("UniKeyNT.exe"),
    ];
    candidates.into_iter().find(|path| {
        let Ok(canonical) = path.canonicalize() else { return false; };
        if !canonical.starts_with(&root) || !canonical.is_file() { return false; }

        // Check each existing component rather than trusting canonicalization
        // alone; a USB junction must not silently redirect the companion.
        let mut current = path.as_path();
        loop {
            let Ok(metadata) = std::fs::symlink_metadata(current) else { return false; };
            if crate::storage::is_link_or_reparse_point(&metadata) { return false; }
            if current == root.as_path() { break; }
            let Some(parent) = current.parent() else { return false; };
            current = parent;
        }
        true
    }).and_then(|path| path.canonicalize().ok())
}

#[cfg(windows)]
pub fn verify_official_signature(path: &Path) -> io::Result<()> {
    use std::process::Command;
    use std::time::Duration;

    if !path.is_file() {
        return Err(io::Error::new(io::ErrorKind::NotFound, "UniKey executable was not found."));
    }
    let powershell = crate::windows_process::system_binary("WindowsPowerShell\\v1.0\\powershell.exe")?;
    // Pass the path as an environment value instead of interpolating it into
    // PowerShell source. This preserves Unicode and avoids script injection
    // through unusual file names.
    let script = "$ErrorActionPreference='Stop'; $sig=Get-AuthenticodeSignature -LiteralPath $env:NOVA_VERIFY_PATH; if ($sig.Status -eq 'Valid' -and $sig.SignerCertificate.GetNameInfo([System.Security.Cryptography.X509Certificates.X509NameType]::SimpleName, $false) -ceq 'PHAM KIM LONG') { exit 0 }; exit 17";
    let mut command = Command::new(powershell);
    command
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .env("NOVA_VERIFY_PATH", path);
    let output = crate::windows_process::run_captured(command, Duration::from_secs(15))?;
    if output.timed_out || output.drain_timed_out {
        return Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "UniKey signature verification timed out. Check Windows certificate services and try again.",
        ));
    }
    if output.status.success() {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "UniKey signature is invalid or is not issued to PHAM KIM LONG. Do not run this file.",
        ))
    }
}

#[cfg(not(windows))]
pub fn verify_official_signature(_path: &Path) -> io::Result<()> {
    Err(io::Error::new(io::ErrorKind::Unsupported, "UniKey integration is available in the Windows build of NOVA."))
}

#[cfg(windows)]
pub fn launch_verified(path: &Path) -> io::Result<std::process::Child> {
    use std::process::Command;
    verify_official_signature(path)?;
    let working_directory = path.parent().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "UniKey executable has no parent directory.")
    })?;
    // Keep UniKey's relative configuration/resource files beside the USB copy
    // instead of inheriting NOVA's potentially unrelated working directory.
    Command::new(path).current_dir(working_directory).spawn()
}

#[cfg(not(windows))]
pub fn launch_verified(_path: &Path) -> io::Result<std::process::Child> {
    Err(io::Error::new(io::ErrorKind::Unsupported, "UniKey integration is available in the Windows build of NOVA."))
}

#[cfg(test)]
mod tests {
    use super::find_portable;
    use std::fs;

    #[test]
    fn finds_only_known_portable_unikey_locations() {
        let root = std::env::temp_dir().join(format!("nova-unikey-test-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let target = root.join("Apps/UniKey/UniKeyNT.exe");
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(&target, b"test placeholder").unwrap();
        // Windows canonicalize() may return an extended-length path (\\?\)
        // and expand short/long names. Compare canonical paths on every platform
        // instead of comparing their presentation forms.
        let expected = target.canonicalize().expect("test UniKey path should canonicalize");
        assert_eq!(find_portable(&root), Some(expected));
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn refuses_unikey_below_a_symlinked_apps_directory() {
        use std::os::unix::fs::symlink;
        let root = std::env::temp_dir().join(format!("nova-unikey-link-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let real_apps = root.join("RealApps/UniKey");
        let linked_apps = root.join("Apps");
        std::fs::create_dir_all(&real_apps).unwrap();
        std::fs::write(real_apps.join("UniKeyNT.exe"), b"placeholder").unwrap();
        symlink(root.join("RealApps"), &linked_apps).unwrap();
        assert_eq!(find_portable(&root), None);
        let _ = fs::remove_file(&linked_apps);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn does_not_find_missing_unikey() {
        let root = std::env::temp_dir().join(format!("nova-unikey-missing-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        assert_eq!(find_portable(&root), None);
    }


}
