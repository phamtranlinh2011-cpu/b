# NOVA Native v0.5.7 — Audit Phase 1

## Security and data-integrity changes

- `storage::canonical_user_path()` now checks each lexical path component before canonicalization and rejects symlinks, junctions, and other reparse points even when they resolve back inside the NOVA data root.
- Settings and the Wi-Fi manifest are read through bounded, no-follow file handles while their parent directory chain is held. Configuration reads are limited to 1 MiB.
- File preview/copy reads use a no-follow regular-file handle. On Windows the opened file denies concurrent write/delete sharing; the parent directory chain is also held while the operation runs.
- File copy uses an atomically reserved private staging directory. Publication, move, rename, Trash moves, restore, and Trash metadata publication use a no-replace move primitive on Windows (`MoveFileW`) and Linux (`renameat2(RENAME_NOREPLACE)`). This prevents a destination that appears after the preliminary name check from being overwritten.
- Recursive deletion now opens and validates each directory without following a final reparse point before enumerating it. Trash metadata reads are bounded and no-follow. A pre-existing metadata temp file is no longer deleted when NOVA fails to reserve that name.
- Wi-Fi profiles created by new NOVA sessions receive a per-attempt unique Windows profile name, stored in an optional manifest map. Older manifests remain readable and continue to use the historic `NOVA::<SSID>` name when deleting legacy profiles. Failed imports/connections attempt rollback, and rollback failures are reported instead of hidden. Existing NOVA-managed profiles must be removed before reconnecting with new credentials.
- Chrome, UniKey, and Security Center executables are held open with write/delete sharing denied while Authenticode verification and process creation occur. Parent directories are held against replacement during that interval.
- The Sound control launches `sndvol.exe` from the resolved Windows system directory rather than searching the current directory or `PATH`.

## Added regression coverage

Five tests were added for nested in-root symlink rejection, exact/legacy Wi-Fi manifest profile names, unique Wi-Fi profile names, late destination collision handling, and preserving a pre-existing Trash metadata temp file. The source currently contains 130 `#[test]` declarations.

## Validation status

- `Cargo.toml` parses successfully with Python `tomllib`.
- `.github/dependabot.yml` parses successfully as YAML.
- A lexical delimiter/string/comment scan passed for all nine changed Rust modules.
- No dependencies or package versions were changed.
- **A Rust build, Clippy run, unit-test run, PowerShell validation, and Windows runtime test were not possible in this environment:** `cargo`, `rustc`, `rustfmt`, `rustup`, and PowerShell are not installed. The Windows CI workflow must still compile and execute the full test suite before this source is considered build-verified.

## Remaining follow-up

- Locale-independent parsing of `netsh wlan show networks` output remains in Audit Phase 2.
- The Windows-specific handle/share-mode behavior and WLAN API profile enumeration require validation on the Windows CI runner and a real Windows WLAN adapter.
- The project source archive still does not include `Cargo.lock`; preserve the committed lockfile in the repository as required by `.github/workflows/rust.yml`.
