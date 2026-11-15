# NOVA Native 0.3.3 — CI test corrections

## Fixed
- Fixed `safe_user_path` trimming the whole input before validation. Trailing spaces are now preserved until Windows filename validation rejects them; empty paths are rejected explicitly.
- Corrected the WPA2 password test: a 64-character hexadecimal value is a valid raw PSK; a 64-character non-hex value is rejected.
- Removed unused `AppPaths::app_dir` and `AppPaths::data_dir` fields to eliminate dead-code warnings; the paths remain local during startup discovery and are used to derive the active data locations.

## Validation
- The 0.3.2 Windows CI run compiled the crate and executed 16 tests; 14 passed and 2 failed due to the path-trimming bug and an incorrect expectation in the WPA2 test.
- This source package includes fixes for both failures. Rust/Cargo are unavailable in the packaging environment, so a fresh Windows `cargo test` and release build are still required to certify this release.
