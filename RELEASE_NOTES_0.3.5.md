# NOVA Native 0.3.5 — Responsive terminal and process security

## Fixed

- Moved Windows CMD execution and Wi-Fi scan/connect/delete operations to a background worker so a slow command or WLAN operation does not block the egui UI thread.
- Added a single-job-at-a-time guard and a visible running state in the terminal.
- Added a Windows Job Object with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`; child processes and their normal descendants are terminated on timeout, reader-drain failure, or worker cleanup.
- Routed both CMD and `netsh.exe` through one bounded process runner with per-stream capture limits, continuous pipe draining, and timeouts.
- Created the temporary WLAN XML file with a protected DACL granting access to the file owner, SYSTEM and local Administrators. NOVA closes its writer before `netsh` imports the file to avoid share-mode conflicts, then deletes it immediately after import. A startup cleanup removes matching NOVA temp XML files older than 10 minutes if an earlier run crashed.
- Create child processes suspended, assign them to the Job Object, then resume the initial thread. This closes the race where a command could spawn descendants before assignment.
- Resolve system executables with `GetSystemDirectoryW` instead of trusting the `SystemRoot` environment variable.
- Added Windows smoke tests for process capture, timeout termination and output larger than the capture cap, plus cross-platform tests for full pipe draining and password non-echoing/task scheduling.
- Pinned the Rust toolchain to 1.95.0. The Windows workflow generates and commits `Cargo.lock` on the first run, then uses `--locked` for metadata, tests and release build.
- Updated the local PowerShell build script to use the lockfile and current release name.

## Security and operational notes

- The temporary WLAN XML still contains the credential in plaintext while it exists; its DACL is restricted, it is deleted immediately after the `netsh` import exits, and stale matching files older than 10 minutes are cleaned at startup. An abrupt crash can leave a protected orphan until the next startup. Local SYSTEM and Administrators retain access by design. The Windows CI test checks the writer-close/import handoff; actual `netsh` import must still be verified on Windows hardware.
- Windows Job Objects cover the command process and descendants that remain in the job. The child is assigned while suspended, before it can execute; NOVA does not enable breakaway. The runner fails closed if initial job assignment or thread resume fails.
- Commands still execute with the current Windows user's permissions. NOVA is not a security sandbox.
- Full Windows CI execution and real adapter/router tests are still required before treating the release as production-verified.
