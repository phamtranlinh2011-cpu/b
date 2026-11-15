# NOVA Native architecture

```text
NOVA.exe (Rust native process)
├── eframe / egui / Glow
│   ├── Desktop shell
│   ├── Launcher and taskbar
│   ├── Files
│   ├── Editor
│   ├── Calculator
│   ├── Settings
│   └── System Monitor
├── Portable Storage
│   ├── Data/Users/Default
│   └── Data/Settings
└── Windows Integration
    ├── cmd.exe (one-shot commands, current user permissions)
    └── netsh wlan (scan and managed WLAN profile operations)
```

## Runtime constraints

The target process is a native Windows executable. No embedded browser engine, Node runtime, JavaScript bundler, or NOVA service is required. Windows supplies the command interpreter, WLAN service, OpenGL DLL/driver and operating-system APIs.

## Portable path resolution

1. Resolve `NOVA.exe` with `current_exe()`.
2. If `portable.flag` exists beside the executable, use `Data/` beside it.
3. Otherwise use `%LOCALAPPDATA%/NOVA OS/Data`.
4. User documents and desktop folders are scoped below `Data/Users/Default`.

## UI framework

`eframe` with its `glow` renderer is used to avoid the larger default WGPU renderer in this lightweight desktop shell. The current app's windows are panels hosted within the NOVA main window, not independent native OS top-level processes. A future window manager can promote these to native viewports if needed.

## Terminal

CMD input is executed by `cmd.exe /D /S /C` on a dedicated worker thread using the current user's permissions. NOVA maintains the working directory between commands. The shared Windows process runner creates each child suspended, assigns it to a Job Object configured with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, and only then resumes its initial thread. It enforces a 30-second CMD deadline and 20-second WLAN deadlines. Stdout and stderr are drained continuously while retaining at most 1 MiB per stream, so capture limits do not stop pipe consumption. Terminal history is bounded to 1,500 lines and 4 MiB.

Interactive shells (`cmd` without arguments), programs that require interactive stdin, and persistent environment changes are not supported by this one-shot command model.

## Wi-Fi

The parser intercepts `wifi` and `/wifi` before forwarding commands to CMD. The Wi-Fi service uses `netsh wlan` and creates profiles with a `NOVA::` prefix. An SSID allow-list in `Data/Settings/wifi-profiles.json` records only profiles NOVA manages. Deletion is refused unless the SSID is in that manifest.

The password is briefly placed in a temporary WLAN XML file so `netsh` can import it. On Windows the file is created with a protected DACL for the file owner, SYSTEM and local Administrators; NOVA closes the writer before starting the import and removes the file immediately after the import process exits. Startup cleanup removes matching abandoned files older than 10 minutes. The file is plaintext while it exists, so SYSTEM and local Administrators are trusted. A future iteration may replace this route with direct Windows WLAN API calls to avoid a credential-bearing file entirely.
