# NOVA Native 0.5.7 — Stability, Security and Release Pipeline

## Dependency and CI reliability
- Added a dedicated lockfile preparation/verification script. Developer builds can create a missing lockfile once; CI requires a committed lockfile and never commits or pushes dependency changes.
- CI now runs on pushes, pull requests and manual dispatch with read-only repository permissions. Signing credentials are passed only to a dedicated post-build signing step on a push to `main`; Cargo and dependency build scripts never receive them. Pull requests and manual runs are packaged unsigned.
- Pinned the checkout, Rust toolchain and upload-artifact actions to immutable commits.
- Added Clippy, locked metadata, Windows-target tests and release-build checks.
- Added Dependabot updates for Cargo dependencies and GitHub Actions.

## Windows integration and security
- Added a Windows junction/reparse-point integration test proving that redirected data folders are rejected and safe recursive deletion does not delete the junction target.
- Added a Windows Job Object test proving that timeout termination also terminates a spawned descendant process.
- Added a concise security policy describing the actual host/Sandbox boundaries and a private vulnerability-reporting path.

## Portable release and performance
- Build version is read from `Cargo.toml` rather than duplicated in the packaging script.
- Portable packages now include build provenance, executable SHA-256, Cargo.lock SHA-256, Authenticode status and a checksum manifest.
- Added optional post-build Authenticode signing using a PFX provided through environment secrets; no certificate or password is stored in source. Signing is isolated from Cargo so dependency build scripts never inherit the certificate secrets.
- Added `scripts/finalize-portable.ps1` to refresh build metadata, executable checksums, ZIP and sidecar after signing.
- Removed the unnecessary `Cargo.lock.build-reference` copy from the runtime USB package.
- Added `scripts/measure-performance.ps1` to sample NOVA CPU, working set, private memory, handles and thread count to CSV on Windows.

## Validation caveats
- The source archive and static configuration can be checked in this environment, but Rust compilation, Clippy, unit tests, Windows integration tests, EXE signing and runtime performance sampling require a Windows machine with Rust 1.95.0 and the MSVC build tools.
- `Cargo.lock` is intentionally not fabricated: the current editing environment has no Cargo/Rust toolchain or network access to resolve the full transitive dependency graph. Run `scripts/prepare-lockfile.ps1` on Windows and commit the generated file before expecting CI to pass.

## Follow-up source audit

- Confirmed the pinned `dtolnay/rust-toolchain` revision hard-codes Rust 1.95.0 in its action metadata and does not declare a `toolchain` input. The workflow intentionally supplies only its supported `targets` and `components` inputs.
- Rechecked Cargo/TOML and workflow YAML, source delimiters, declared test count and repository file references. These are static checks, not a substitute for `cargo check`, Clippy, or Windows runtime tests.

## Compile fixes from Windows CI feedback

- Replaced deprecated `Ui::id()` with `Ui::scope_id()` and deprecated `egui::Id::new` calls with `egui::Id::unique` while preserving the same stable salts.
- Explicitly typed the shortcut icon font size as `f32` so `to_bits()` has an unambiguous receiver type.
- Converted shortcut icon text from `&str` to the owned `String` required by egui 0.37's `Painter::layout_no_wrap`.
- Compared the quick-access app name directly as `&str`, avoiding the unstable `str::as_str()` API.
- Added `pinned_apps` to the storage test's `Settings` initializer and asserted that pinned apps survive save/load.


## Additional desktop-shortcut compile fix

- Corrected the iterator predicate for desktop shortcut lookup: `.iter()` passes `&&str` to `find`, so the comparison now dereferences the name to `&str` (`app.name == *name`).
- Promoted the desktop shortcut list to a shared catalogue constant and added a test ensuring every desktop shortcut maps to a unique app-catalog entry.

- Fixed Windows junction deletion: classify directory-shaped reparse points using `FILE_ATTRIBUTE_DIRECTORY` and remove them with `RemoveDirectoryW` via `std::fs::remove_dir`, rather than relying on `Metadata::is_dir()` and accidentally calling `RemoveFileW` (which returns `ERROR_ACCESS_DENIED`).


## Desktop app integration update

- Windows packaging now downloads official UniKey 4.6 RC2 x64 from unikey.org, validates archive contents and the `PHAM KIM LONG` Authenticode signer, bundles the executable under `Apps/UniKey`, and records its SHA-256 and signer in build metadata. Opening the UniKey workspace starts the bundled process; the in-app self-download action was removed.
- Replaced the basic two-number calculator with a ClassWiz-inspired scientific calculator: expression parser, angle modes, scientific functions, factorial/combinatorics, normal/binomial/Poisson distributions, 1/2-variable statistics and regression, linear/quadratic solver, quadratic inequalities, numerical derivative/integration, function tables, Base-N, 2×2/3×3 matrix operations, vector operations and ratios. This is not yet full parity with every fx-991CW application.
- Google Chrome now opens the actual installed Chrome executable in its native Windows window, with the existing Chrome profile, extensions and settings. The old NOVA fake-browser panel was removed. Chrome is a host application and is not sandboxed by NOVA.


