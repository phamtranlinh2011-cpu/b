# NOVA Native 0.5.1 architecture

## Runtime shape

```text
NOVA.exe (Rust native process)
├── eframe / egui / Glow root viewport
│   ├── desktop shell and navigation rail
│   ├── searchable launcher
│   ├── floating in-shell app windows
│   │   ├── Files / Editor / Calculator / Monitor / Settings
│   │   ├── Terminal / UniKey
│   │   ├── NOVA Trash
│   │   ├── Chrome launcher panel
│   │   ├── UniKey control panel
│   │   ├── Security Center
│   │   └── five external GUI security-tool workspaces
│   └── bottom dock/taskbar
├── portable Data store
│   ├── Users/Default/{Desktop,Documents,Downloads}
│   ├── Settings
│   ├── Logs
│   └── Trash/{Items,Meta}
└── Windows integration
    ├── cmd.exe one-shot commands
    ├── netsh wlan profile/scan operations
    ├── Windows Job Objects
    ├── Windows Sandbox (.wsb) isolated file/browser sessions
    ├── host Chrome discovery + isolated guest profile
    └── official UniKey companion with Authenticode checks
```

## Application presentation model

The desktop shell is one native eframe root window. Files, Editor, Terminal, Calculator, Monitor, Settings, Trash and Chrome panel are egui windows hosted inside that viewport. They are movable/resizable floating surfaces, use a consistent theme, and appear as active items in the dock. Launcher search opens an app; clicking an active dock item toggles its visibility. These are not independent native Windows top-level viewports. This keeps the desktop shell lightweight and lets NOVA control app layout consistently; separate native viewports remain a possible later architecture.

Motion uses short hover/fade transitions and egui animation primitives. The Reduce visual effects setting disables most transitions and decorative desktop shapes. Motion is feedback, not required to access controls. UI structure uses consistent spacing, rounded window/popup surfaces, selected accent color, and standard egui buttons, inputs, selectable rows and scroll areas.

## App modules

- `app.rs`: desktop shell, searchable launcher, dock, window state, Files/Editor/Trash/Chrome/UniKey/Security Center UI, worker-task coordination and settings.
- `apps.rs`: Chrome discovery, URL validation, bounded Google Authenticode publisher verification and normal host launch (requires explicit confirmation in the UI).
- `isolation.rs`: creates disposable Windows Sandbox `.wsb` sessions for offline file inspection and isolated Chrome; no writable host folders are mapped.
- `storage.rs`: portable path discovery with component-by-component symlink/junction/reparse-point rejection, settings/manifest file validation, atomic JSON writes and path containment.
- `terminal.rs`: UTF-8 CMD payload, Wi-Fi command parser, masked secrets, bounded command history and background task model.
- `unicode.rs`: strict UTF-8/UTF-16 LE/BE decoding; Editor saves Unicode as UTF-8.
- `unikey.rs`: embedded official UniKey download helper, bounded Authenticode publisher verification and USB-relative host process launcher.
- `windows_process.rs`: suspended process creation, Job Object assignment, timeout, output draining and bounded capture.
- `wifi.rs`: WLAN XML profile generation, protected TEMP file, netsh import/scan/connect/delete and stale-file cleanup.
- `trash.rs`: move/restore/permanent deletion and metadata for NOVA's own Trash; validates Trash roots and rejects reparse-point entries.
- `security_tools.rs`: detects and launches separately installed Zenmap, ZAP, Wireshark and Burp GUIs; opens a separately hosted Greenbone GSA console; validates the NOVA-side scope acknowledgement. It does not bundle scanner engines or enforce target allowlists inside those tools.
- `scripts/Download-UniKey.ps1`: readable source copy of the helper embedded into NOVA at compile time; the runtime Download action does not execute a mutable USB copy.

## Unicode and Vietnamese input

The built-in Editor decodes UTF-8 (with or without BOM) and UTF-16 LE/BE with BOM, then saves UTF-8. eframe's `system_fonts` feature and native `system_font_fallback` allow missing glyphs to be loaded from installed Windows fonts. The visible glyph coverage therefore depends on fonts installed on the host. CMD is invoked with `chcp 65001` before user commands; legacy console programs that still emit OEM/ANSI bytes may not display every character correctly. UniKey provides system-wide Vietnamese Telex/VNI input in Unicode mode and runs as a separate Windows host process.

## Disposable Sandbox profiles

