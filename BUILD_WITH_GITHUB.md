# Build NOVA Native 0.5.6 on GitHub (without installing Rust locally)

1. Extract the NOVA 0.5.6 source ZIP.
2. Upload the complete contents to the repository root, keeping `.github/` and `.cargo/` (including hidden folders).
3. Commit to `main`. The `push` trigger runs the Windows workflow automatically. You can also open **Actions → Build NOVA Native for Windows → Run workflow**.
4. The workflow uses pinned Rust 1.95.0 and `x86_64-pc-windows-msvc`.
5. It generates or refreshes `Cargo.lock` if missing or if the root `nova-native` package version changed, commits the lockfile using the Actions token, then runs `cargo test --locked` and `cargo build --locked --release`.
6. On success, download artifact **`NOVA-OS-Native-v0.5.6-USB`** from the bottom of the successful workflow run.
7. Extract the artifact and copy the entire folder to USB. Keep `NOVA.exe`, `portable.flag`, `README_PORTABLE.txt`, `BUILD_INFO.txt`, `scripts/Download-UniKey.ps1` and `Data/` together.

Repository Actions permissions must allow `contents: write` for the lockfile commit when the package version changes. Branch protection or organization policy may block the workflow from pushing the lockfile; if that happens, add a generated `Cargo.lock` manually and rerun.

A green workflow confirms the Windows tests/build/package steps ran successfully for that commit. It does not replace real-device testing of Unicode/UniKey typing, Windows Sandbox availability, Chrome mapped-folder operation, USB write behavior, Wi-Fi adapters or graphics drivers.
