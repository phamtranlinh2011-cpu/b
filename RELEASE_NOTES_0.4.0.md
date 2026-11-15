# NOVA Native 0.4.0 — Desktop UX and app expansion

## Desktop shell and UI
- Added a searchable Launcher with app descriptions and eight app entries.
- Added navigation rail, floating app windows within the main NOVA viewport and a dock/taskbar that opens or hides active apps.
- Refreshed surfaces, spacing, corner radius, shadow, accent color, light/dark palette and typography hierarchy.
- Added tile hover feedback, launcher fade and window fade transitions.
- Added Reduce visual effects preference.

## App updates
- Files: select with a single click, open with a double click, refresh, create folders and move selected files/folders into NOVA Trash.
- Editor: New, Save, character/word counts and 2 MiB limit.
- Calculator: Clear and Swap controls, with small-result and non-finite formatting behavior.
- System Monitor: updates only while visible.
- Terminal: background worker remains responsive during command and WLAN tasks, with bounded output/history and Job Object process-tree cleanup.

## NOVA Trash
- Added `Data/Trash/Items` and `Data/Trash/Meta` portable storage.
- Added restore, collision-safe restored filenames, permanent delete and Empty Trash confirmation.
- This is NOVA's own Trash, not the Windows Recycle Bin. It handles items moved from NOVA Files only.

## Google Chrome
- Added a Chrome launcher panel, common-path detection, URL scheme validation, new-window launch and a detect-again action.
- If Chrome is not installed, the panel opens Google's official download page.
- Chrome is not bundled or installed by NOVA. Browser content and profile remain in the Windows host Chrome app.

## Build and documentation
- Workflow outputs `NOVA-OS-Native-v0.4.0-USB` and packages NOVA Trash data directories.
- Updated portable instructions, architecture, security notes and roadmap.
- Added `docs/RESEARCH_REFERENCES_0.4.0.md` listing the primary UI, egui, Windows, Chrome, process and file-management sources consulted.

## Validation boundaries
- Source-level and archive checks are performed before packaging.
- This authoring environment does not include Rust/Cargo, so no claim is made that the 36 source unit tests or MSVC release build passed here. GitHub Actions on the exact v0.4.0 commit must execute them.
- Actual Windows runtime checks remain necessary for WLAN, Chrome discovery, USB persistence, high-DPI, graphics drivers and Job Object process handling.
