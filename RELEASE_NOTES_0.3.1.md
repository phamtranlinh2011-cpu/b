# NOVA Native 0.3.1 — Bug-fix release

## Changes
- Files now canonicalizes the current folder and selected entries before navigation/opening, and checks the destination before creating folders. This blocks paths that resolve outside `Data/Users/Default`, including common symlink/junction escape cases.
- Settings save failures are surfaced in the status bar instead of being silently ignored.
- Wi-Fi temporary profile cleanup is guarded on all normal Rust exit paths, including import failures.
- Unit coverage added for canonical path containment.
- Build workflow and portable artifact names updated to 0.3.1.

## Known limitations / validation
- The Wi-Fi XML profile temporarily contains the WPA2 key in plaintext in the current user's TEMP directory so Windows `netsh` can import it. The file is created with `create_new` and cleanup is attempted on scope exit, but this release does not set a custom Windows DACL; abrupt power loss/process termination may leave the file behind. Use only on a trusted Windows account/device.
- CMD process-tree timeout behavior and actual WLAN connection/deletion still require runtime testing on Windows 10/11.
- A Rust compiler/Cargo toolchain and Windows MSVC runner are required to run `cargo test` and produce the EXE.