- **File inspection:** copy one selected file from the NOVA user folder to `Data/Security/Quarantine/<session>`, map that copy read-only, disable network, vGPU, audio/video, printer and clipboard redirection, enable Protected Client, and open the folder in Sandbox without auto-running the sample.
- **Isolated Chrome:** map the detected Chrome application directory read-only, create a fresh guest-only profile at `C:\NOVAProfile`, disable vGPU/audio/video/printer/clipboard, enable Protected Client, and enable networking so websites can load. Networking can expose local-network services.
- Sandbox guest state is disposed when the user closes Windows Sandbox and confirms deletion. The USB-side file staging copy and `.wsb` config remain until the user explicitly clears them in Security Center after closing all Sandbox sessions.
- Windows Sandbox is provided by Windows, not bundled with NOVA. The supported edition, virtualization, RAM, CPU and disk requirements apply. NOVA fails closed when it cannot launch Sandbox; it never falls back to host execution.

## Portable data paths

1. Resolve `NOVA.exe` using `current_exe()`.
2. If `portable.flag` exists beside the executable, store data under `Data/` beside it.
3. Otherwise use `%LOCALAPPDATA%/NOVA OS/Data`.
4. Files and Editor are scoped to `Data/Users/Default`.
5. Trash is `Data/Trash/Items` plus JSON metadata under `Data/Trash/Meta`.
6. Sandbox sample staging and launch configs live under `Data/Security/`; they are host-side copies and do not disappear automatically when the guest closes.

## Windows process runner

CMD and WLAN work runs outside the egui UI thread. The runner creates the process suspended, assigns it to a Job Object with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, then verifies and resumes the expected initial thread. This avoids executing unassigned child code before the Job Object takes ownership. It drains stdout/stderr continuously, retaining at most 1 MiB per stream. CMD commands have a 30-second deadline, WLAN helper operations have a 20-second deadline, and UniKey/Chrome publisher checks have a 15-second deadline. Reader wait/cleanup has bounded grace periods. Interactive stdin and persistent command shells are not supported.

## UniKey verification and host boundary

The UniKey helper is compiled into `NOVA.exe` from `scripts/Download-UniKey.ps1`, rather than loaded from a potentially replaced USB script at runtime. It downloads only the official UniKey x64 ZIP, enforces archive/executable size limits, checks a valid Authenticode signature whose simple publisher name is exactly `PHAM KIM LONG`, copies the executable onto USB and checks the signature again. NOVA checks it again immediately before launch using a bounded PowerShell process. The executable path is passed as a process environment value rather than embedded in PowerShell source, preserving Unicode paths. UniKey starts with its own directory as the working directory so USB-relative resources can be resolved. This is a provenance/integrity check, not a guarantee that UniKey is free of defects. UniKey runs on the Windows host and is not part of Windows Sandbox.

## Wi-Fi integration

Wi-Fi commands are intercepted before normal CMD forwarding. Profiles are prefixed `NOVA::`; the JSON manifest stores SSID names managed by NOVA. Deletion requires the manifest entry and fails closed if the manifest is corrupt. Profile XML is created in TEMP with a protected DACL for owner, SYSTEM and Administrators. The writer is closed before `netsh` imports it, then the file is removed; startup removes matching NOVA profile XML files older than ten minutes. The credential is plaintext while the XML exists. `netsh` text parsing may vary with Windows display language.

## NOVA Trash and Chrome

NOVA Trash is a separate portable feature; same-volume rename moves items from the NOVA user folder into `Data/Trash/Items`. Restore avoids overwriting existing files by generating a `(restored N)` name. Empty/delete permanently removes the selected NOVA Trash content. It does not call Windows Shell Recycle Bin APIs and cannot collect items deleted outside NOVA Files.

Normal host Chrome is a user-confirmed external process and is not isolated. Isolated Chrome uses a new guest-only `--user-data-dir` inside Windows Sandbox; its normal Windows profile is not mapped. The browser executable directory is mounted read-only; browser profile, cookies, extensions and downloads remain in the guest. The Chrome application itself is not bundled or installed by NOVA.

## Rendering/runtime constraints

The app uses eframe's Glow renderer to avoid enabling the default WGPU renderer. Windows provides OpenGL libraries and graphics drivers. High-DPI and GPU coverage needs validation on real target machines. No NOVA service, auto-run registration or bootloader is installed.


## Recursive deletion safety

NOVA Trash and Security Center cleanup use a no-follow recursive remover. Normal directories are walked explicitly; a symlink or Windows reparse-point directory is removed as a link and its target is never enumerated. This avoids recursive deletion through a nested junction in a folder moved to NOVA Trash.
