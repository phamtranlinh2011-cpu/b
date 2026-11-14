# NOVA OS Native 0.3.2

**NOVA Native** is a portable Windows desktop environment built in Rust with `egui/eframe`. It is a normal native Windows application, not a replacement for Windows and not yet a bootable USB operating system.

## User-facing goal

The finished USB package is intended to work on a compatible Windows 10/11 x64 computer without installing Node.js, npm, Rust, Cargo, WebView2, a separate runtime, or NOVA itself. The user opens `NOVA.exe` manually from the USB. Windows does not reliably auto-run software merely because removable media was inserted, by design.

The target computer still supplies Windows system components and drivers. NOVA uses built-in `cmd.exe` for the real command shell and `netsh wlan` for Windows Wi-Fi management. The `eframe`/Glow renderer uses Windows OpenGL support; an up-to-date compatible graphics driver is recommended. Windows' basic OpenGL fallback may be limited.

## Features in this native first pass

- Native desktop window with NOVA launcher, sidebar, app tiles and taskbar.
- Files app scoped to `Data/Users/Default` (no access to arbitrary host folders through the Files UI).
- Text editor for UTF-8 text files up to 2 MiB.
- Calculator, dark/light settings, and system monitor (CPU, memory, process count).
- Windows CMD terminal using the current signed-in Windows user's permissions.
- Terminal `cd` state persists between one-shot `cmd.exe /C` invocations. `cd`, `cd /d`, `cd..`, and `cd\` are handled by NOVA.
- Wi-Fi commands: `wifi`, `wifi -h`, `wifi connect "SSID" [password]`, `wifi select "SSID"`, `password <password>`, `password delete`, `exit`.
- Settings and user files stored beside the executable in portable mode.

## Important distinctions

- **Build machine:** compiling this Rust project requires Rust/Cargo and Microsoft C++ Build Tools for the MSVC target. Those are development tools only.
- **Target machine:** the built portable package does not require Rust, Cargo, Node.js, npm, WebView2, or an installer.
- **One EXE:** `NOVA.exe` is the only application executable. `portable.flag` and `Data/` are sidecar files/folders that let the EXE detect portable mode and persist user data. Keep them together.
- **Windows tools:** CMD and Wi-Fi commands invoke `cmd.exe` and `netsh.exe` internally; users do not need to download these tools because they are supplied by supported Windows installations.
- **Not a kernel:** NOVA is a desktop environment running on Windows, not a separate OS kernel or bootable Live USB. Its built-in app windows are panels inside the main NOVA window in this first native version.

## Build locally on Windows

Requirements on the *developer/build computer*:

1. Rust stable (MSVC toolchain), Rust 1.95 or newer.
2. Microsoft C++ Build Tools / Visual Studio Build Tools with MSVC and Windows SDK.
3. Internet access for the first Cargo dependency download.

Node.js, npm and WebView2 are not used by this project.

Open this folder in Windows and run:

```powershell
.\BUILD_NOVA.bat
```

Or in PowerShell:

```powershell
.\scripts\build-portable.ps1
```

The source archive does not currently include `Cargo.lock`. Cargo creates a lockfile during the first CI build, but the runner does not automatically commit it back to the repository. For reproducible future builds, retain and commit the generated `Cargo.lock` after a successful build.

The build script creates:

```text
release/
├── NOVA-Portable/
│   ├── NOVA.exe
│   ├── portable.flag
│   ├── README_PORTABLE.txt
│   ├── Data/
│   │   ├── Users/Default/Desktop/
│   │   ├── Users/Default/Documents/
│   │   ├── Users/Default/Downloads/
│   │   ├── Settings/
│   │   └── Logs/
│   └── Backup/
├── NOVA-OS-Native-v0.3.2-USB.zip
└── NOVA-OS-Native-v0.3.2-USB.zip.sha256
```

Copy the whole `NOVA-Portable` folder to the USB. Do not copy only `NOVA.exe`, because portable mode and saved data require the sidecar files/folders.

## Build without installing Rust on your own computer

This repository includes `.github/workflows/windows-build.yml`. Upload the source repository to GitHub, open **Actions → Build NOVA Native for Windows → Run workflow**, and download the generated artifact. GitHub's Windows runner runs unit tests, builds the EXE and uploads the prepared USB folder as a downloadable ZIP artifact. The computer that runs NOVA still needs no developer tools. Follow [BUILD_WITH_GITHUB.md](BUILD_WITH_GITHUB.md) for step-by-step instructions.

## Wi-Fi commands

```text
wifi
wifi -h
wifi connect "Home WiFi" "your-password"
wifi connect "Open Guest Network"
wifi select "Home WiFi"
<wifi-Home WiFi>: password "your password"
<wifi-Home WiFi>: password delete
<wifi-Home WiFi>: exit
```

Both `wifi ...` and `/wifi ...` are accepted. SSIDs containing spaces must be quoted. `password` input in Wi-Fi setup mode and the direct `wifi connect` input are masked in the terminal UI.

The initial implementation supports open networks and WPA2-Personal (AES), using printable ASCII passphrases or a 64-character hexadecimal key. WPA3-only, Enterprise/802.1X, captive portal login, hidden-network setup, and advanced adapter selection are not implemented. A scan result does not guarantee the adapter can connect.

**Credential detail:** NOVA does not save Wi-Fi passwords in its own Data folder, settings or terminal history. To import a Windows WLAN profile via `netsh`, a temporary XML file containing the password is written in the current user's TEMP folder and NOVA attempts to delete it immediately after Windows imports it. Abrupt termination or power loss can leave the plaintext XML behind in TEMP; this release does not apply a custom Windows file ACL. Windows then stores/manages its WLAN profile. NOVA records only the SSID names of profiles it created, so `password delete` can refuse to delete profiles it does not manage. The Windows profile itself may persist after the USB is removed until it is deleted.

## CMD safety

The Terminal is a real Windows command shell, not a sandbox. It runs as the currently signed-in Windows user and is not automatically elevated. Commands such as `del`, `format`, `diskpart`, `shutdown`, and scripts can affect the actual computer. Run only commands you understand. Each command is a one-shot process; working directory is preserved, but interactive shells and environment-variable changes do not persist. NOVA requests process-tree termination after 30 seconds and also attempts to stop `cmd.exe` directly. Output readers use a bounded drain period so an inherited pipe cannot freeze the NOVA UI indefinitely; a surviving descendant may still leave a detached reader until its pipe handle closes. Captured output is limited to 1 MiB per stream, with truncation reported.

## Portability and performance notes

- The MSVC C runtime is statically linked through `.cargo/config.toml` to avoid a separate Visual C++ runtime dependency in the target package.
- The release profile uses size optimization, LTO, one codegen unit and symbol stripping.
- Glow uses the system OpenGL implementation/driver; Windows' `opengl32.dll` is a system component, but a compatible graphics driver is still needed.
- No benchmark numbers are claimed yet. Measure cold launch, warm launch, idle RAM/CPU, and USB read latency on real Windows computers before publishing a performance target. System Monitor is lazy-started: NOVA does not scan the process list until that window is opened.
- The executable is not code-signed in this source package. Windows SmartScreen or antivirus may warn about an unfamiliar binary; a public release should use an Authenticode code-signing certificate.
- Do not remove the USB while NOVA is writing data. Close NOVA and safely eject the drive.

## Tests

Run on a developer machine:

```powershell
cargo test --target x86_64-pc-windows-msvc
cargo build --release --target x86_64-pc-windows-msvc
```

Then test on at least one Windows 10 and one Windows 11 computer without Rust/Node/WebView2 installed. Test USB persistence, command timeout, CMD behavior, open Wi-Fi and WPA2-Personal Wi-Fi. Do not use a critical network during initial validation.

## Project structure

```text
src/main.rs          Windows native entry point
src/app.rs           NOVA desktop UI and built-in apps
src/storage.rs       Portable/local data paths, settings, Wi-Fi manifest
src/terminal.rs      CMD terminal and Wi-Fi command parser
src/wifi.rs          Windows WLAN scan/profile/connect/delete logic
scripts/             Local Windows build and USB packager
.github/workflows/   Cloud Windows build workflow
docs/                Architecture and release notes
```
