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

CMD input is executed by `cmd.exe /D /S /C` using the current user's permissions. NOVA maintains the working directory between commands. It polls a 30-second deadline and uses `taskkill /T /F` to stop the process tree when a command times out. Output readers cap each stream to roughly 1 MiB.

Interactive shells (`cmd` without arguments), programs that require interactive stdin, and persistent environment changes are not supported by this one-shot command model.

## Wi-Fi

The parser intercepts `wifi` and `/wifi` before forwarding commands to CMD. The Wi-Fi service uses `netsh wlan` and creates profiles with a `NOVA::` prefix. An SSID allow-list in `Data/Settings/wifi-profiles.json` records only profiles NOVA manages. Deletion is refused unless the SSID is in that manifest.

The password is placed in a temporary WLAN XML file so `netsh` can import it, then the temporary file is deleted. The password is not written into NOVA's own database or logs. A future iteration should replace the temporary XML route with direct Windows WLAN API calls to keep the credential entirely in process memory.
