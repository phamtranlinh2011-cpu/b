# NOVA Native 0.3.1 — build status

## Delivered in this source package
- Replaced Tauri/Svelte/WebView2 architecture with Rust + eframe/egui native GUI.
- Added native desktop shell, launcher, taskbar, Files, Editor, Settings, Calculator and System Monitor.
- Added Windows CMD terminal with working-directory state, 30-second timeout and process-tree termination.
- Added `wifi`, `/wifi`, `wifi -h`, `wifi connect`, `wifi select`, `password`, `password delete`, and `exit` flows.
- Added portable data path using `portable.flag` and `Data/` beside NOVA.exe.
- Added Windows local packager and GitHub Actions workflow to run unit tests, create the Windows EXE and package the USB ZIP.
- Deferred System Monitor process enumeration until the monitor window is opened to reduce startup work.
- Added a Windows message box for portable-data initialization failures in GUI release builds.
- Added `BUILD_WITH_GITHUB.md` for a cloud Windows build without installing Rust on the user's own PC.
- Configured static MSVC CRT and a size-optimized release profile.

## 0.3.1 bug-fix changes
- Files canonicalizes the user-data root, current directory and selected entries before listing/navigation/opening; folder creation also revalidates its parent.
- Settings persistence errors are shown in the app status bar.
- Wi-Fi temporary XML cleanup now uses a scope guard and is attempted on every normal return path; cleanup happens immediately after `netsh` import. The file still has no custom Windows DACL, so abrupt termination may leave plaintext XML in TEMP.
- Added unit tests for canonical path containment and temporary-profile cleanup.

## Not yet verified
- Rust compilation and unit tests: not run in this container (Rust/Cargo are unavailable). Static source/config checks only; GitHub Actions must run them.
- Windows EXE: not compiled here; a real Windows runner is required.
- Wi-Fi operations: not tested against a physical wireless adapter/router.
- Cold-start/RAM/CPU benchmarks: not measured.
- OpenGL compatibility: requires validation on target PCs.

## Important
This is a source-code milestone, not a precompiled EXE release. The workflow `.github/workflows/windows-build.yml` can compile the EXE on GitHub's Windows runner after the project is pushed to a repository. The resulting target package should not require Rust, Cargo, Node.js, npm or WebView2.
