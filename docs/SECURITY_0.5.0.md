# NOVA 0.5.0 security boundary and threat model

## 1. What NOVA is — and is not

NOVA is a normal Win32 desktop process running under the current Windows user. It is not a hypervisor, kernel, virtual machine, antivirus engine or security boundary. A process launched by NOVA without an explicit isolation route inherits host permissions. The current build has three important host-side paths:

- Terminal invokes `cmd.exe` as the logged-in user. Arbitrary commands may modify real host files, settings, processes or network state.
- UniKey is a separate signed companion process on the Windows host. It provides system-wide Vietnamese input and is not contained by Windows Sandbox.
- Normal Chrome, WLAN commands and ordinary Files/Editor writes operate on the host or on NOVA's configured data location. In portable mode the NOVA data location is the USB, but this is not a process sandbox.

The application must not describe these host-side operations as isolated. Startup creates the Data tree one component at a time and rejects symlinks, junctions and Windows reparse points in the core directories. Settings and Wi-Fi manifest files are also checked before reading/replacement; Trash and Sandbox Security roots revalidate their directories before use. Recursive Trash deletion and quarantine cleanup remove nested reparse points as links rather than traversing their targets. This does not prevent a hostile process running as the same Windows user from racing filesystem changes. At startup, NOVA creates its Data tree one directory component at a time and rejects symlinks/junctions/reparse points in the Data, Users, Settings, Trash and Security directory roots before using them. The optional UniKey downloader also refuses reparse-point Apps/UniKey destinations. This hardens portable-folder handling but does not defend against a malicious process running concurrently as the same Windows user and racing filesystem changes.

## 2. Isolated file inspection

For a selected file inside `Data/Users/Default`, NOVA:

1. Resolves the source under the NOVA user root and rejects paths outside that root.
2. Copies only the selected file into a unique `Data/Security/Quarantine/<session>/` folder. The source is not moved or modified.
3. Creates a Windows Sandbox `.wsb` file that maps only that staging folder as read-only.
4. Disables Sandbox networking, vGPU, audio/video input, printer redirection and clipboard redirection, and enables Protected Client.
5. Opens the mapped folder in Explorer. The sample is not auto-launched; the user must manually choose to inspect/run it inside Sandbox.

The guest cannot write back through the read-only mapped folder. Any modifications made in the guest's own virtual disk are discarded when the user closes Sandbox and confirms deletion. NOVA keeps the staged sample copy and `.wsb` configuration on the USB until the user closes all Sandbox windows and clears staged items in Security Center.

The staging copy is an inert file until opened by the user. Do not open the file from File Explorer on the host, or run it through Terminal.

## 3. Isolated Chrome

Before either launch route, NOVA verifies that `chrome.exe` has a valid Authenticode signature with the expected Google publisher name (`Google LLC` or legacy `Google Inc.`). NOVA maps only the installed Chrome application directory into Windows Sandbox as read-only. It launches Chrome with a new `--user-data-dir` inside the guest. Protected Client is enabled; vGPU, audio/video input, printer redirection and clipboard redirection are disabled. It does not map the user's normal Chrome profile. Cookies, extensions, cache and downloads belong to the disposable guest and are discarded when Sandbox closes.

Networking is enabled for this mode so websites load. Microsoft warns that Sandbox networking can expose untrusted applications to internal-network services. Clipboard, vGPU, audio/video input and printer redirection are disabled, and no writable host folder is mapped. For unknown executable files, use the offline file-inspection profile instead of browser mode.

Normal host Chrome is still available only after a warning/confirmation and the Google signature check. It is not isolated by NOVA; a browser vulnerability can still affect the real Windows host.

## 4. UniKey provenance and behavior

