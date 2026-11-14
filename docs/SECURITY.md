# Security notes

- The terminal runs real Windows CMD commands with the current signed-in user's permissions. It is intentionally not sandboxed. Do not paste unknown commands.
- Commands time out after 30 seconds and NOVA requests process-tree termination. Descendant processes retaining pipe handles may still affect shutdown; this needs runtime testing on Windows. Output is capped per stream. Interactive stdin is not supported.
- Built-in Files is scoped to NOVA user data and rejects absolute paths, parent traversal, and entries whose canonical target resolves outside the user-data root (including common symlink/junction cases).
- The editor accepts only relative paths under NOVA's user data and limits files it opens to 2 MiB.
- Wi-Fi profile deletion is allowed only for SSIDs recorded in NOVA's managed-profile manifest.
- Wi-Fi credentials are never written into NOVA's own settings, app terminal history or logs. The current `netsh` import method briefly writes the WLAN XML profile in the current user's TEMP directory; NOVA attempts deletion immediately after import and uses a scope guard on normal exit paths. It does not set a custom Windows DACL; abrupt termination or power loss can leave plaintext XML in TEMP. Replace this with direct WLAN API calls and secure memory handling before making a strong in-memory-only credential promise.
- Windows may retain the imported WLAN profile after NOVA exits. This is managed by Windows and can be removed with the NOVA `password delete` command while in that SSID's setup mode.
- No admin elevation is requested.
- No auto-start registration, background service or autorun behavior is installed.
