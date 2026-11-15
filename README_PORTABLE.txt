NOVA OS NATIVE 0.5.1 — USB PORTABLE EDITION

START
1. Keep NOVA.exe, portable.flag and the Data folder together in one folder.
2. Plug the USB drive into Windows 10/11 x64, open it in File Explorer and double-click NOVA.exe.
3. The target PC does not need Node.js, npm, Rust, Cargo, WebView2 or a NOVA installer.
4. NOVA stores documents, settings, Trash and security staging data in Data/ beside NOVA.exe.
5. Close NOVA before safely ejecting the USB drive.
6. NOVA refuses to use core Data folders that are symlinks, junctions or Windows reparse points; do not replace its Data tree with links.

UNICODE + UNIKEY
- NOVA Editor reads UTF-8 (with/without BOM) and UTF-16 LE/BE text. Save writes UTF-8.
- Windows paths and NOVA text fields use Unicode. NOVA Terminal starts CMD with code page 65001 (UTF-8).
- Open UniKey in the NOVA Launcher, choose Download UniKey to USB, then Start UniKey.
- The Download action uses a helper embedded in NOVA.exe (not a mutable USB script), downloads UniKey 4.6 RC2 x64 from unikey.org, enforces archive size limits and checks the exact Authenticode publisher PHAM KIM LONG before launch.
- In UniKey's tray menu choose Character set: Unicode, then input method Telex or VNI. UniKey runs system-wide as a normal user process; NOVA does not install a service or startup task.

APPS
- Files: manage files inside Data/Users/Default. Single-click selects; double-click opens. A selected file can be copied into offline Windows Sandbox for inspection.
- Editor: Unicode text editor for UTF-8 and UTF-16 LE/BE files up to 2 MiB; saves UTF-8 inside the NOVA user folder.
- Calculator: basic +, -, x and division operations.
- System Monitor: host CPU, memory and process count.
- Settings: dark/light theme, accent color, reduce motion, plus links to UniKey and Security Center.
- Terminal: Windows CMD and NOVA Wi-Fi commands. CMD executes on the real Windows host with your user permissions.
- NOVA Trash: restores files moved from NOVA Files. Emptying it permanently deletes those NOVA items.
- Google Chrome: opens existing host Chrome normally or in a fresh Windows Sandbox profile.
- Security Center: authorized-scope workflow reminder, Windows Sandbox status, staged-copy cleanup and launch workspaces for five GUI security tools. The scope acknowledgement is not a firewall or technical target restriction.
- Network Discovery: launches the separately installed Zenmap/Nmap GUI.
- Vulnerability Management: opens a separately hosted Greenbone Security Assistant (GSA) console; the scanner/server/database/feed are not bundled.
- Web Security: launches the separately installed OWASP ZAP desktop GUI.
- Packet Analyzer: launches the separately installed Wireshark GUI.
- Web/API Inspector: launches the separately installed Burp Suite Community/Professional GUI.
- These third-party tools run on the Windows host, not in Windows Sandbox. Install them from their official sources and set the same authorized target allowlist inside each tool before testing.

SECURITY BOUNDARY — READ THIS
- NOVA itself is a normal Windows app, NOT a virtual machine. Only content explicitly opened in Windows Sandbox receives the disposable VM isolation. Core Data directories are validated against symlink/junction/reparse-point redirection, but this is not a full host sandbox.
- For suspicious files, select the file in NOVA Files and choose Inspect in Sandbox. NOVA copies only that file to a staging folder, maps it read-only, disables networking and clipboard, then opens the folder inside Windows Sandbox. Close Sandbox and confirm deletion to discard its VM state. The staged copy remains in Data/Security/Quarantine until you clear it in Security Center after Sandbox has closed. The original file is not changed.
- Isolated Chrome maps only the host Chrome application folder read-only, uses a fresh sandbox-only profile and has networking enabled for browsing. Networking can expose local-network services; clipboard and writable host folders are disabled. Downloads are discarded when Sandbox closes.
- Windows Sandbox requires Windows Pro, Enterprise or Education, enabled CPU virtualization and compatible hardware (at least 4 GB RAM, 2 CPU cores and 1 GB free disk; 8 GB RAM and 4 cores recommended). Windows Home does not include Windows Sandbox. Only one Sandbox instance can run at a time.
- CMD, normal host Chrome, UniKey, Wi-Fi and ordinary NOVA file operations still run on the Windows host. Do not run unknown programs or commands outside Windows Sandbox. No sandbox can promise protection against every future hypervisor/OS vulnerability.
- Keep Windows and Microsoft Defender up to date.

IMPORTANT
- NOVA is a desktop environment running inside Windows, not a bootable operating system.
- NOVA app windows are managed inside the main NOVA window; Chrome opens as a separate Windows application.
- NOVA Trash is separate from the Windows Recycle Bin.
- Wi-Fi passwords are briefly stored in a protected TEMP XML file to import a Windows WLAN profile.
- A compatible Windows OpenGL graphics driver is required. No automatic USB autorun is installed.

SECURITY BOUNDARIES
- NOVA is a normal Windows application, not a virtual machine or antivirus.
- Only "Open isolated" Chrome and "Inspect in Sandbox" use Windows Sandbox.
- Downloads from host Chrome, CMD or other host applications are NOT automatically isolated.
- Wi-Fi profiles are imported into the real Windows user profile; the temporary XML is DACL-restricted but briefly exists in host TEMP.
- UniKey runs as a normal Windows host process to provide system-wide Vietnamese input.
- Windows Sandbox requires a supported Pro/Enterprise/Education edition and virtualization; it is not available on Windows Home.