NOVA does not bundle an opaque UniKey executable in the source archive. The optional helper is embedded into `NOVA.exe` at compile time; the Download action does not execute a mutable PowerShell script stored on the USB. It downloads the x64 ZIP from the official `unikey.org` release, rejects unsafe archive entry paths and symbolic links, enforces compressed/uncompressed archive and executable size limits, extracts `UniKeyNT.exe`, checks Authenticode status and requires the exact simple publisher name `PHAM KIM LONG`, checks the copied executable again, and places it below `Apps/UniKey` beside NOVA. NOVA repeats signature validation before launch.

UniKey is a separate Windows process. It can receive system-wide input, and it is not sandboxed by NOVA. Closing NOVA attempts to stop only the UniKey child process that NOVA started. A valid code signature verifies publisher/integrity according to Windows trust policy; it does not prove software is free of vulnerabilities. The embedded PowerShell helper is a downloaded-companion workflow, not a sandbox: UniKey still runs on the Windows host.


## 5. Other host-side operations

- Wi-Fi setup invokes the real Windows `netsh.exe` and imports a WLAN profile for the current Windows user. NOVA writes the profile XML briefly to the host TEMP directory with a protected DACL granting access to the file owner, Local System and local Administrators, then closes and deletes it after the import. A crash can leave a stale file; NOVA removes only matching old generated files on next startup. A malicious process running as the same Windows user can still access owner-permitted files. The WLAN profile itself is stored by Windows and may persist after NOVA exits.
- Ordinary Files/Editor operations modify the NOVA Data tree, which is the USB in portable mode. They are not a sandbox for programs opened manually by the user.
- NOVA does not automatically intercept or sandbox every file downloaded by Windows, host Chrome, CMD or another host application. Only the explicit **Open isolated** browsing route and **Inspect in Sandbox** file route use Windows Sandbox.

## 6. Windows Sandbox requirements and limits

Windows Sandbox is available in supported Windows Pro, Enterprise and Education editions, but not Windows Home. It permits only one running Sandbox instance at a time. It requires an AMD64/ARM64-supported Windows build, virtualization enabled in firmware, and sufficient RAM, CPU and free disk. NOVA currently builds x64 MSVC and requires the optional Windows Sandbox feature to be enabled on the target PC.

NOVA checks for the Sandbox launcher, but presence of the executable is not proof that the edition, policy, virtualization or hypervisor configuration is valid. The actual launch may still fail. NOVA fails closed: if Sandbox cannot launch, it returns an error rather than opening the sample on the host.

A sandbox is a defense-in-depth boundary, not an absolute promise. Keep Windows, Defender and firmware updated. Do not treat a Sandbox as protection against every future hypervisor escape or operating-system vulnerability.

## 7. Reset and cleanup

- Closing Windows Sandbox and confirming deletion discards the guest's installed software, downloads, files, settings and process state.
- The host-side staging copy under `Data/Security/Quarantine` and generated `.wsb` config files persist until the user chooses **Clear staged copies** in Security Center. Close all Sandbox instances before clearing them.
- Clearing staging removes only the NOVA test copies and NOVA-generated `.wsb` files. It does not delete the original workspace file, installed Chrome, Windows profile, or any other host folder.
- NOVA does not silently remove source files from `Data/Users/Default`.

## 8. Operational checklist

- Use **Inspect in Sandbox** for untrusted files.
- Use **Open isolated** for untrusted websites; understand that its network is enabled and may reach local-network services.
- Do not use **Open on Windows host** for unknown websites/files.
- Do not run unknown commands in NOVA Terminal.
- Do not install downloaded software on the host just because it was downloaded to a NOVA folder.
- Keep Windows Defender and Windows updates enabled.
- After inspection, close all Sandbox sessions and clear staging if the sample is no longer needed.

## 9. Remaining non-goals

This version does not provide a custom VM for Windows Home, a kernel/hypervisor of its own, full host file-system virtualization, host network isolation for browser mode, protection against malicious host-side commands, or a guarantee that all downloads are scanned. It does not replace Defender, Windows Sandbox security updates or a dedicated analysis VM.
