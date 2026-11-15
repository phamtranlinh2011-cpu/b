# NOVA 0.4.0 research references

The desktop/UX direction, motion policy, app-window behavior, file management, host Chrome integration and Windows process design were reviewed against the following primary documentation and platform references. The source package does not copy design assets or browser binaries from these sources.

## Windows and Fluent UX foundations

1. [Microsoft Learn — Design guidelines for Windows apps](https://learn.microsoft.com/en-us/windows/apps/design/devices/designing-for-mr) — color, commanding, elevation, geometry, iconography, layout, materials, motion, navigation, typography and usability.
2. [Microsoft Learn — Motion in Windows](https://learn.microsoft.com/en-us/windows/apps/design/motion/) — responsive, consistent and context-appropriate motion.
3. [Microsoft Learn — Windows app development best practices](https://learn.microsoft.com/en-us/windows/apps/get-started/best-practices) — layout, interaction, visual style, window behavior and shell integration.
4. [Microsoft Learn — User interface overview](https://learn.microsoft.com/en-us/windows/apps/develop/user-interface) — controls, layout and navigation patterns.
5. [Microsoft Learn — Fluent Design overview](https://learn.microsoft.com/en-us/windows/apps/design/signature-experiences/design-principles) — system-level visual principles.
6. [Microsoft Learn — Color](https://learn.microsoft.com/en-us/windows/apps/design/signature-experiences/color) — purposeful color hierarchy.
7. [Microsoft Learn — Typography](https://learn.microsoft.com/en-us/windows/apps/design/signature-experiences/typography) — readability and hierarchy.
8. [Microsoft Learn — Layout](https://learn.microsoft.com/en-us/windows/apps/design/layout/) — adaptive layout and spacing.
9. [Microsoft Learn — Navigation design](https://learn.microsoft.com/en-us/windows/apps/design/controls/navigationview) — predictable app navigation.
10. [Microsoft Learn — App windowing](https://learn.microsoft.com/en-us/windows/apps/develop/ui-input/windowing-overview) — Windows window behavior and lifecycle.
11. [Microsoft Learn — Keyboard interactions](https://learn.microsoft.com/en-us/windows/apps/design/input/keyboard-interactions) — keyboard access and predictable navigation.
12. [Microsoft Learn — Accessibility](https://learn.microsoft.com/en-us/windows/apps/design/accessibility/accessibility) — accessible controls and input.
13. [Microsoft Learn — High contrast themes](https://learn.microsoft.com/en-us/windows/apps/design/accessibility/high-contrast-themes) — system contrast and legibility.
14. [Microsoft Learn — Mica material](https://learn.microsoft.com/en-us/windows/apps/design/style/mica) — material and surface guidance.
15. [Microsoft Learn — Acrylic material](https://learn.microsoft.com/en-us/windows/apps/design/style/acrylic) — translucent surface considerations.

## egui / eframe framework API

16. [eframe 0.37 — crate documentation](https://docs.rs/eframe/0.37.0/eframe/) — native app runner and renderer configuration.
17. [eframe 0.37 — NativeOptions](https://docs.rs/eframe/0.37.0/eframe/struct.NativeOptions.html) — native viewport and Glow renderer setup.
18. [egui 0.37 — Context](https://docs.rs/egui/0.37.0/egui/struct.Context.html) — animation state and repaint scheduling.
19. [egui 0.37 — Window](https://docs.rs/egui/0.37.0/egui/containers/struct.Window.html) — floating in-shell windows and transitions.
20. [egui 0.37 — Area](https://docs.rs/egui/0.37.0/egui/containers/struct.Area.html) — anchored launcher surface and fade.
21. [egui 0.37 — Ui](https://docs.rs/egui/0.37.0/egui/struct.Ui.html) — opacity, layout and controls.
22. [egui 0.37 — Visuals](https://docs.rs/egui/0.37.0/egui/style/struct.Visuals.html) — theme-level visuals.
23. [egui 0.37 — Style](https://docs.rs/egui/0.37.0/egui/style/struct.Style.html) — spacing and widget style.
24. [egui 0.37 — Frame](https://docs.rs/egui/0.37.0/egui/containers/struct.Frame.html) — cards and surface frames.
25. [egui 0.37 — AccessKit integration](https://docs.rs/egui/0.37.0/egui/) — accessible UI semantics supported by the framework.
26. [egui — Viewports](https://docs.rs/egui/0.37.0/egui/viewport/index.html) — native extra viewport model considered for a future release.
27. [egui — Memory](https://docs.rs/egui/0.37.0/egui/struct.Memory.html) — per-context state and window memory.
28. [egui — ComboBox](https://docs.rs/egui/0.37.0/egui/containers/struct.ComboBox.html) — stable ID for accent selection control.
29. [egui — Grid](https://docs.rs/egui/0.37.0/egui/containers/struct.Grid.html) — app tile layout.
30. [egui — ScrollArea](https://docs.rs/egui/0.37.0/egui/containers/struct.ScrollArea.html) — file lists and Trash list scrolling.

## Windows file management and process lifecycle

31. [Microsoft Learn — Job Objects](https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects) — group process lifecycle and termination.
32. [Microsoft Learn — AssignProcessToJobObject](https://learn.microsoft.com/en-us/windows/win32/api/jobapi2/nf-jobapi2-assignprocesstojobobject) — process assignment.
33. [Microsoft Learn — TerminateJobObject](https://learn.microsoft.com/en-us/windows/win32/api/jobapi2/nf-jobapi2-terminatejobobject) — terminate processes in a job.
34. [Microsoft Learn — Process and thread functions](https://learn.microsoft.com/en-us/windows/win32/procthread/process-and-thread-functions) — thread control and Job Object APIs.
35. [Microsoft Learn — CreateProcess flags](https://learn.microsoft.com/en-us/windows/win32/procthread/process-creation-flags) — suspended process creation.
36. [Microsoft Learn — ResumeThread](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-resumethread) — resume a suspended initial thread.
37. [Microsoft Learn — CreateFileW](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-createfilew) — creation with explicit security attributes.
38. [Microsoft Learn — ConvertStringSecurityDescriptorToSecurityDescriptorW](https://learn.microsoft.com/en-us/windows/win32/api/sddl/nf-sddl-convertstringsecuritydescriptortosecuritydescriptorw) — protected DACL creation.
39. [Microsoft Learn — ShellExecuteW](https://learn.microsoft.com/en-us/windows/win32/api/shellapi/nf-shellapi-shellexecutew) — opening the official Chrome download page.
40. [Rust standard library — `std::fs::rename`](https://doc.rust-lang.org/std/fs/fn.rename.html) — atomic same-volume file rename and Windows behavior.
41. [Microsoft Learn — File naming conventions](https://learn.microsoft.com/en-us/windows/win32/fileio/naming-a-file) — reserved device names and filename constraints.
42. [Microsoft Learn — File attributes](https://learn.microsoft.com/en-us/windows/win32/fileio/file-attribute-constants) — file and reparse-point attributes.
43. [Microsoft Learn — IFileOperation::SetOperationFlags](https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nf-shobjidl_core-ifileoperation-setoperationflags) — Shell file-operation flags, including recycle behavior.
44. [Microsoft Learn — SHFileOperationW](https://learn.microsoft.com/en-us/windows/win32/api/shellapi/nf-shellapi-shfileoperationw) — legacy shell file operations and `FOF_ALLOWUNDO`.

## Chrome and WLAN behavior

45. [Google Chrome — Official download](https://www.google.com/chrome/) — host installation download page.
46. [Chrome for Testing documentation](https://developer.chrome.com/blog/chrome-for-testing/) — why automation-focused Chrome for Testing is not substituted for the consumer browser.
47. [Chrome for Testing availability](https://googlechromelabs.github.io/chrome-for-testing/) — distribution distinction from consumer Chrome.
48. [Chrome Enterprise — Download](https://chromeenterprise.google/browser/download/) — managed Windows deployment reference.
49. [Chromium — User data directory](https://chromium.googlesource.com/chromium/src/+/main/docs/user_data_dir.md) — browser profile separation and user-data location.
50. [Microsoft Learn — WLAN profile schema](https://learn.microsoft.com/en-us/windows/win32/nativewifi/wlan-profileschema) — Windows WLAN XML structure.
51. [Microsoft Learn — WlanSetProfile](https://learn.microsoft.com/en-us/windows/win32/api/wlanapi/nf-wlanapi-wlansetprofile) — native WLAN profile API considered as a future way to remove temporary XML.
52. [Microsoft Learn — WLAN profile shared key](https://learn.microsoft.com/en-us/windows/win32/nativewifi/wlan-profileschema-sharedkey-security-element) — shared-key profile element reference.
53. [Microsoft Learn — WLAN authentication/encryption](https://learn.microsoft.com/en-us/windows/win32/nativewifi/wlan-profileschema-authencryption-security-element) — WLAN security element reference.

## Design decisions derived from the research

- Use one consistent navigation model: launcher, rail, desktop tiles and taskbar/dock.
- Keep motion short and tied to user actions; expose a reduced-motion preference. This follows Microsoft's guidance that motion should be responsive, consistent and purposeful rather than decorative for its own sake.
- Use egui's built-in window/area animation primitives rather than adding a large animation runtime.
- Keep built-in app windows inside the root NOVA viewport for the lightweight 0.4.0 architecture; revisit independent native viewports only after window lifecycle and multi-monitor testing.
- Implement a separate portable NOVA Trash instead of pretending this is the Windows Recycle Bin. Windows Shell recycle operations were reviewed as a distinct integration path; the current Trash scope is explicit and stored inside the USB Data tree.
- Launch the existing host Chrome installation rather than bundling a browser binary or a Chrome-for-Testing build. The user's installed Chrome controls updates and profile data.
- Keep the portable app independent from Node/WebView; Chrome is an optional external host app.

Some references above are broad platform guidance or API references rather than claims that NOVA implements every Windows shell feature. For example, 0.4.0 does not yet use Mica/Acrylic, the Windows Shell Recycle Bin, separate native per-app viewports, or a bundled embedded browser.