## Desktop interaction and visual refresh

- Reworked the NOVA taskbar into a Windows 11-inspired floating bar: Start/Search and app shortcuts are centered, with a balanced NOVA rail on the left and Network, Monitor, local Windows clock/date and Quick Settings on the right.
- Added animated hover feedback and active-app markers to taskbar shortcuts; Start and Quick Settings now use eased slide/fade transitions.
- Added interpolated movement/resize transitions when snapping NOVA workspaces to the left, right or full desktop. All built-in app windows retain fade-in/fade-out behavior.
- Applied a shared modern visual language across app windows and controls: refined dark/light surfaces, rounded corners, subtle borders, stronger shadows, consistent spacing and button states.
- Added Quick Settings controls for Dark/Light theme and Reduce Motion, plus quick links to Network, Settings, Monitor, Files and Terminal.
- The taskbar is an in-app NOVA surface; it does not replace the Windows host taskbar or control native host volume/battery.


## UI audit and refinement

- Taskbar now uses the all configured Start pins and appends open, unpinned NOVA workspaces; the centered app strip scrolls horizontally when it overflows. Balanced side rails keep the dock group centered, while the Search control and secondary tray item collapse on narrower layouts.
- Moved the floating taskbar, Start menu and Quick Settings into a foreground shell layer so large app windows cannot obscure them.
- Snap layouts reserve a safe area above the floating taskbar, animate from the window's actual bounds and preserve the dock's visibility.
- Security Center summary/tool cards reflow from three/two columns to one column according to available width; File Manager command rows wrap instead of overflowing. Terminal output keeps a sensible minimum scroll region in short windows.
- Fixed destructive confirmation dialogs so the title-bar close button, Cancel and Escape dismiss them reliably. The same dialogs now respect Reduce Motion on both opening and closing.
- Refined light/dark surface contrast, window borders and popup shadows, and made desktop shortcut hover feedback visible in both themes.
- Kept egui 0.37's `Id::unique` for stable shell overlay identities; `Id::new` is deprecated in this dependency version.


## Follow-up responsive UI audit

- Fixed taskbar rail overflow by deriving equal left/right rail widths from the actual available viewport; the center strip retains a bounded width and scrolls instead of pushing the dock beyond its frame.
- Made the Search control, tray buttons and NOVA label collapse at compact widths while preserving a centered Start/app group.
- Sized Quick Settings to the viewport and calculated its two primary control widths from the available inner width; quick actions wrap when space is limited.
- Reworked Start Menu dimensions and All apps grid columns around the actual viewport and content width, preventing fixed 300 px minimums and fixed two-column cards from clipping narrow views.
- Removed oversized snap constraints that could force panes wider than half the desktop after the animation ended. Terminal, Files, Settings and Calculator retain a smaller usable minimum so split panes stay in place.
- Settings Appearance now switches to a single-column layout in narrow panes. Editor toolbars/footers and Terminal, Security Center, Trash and Monitor action/KPI rows wrap instead of overflowing.
- Reused the selected theme accent for the NOVA taskbar brand and Start control, and added responsive layout regression tests for taskbar rails and Start Menu sizing.
- Kept the established egui 0.37 `Id::unique` pattern to avoid reintroducing the API deprecation already addressed in the Windows CI feedback.


## UI defect fixes and responsive polish

- Added a dedicated vertical scroll region to the Start Menu's Pinned view as well as All apps, preventing lower pinned rows and Recommended shortcuts from being clipped on short desktop windows.
- Clamped Start Menu inner width to a positive value even for pathological tiny viewports, and made Quick Access switch between one, two and three columns using its measured inner width.
- Reworked File Manager List rows with explicit bounded name/size columns. Long names are shortened only for display, Unicode characters are not split into bytes, and the full filename remains unchanged on disk.
- Changed remaining destructive-confirmation, UniKey, Security Center, Calculator and desktop status rows to wrap where practical; Security Center tool cards can now reflow into narrow snapped panes.
- Added regression tests for Unicode-safe file-name shortening and Start Menu behavior at very small viewport sizes. The source declares 88 Rust tests; execution still requires the Rust 1.95/Windows build environment.


## Windows CI follow-up compile fix

- Explicitly typed the responsive Start Menu Quick Access column count as `usize`. This resolves E0689 when calling `saturating_sub` before passing the count to `egui::Grid::num_columns`.
