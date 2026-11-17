use crate::storage::AppPaths;
use std::ffi::OsString;
use std::fs::{self, File};
use std::path::Path;
#[cfg(not(windows))]
use std::fs::OpenOptions;
use std::io::{self, Write};
#[cfg(windows)]
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const PROFILE_PREFIX: &str = "NOVA::";
static PROFILE_SEQUENCE: AtomicU64 = AtomicU64::new(1);

fn new_profile_name(ssid: &str) -> io::Result<String> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_nanos();
    let sequence = PROFILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    // A per-attempt suffix avoids clobbering a user's existing profile named
    // `NOVA::<SSID>` or another NOVA session's profile during a name race.
    Ok(format!("{PROFILE_PREFIX}{ssid}::{}-{timestamp:x}-{sequence:x}", std::process::id()))
}

#[cfg(windows)]
#[repr(C)]
#[derive(Clone, Copy)]
struct WlanGuid {
    data1: u32,
    data2: u16,
    data3: u16,
    data4: [u8; 8],
}

#[cfg(windows)]
#[repr(C)]
struct WlanInterfaceInfo {
    interface_guid: WlanGuid,
    description: [u16; 256],
    state: i32,
}

#[cfg(windows)]
#[repr(C)]
struct WlanInterfaceInfoList {
    number_of_items: u32,
    index: u32,
    interface_info: [WlanInterfaceInfo; 1],
}

#[cfg(windows)]
#[repr(C)]
struct WlanProfileInfo {
    profile_name: [u16; 256],
    flags: u32,
}

#[cfg(windows)]
#[repr(C)]
struct WlanProfileInfoList {
    number_of_items: u32,
    index: u32,
    profile_info: [WlanProfileInfo; 1],
}

#[cfg(windows)]
#[repr(C)]
struct Dot11Ssid {
    length: u32,
    bytes: [u8; 32],
}

#[cfg(windows)]
#[repr(C)]
struct WlanAvailableNetwork {
    profile_name: [u16; 256],
    ssid: Dot11Ssid,
    bss_type: i32,
    number_of_bssids: u32,
    connectable: i32,
    not_connectable_reason: u32,
    number_of_phy_types: u32,
    phy_types: [i32; 8],
    more_phy_types: i32,
    signal_quality: u32,
    security_enabled: i32,
    default_auth_algorithm: i32,
    default_cipher_algorithm: i32,
    flags: u32,
    reserved: u32,
}

#[cfg(windows)]
#[repr(C)]
struct WlanAvailableNetworkList {
    number_of_items: u32,
    index: u32,
    networks: [WlanAvailableNetwork; 1],
}

#[cfg(windows)]
#[repr(C)]
struct WlanAssociationAttributes {
    ssid: Dot11Ssid,
    bss_type: i32,
    bssid: [u8; 6],
    phy_type: i32,
    phy_index: u32,
    signal_quality: u32,
    receive_rate: u32,
    transmit_rate: u32,
}

#[cfg(windows)]
#[repr(C)]
struct WlanSecurityAttributes {
    security_enabled: i32,
    one_x_enabled: i32,
    authentication_algorithm: i32,
    cipher_algorithm: i32,
}

#[cfg(windows)]
#[repr(C)]
struct WlanConnectionAttributes {
    state: i32,
    connection_mode: i32,
    profile_name: [u16; 256],
    association: WlanAssociationAttributes,
    security: WlanSecurityAttributes,
}

#[cfg(windows)]
const _: () = {
    assert!(std::mem::size_of::<WlanAssociationAttributes>() == 68);
    assert!(std::mem::size_of::<WlanSecurityAttributes>() == 16);
    assert!(std::mem::size_of::<WlanConnectionAttributes>() == 604);
    assert!(std::mem::offset_of!(WlanConnectionAttributes, association) == 520);
    assert!(std::mem::offset_of!(WlanConnectionAttributes, security) == 588);
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WifiConnectionInfo {
    pub ssid: String,
    pub signal_quality: u32,
}

fn matches_expected_ssid(actual: &str, expected: &str) -> bool {
    // SSIDs are byte identifiers; case changes represent a different value.
    actual.as_bytes() == expected.as_bytes()
}

// Keep the hand-written FFI layout pinned to wlanapi.h. A size/offset drift
// would make the variable-length list unsafe to iterate on Windows x64/x86.
#[cfg(windows)]
const _: () = {
    assert!(std::mem::size_of::<WlanInterfaceInfo>() == 532);
    assert!(std::mem::offset_of!(WlanInterfaceInfoList, interface_info) == 8);
    assert!(std::mem::size_of::<WlanAvailableNetwork>() == 628);
    assert!(std::mem::offset_of!(WlanAvailableNetwork, ssid) == 512);
    assert!(std::mem::offset_of!(WlanAvailableNetwork, signal_quality) == 604);
    assert!(std::mem::offset_of!(WlanAvailableNetworkList, networks) == 8);
};

/// WLAN SSIDs are byte strings, not guaranteed UTF-8. NOVA's current connect
/// and profile interface accepts UTF-8 strings, so reject malformed values
/// instead of lossy-decoding them into a different SSID.
fn decode_ssid(bytes: &[u8]) -> Option<String> {
    if bytes.is_empty() || bytes.len() > 32 {
        return None;
    }
    let value = std::str::from_utf8(bytes).ok()?;
    if value.is_empty() || value.chars().any(char::is_control) {
        return None;
    }
    Some(value.to_owned())
}

#[cfg(windows)]
fn wlan_api_error(code: u32, operation: &str) -> io::Error {
    const ERROR_ACCESS_DENIED: u32 = 5;
    if code == ERROR_ACCESS_DENIED && matches!(operation, "WlanScan" | "WlanGetAvailableNetworkList") {
        io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "Location access is required to scan nearby Wi-Fi networks. In Windows Settings > Privacy & security > Location, enable Location services and 'Let desktop apps access your location', then retry. (WLAN API: {operation})"
            ),
        )
    } else {
        io::Error::from_raw_os_error(code as i32)
    }
}

#[cfg(windows)]
#[link(name = "wlanapi")]
unsafe extern "system" {
    fn WlanOpenHandle(version: u32, reserved: *mut std::ffi::c_void, negotiated_version: *mut u32, client_handle: *mut *mut std::ffi::c_void) -> u32;
    fn WlanCloseHandle(client_handle: *mut std::ffi::c_void, reserved: *mut std::ffi::c_void) -> u32;
    fn WlanEnumInterfaces(client_handle: *mut std::ffi::c_void, reserved: *mut std::ffi::c_void, interface_list: *mut *mut WlanInterfaceInfoList) -> u32;
    fn WlanGetProfileList(client_handle: *mut std::ffi::c_void, interface_guid: *const WlanGuid, reserved: *mut std::ffi::c_void, profile_list: *mut *mut WlanProfileInfoList) -> u32;
    fn WlanScan(client_handle: *mut std::ffi::c_void, interface_guid: *const WlanGuid, ssid: *const Dot11Ssid, ie_data: *const std::ffi::c_void, reserved: *mut std::ffi::c_void) -> u32;
    fn WlanGetAvailableNetworkList(client_handle: *mut std::ffi::c_void, interface_guid: *const WlanGuid, flags: u32, reserved: *mut std::ffi::c_void, network_list: *mut *mut WlanAvailableNetworkList) -> u32;
    fn WlanQueryInterface(client_handle: *mut std::ffi::c_void, interface_guid: *const WlanGuid, opcode: i32, reserved: *mut std::ffi::c_void, data_size: *mut u32, data: *mut *mut std::ffi::c_void, value_type: *mut i32) -> u32;
    fn WlanFreeMemory(memory: *mut std::ffi::c_void);
}

#[cfg(windows)]
#[repr(C)]
struct SecurityAttributes {
    length: u32,
    security_descriptor: *mut std::ffi::c_void,
    inherit_handle: i32,
}

#[cfg(windows)]
#[link(name = "advapi32")]
unsafe extern "system" {
    fn ConvertStringSecurityDescriptorToSecurityDescriptorW(
        text: *const u16,
        revision: u32,
        descriptor: *mut *mut std::ffi::c_void,
        size: *mut u32,
    ) -> i32;
}

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateFileW(
        name: *const u16,
        access: u32,
        share: u32,
        attributes: *mut SecurityAttributes,
        disposition: u32,
        flags: u32,
        template: *mut std::ffi::c_void,
    ) -> *mut std::ffi::c_void;
    fn LocalFree(memory: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
}


pub fn validate_ssid(ssid: &str) -> io::Result<()> {
    let bytes = ssid.as_bytes();
    if bytes.is_empty() || bytes.len() > 32 || ssid.chars().any(char::is_control) || ssid.contains('"') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "SSID must be 1–32 UTF-8 bytes and contain no control characters.",
        ));
    }
    Ok(())
}

pub fn scan_networks() -> io::Result<Vec<String>> {
    #[cfg(windows)]
    {
        scan_networks_windows()
    }
    #[cfg(not(windows))]
    {
        Err(io::Error::new(io::ErrorKind::Unsupported, "Wi-Fi discovery is only available in the Windows build of NOVA."))
    }
}

#[cfg(windows)]
fn scan_networks_windows() -> io::Result<Vec<String>> {
    struct ClientHandle(*mut std::ffi::c_void);
    impl Drop for ClientHandle {
        fn drop(&mut self) {
            unsafe { WlanCloseHandle(self.0, std::ptr::null_mut()); }
        }
    }
    struct WlanMemory(*mut std::ffi::c_void);
    impl Drop for WlanMemory {
        fn drop(&mut self) {
            if !self.0.is_null() { unsafe { WlanFreeMemory(self.0); } }
        }
    }

    let mut negotiated_version = 0u32;
    let mut client = std::ptr::null_mut();
    let status = unsafe { WlanOpenHandle(2, std::ptr::null_mut(), &mut negotiated_version, &mut client) };
    if status != 0 {
        return Err(wlan_api_error(status, "WlanOpenHandle"));
    }
    let _client_guard = ClientHandle(client);

    let mut raw_interfaces = std::ptr::null_mut();
    let status = unsafe { WlanEnumInterfaces(client, std::ptr::null_mut(), &mut raw_interfaces) };
    if status != 0 {
        if !raw_interfaces.is_null() { unsafe { WlanFreeMemory(raw_interfaces.cast()); } }
        return Err(wlan_api_error(status, "WlanEnumInterfaces"));
    }
    if raw_interfaces.is_null() {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "Windows WLAN returned a null interface list."));
    }
    let _interfaces_guard = WlanMemory(raw_interfaces.cast());
    let interface_list = unsafe { &*raw_interfaces };
    let interface_count = interface_list.number_of_items as usize;
    if interface_count == 0 { return Ok(Vec::new()); }
    let interface_ptr = std::ptr::addr_of!(interface_list.interface_info).cast::<WlanInterfaceInfo>();

    let mut scan_errors = Vec::new();
    let mut scan_requested = 0usize;
    for index in 0..interface_count {
        let interface = unsafe { &*interface_ptr.add(index) };
        let status = unsafe {
            WlanScan(client, &interface.interface_guid, std::ptr::null(), std::ptr::null(), std::ptr::null_mut())
        };
        if status == 0 {
            scan_requested += 1;
        } else {
            scan_errors.push(status);
        }
    }
    if scan_requested == 0 {
        let code = scan_errors.iter().copied().find(|code| *code == 5)
            .or_else(|| scan_errors.first().copied()).unwrap_or(1);
        return Err(wlan_api_error(code, "WlanScan"));
    }

    // WlanScan is asynchronous. Drivers are expected to finish within four
    // seconds; this worker waits off the UI thread before reading the result.
    std::thread::sleep(Duration::from_secs(4));

    let mut networks = Vec::new();
    let mut query_errors = Vec::new();
    for index in 0..interface_count {
        let interface = unsafe { &*interface_ptr.add(index) };
        let mut raw_networks = std::ptr::null_mut();
        let status = unsafe {
            WlanGetAvailableNetworkList(client, &interface.interface_guid, 0, std::ptr::null_mut(), &mut raw_networks)
        };
        if status != 0 {
            if !raw_networks.is_null() { unsafe { WlanFreeMemory(raw_networks.cast()); } }
            query_errors.push(status);
            continue;
        }
        if raw_networks.is_null() {
            query_errors.push(13); // ERROR_INVALID_DATA
            continue;
        }
        let _networks_guard = WlanMemory(raw_networks.cast());
        let network_list = unsafe { &*raw_networks };
        let network_count = network_list.number_of_items as usize;
        let network_ptr = std::ptr::addr_of!(network_list.networks).cast::<WlanAvailableNetwork>();
        for network_index in 0..network_count {
            let network = unsafe { &*network_ptr.add(network_index) };
            let length = network.ssid.length as usize;
            if length == 0 || length > network.ssid.bytes.len() { continue; }
            let Some(name) = decode_ssid(&network.ssid.bytes[..length]) else { continue; };
            if !networks.iter().any(|known| known == &name) {
                networks.push(name);
            }
        }
    }
    if networks.is_empty() && !query_errors.is_empty() {
        let code = query_errors.iter().copied().find(|code| *code == 5)
            .or_else(|| query_errors.first().copied()).unwrap_or(1);
        return Err(wlan_api_error(code, "WlanGetAvailableNetworkList"));
    }
    networks.sort_by(|left, right| left.to_lowercase().cmp(&right.to_lowercase()));
    Ok(networks)
}

/// Query the current association through WlanQueryInterface rather than
/// treating a successful `netsh wlan connect` command as a completed join.
pub fn current_connection() -> io::Result<Option<WifiConnectionInfo>> {
    #[cfg(windows)]
    { current_connection_windows() }
    #[cfg(not(windows))]
    { Err(io::Error::new(io::ErrorKind::Unsupported, "Wi-Fi connection status is only available in the Windows build of NOVA.")) }
}

#[cfg(windows)]
fn current_connection_windows() -> io::Result<Option<WifiConnectionInfo>> {
    struct ClientHandle(*mut std::ffi::c_void);
    impl Drop for ClientHandle {
        fn drop(&mut self) { unsafe { WlanCloseHandle(self.0, std::ptr::null_mut()); } }
    }
    struct WlanMemory(*mut std::ffi::c_void);
    impl Drop for WlanMemory {
        fn drop(&mut self) { if !self.0.is_null() { unsafe { WlanFreeMemory(self.0); } } }
    }

    let mut negotiated_version = 0u32;
    let mut client = std::ptr::null_mut();
    let status = unsafe { WlanOpenHandle(2, std::ptr::null_mut(), &mut negotiated_version, &mut client) };
    if status != 0 { return Err(wlan_api_error(status, "WlanOpenHandle")); }
    let _client_guard = ClientHandle(client);

    let mut raw_interfaces = std::ptr::null_mut();
    let status = unsafe { WlanEnumInterfaces(client, std::ptr::null_mut(), &mut raw_interfaces) };
    if status != 0 {
        if !raw_interfaces.is_null() { unsafe { WlanFreeMemory(raw_interfaces.cast()); } }
        return Err(wlan_api_error(status, "WlanEnumInterfaces"));
    }
    if raw_interfaces.is_null() {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "Windows WLAN returned a null interface list."));
    }
    let _interfaces_guard = WlanMemory(raw_interfaces.cast());
    let interface_list = unsafe { &*raw_interfaces };
    let interface_count = interface_list.number_of_items as usize;
    let interface_ptr = std::ptr::addr_of!(interface_list.interface_info).cast::<WlanInterfaceInfo>();
    let mut query_errors = Vec::new();

    for index in 0..interface_count {
        let interface = unsafe { &*interface_ptr.add(index) };
        let mut data_size = 0u32;
        let mut raw_data = std::ptr::null_mut();
        let mut value_type = 3i32;
        // wlan_intf_opcode_current_connection is 7 in wlanapi.h.
        let status = unsafe { WlanQueryInterface(client, &interface.interface_guid, 7, std::ptr::null_mut(), &mut data_size, &mut raw_data, &mut value_type) };
        if status != 0 {
            if !raw_data.is_null() { unsafe { WlanFreeMemory(raw_data); } }
            // ERROR_INVALID_STATE indicates that this adapter is disconnected
            // or disconnecting; other errors leave the overall state unknown.
            if status != 5023 { query_errors.push(status); }
            continue;
        }
        if raw_data.is_null() || (data_size as usize) < std::mem::size_of::<WlanConnectionAttributes>() {
            if !raw_data.is_null() { unsafe { WlanFreeMemory(raw_data); } }
            query_errors.push(13); // ERROR_INVALID_DATA
            continue;
        }
        let _data_guard = WlanMemory(raw_data);
        let attributes = unsafe { &*raw_data.cast::<WlanConnectionAttributes>() };
        if attributes.state != 1 { continue; } // wlan_interface_state_connected
        let length = attributes.association.ssid.length as usize;
        if let Some(ssid) = decode_ssid(&attributes.association.ssid.bytes[..length.min(32)]) {
            return Ok(Some(WifiConnectionInfo {
                ssid,
                signal_quality: attributes.association.signal_quality.min(100),
            }));
        }
        return Err(io::Error::new(io::ErrorKind::InvalidData, "Windows reported a connected Wi-Fi interface with an invalid SSID."));
    }

    if let Some(code) = query_errors.first().copied() {
        // A disconnected result from one adapter must not hide an unknown
        // status from another adapter which failed its query.
        return Err(wlan_api_error(code, "WlanQueryInterface"));
    }
    Ok(None)
}

/// Wait briefly for the real WLAN state to reflect a connect/disconnect request.
pub fn wait_for_connection(expected_ssid: Option<&str>, timeout: Duration) -> io::Result<String> {
    let deadline = std::time::Instant::now() + timeout;
    let mut last_error = None;
    loop {
        match current_connection() {
            Ok(Some(connection)) => {
                if let Some(expected) = expected_ssid {
                    if connection.ssid.eq_ignore_ascii_case(expected) {
                        return Ok(format!("Connected to '{}' · signal {}%.", connection.ssid, connection.signal_quality));
                    }
                } else {
                    last_error = Some(format!("Still connected to '{}'", connection.ssid));
                }
            }
            Ok(None) if expected_ssid.is_none() => return Ok("Wi-Fi disconnected successfully.".to_owned()),
            Ok(None) => {}
            Err(error) => last_error = Some(error.to_string()),
        }
        if std::time::Instant::now() >= deadline { break; }
        std::thread::sleep(Duration::from_millis(500));
    }
    match expected_ssid {
        Some(ssid) => Ok(format!("Windows accepted the connection request for '{ssid}', but the WLAN API did not confirm association within {} seconds.{}", timeout.as_secs(), last_error.map(|error| format!(" Last status: {error}.")).unwrap_or_default())),
        None => Ok(format!("Windows accepted the disconnect request, but disconnection was not confirmed within {} seconds.{}", timeout.as_secs(), last_error.map(|error| format!(" Last status: {error}.")).unwrap_or_default())),
    }
}

/// Request disconnection from the active Windows WLAN network.
pub fn disconnect() -> io::Result<String> {
    let output = run_netsh(&["wlan", "disconnect"])?;
    if output.timed_out || output.drain_timed_out || !output.status.success() {
        return Err(io::Error::other(output.combined_text()));
    }
    Ok("Windows accepted the Wi-Fi disconnect request.".to_owned())
}

pub fn connect(paths: &AppPaths, ssid: &str, password: Option<&str>) -> io::Result<String> {
    validate_ssid(ssid)?;
    if let Some(secret) = password {
        validate_password(secret)?;
    }

    let profile_name = new_profile_name(ssid)?;
    if paths.is_nova_wifi_profile(ssid)? {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "A NOVA-managed profile already exists for this SSID. Delete that profile first, then reconnect to replace its saved credentials.",
        ));
    }
    if windows_profile_exists(&profile_name)? {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "A Windows Wi-Fi profile already uses NOVA's reserved profile name. NOVA left that profile untouched; remove or rename it in Windows Wi-Fi settings before retrying.",
        ));
    }
    let profile = build_profile(ssid, &profile_name, password);
    let mut temp_guard = create_temp_profile_file()?;
    temp_guard.file_mut().write_all(profile.as_bytes())?;
    temp_guard.file_mut().flush()?;
    temp_guard.file_mut().sync_all()?;
    // Close the writer before launching netsh. This avoids requiring netsh's
    // internal file opener to share DELETE access while the XML is imported.
    temp_guard.close_for_import();

    let import_result = run_netsh_import_profile(&temp_guard.path);
    // TempProfileGuard removes the plaintext XML even when process startup or
    // output collection fails. A child may already have imported the profile
    // before a later capture error, so every error path checks WLAN state.
    drop(temp_guard);
    let import_error = match import_result {
        Ok(import) if !import.timed_out && !import.drain_timed_out && import.status.success() => None,
        Ok(import) => Some(io::Error::other(import.combined_text())),
        Err(error) => Some(error),
    };
    if let Some(import_error) = import_error {
        match windows_profile_exists(&profile_name) {
            Ok(true) => {
                if let Err(cleanup_error) = rollback_new_profile(paths, ssid, &profile_name) {
                    return Err(io::Error::other(format!(
                        "{import_error}. NOVA could not fully roll back the partially imported profile: {cleanup_error}"
                    )));
                }
            }
            Ok(false) => {}
            Err(check_error) => {
                return Err(io::Error::other(format!(
                    "{import_error}. NOVA could not determine whether Windows partially imported the profile: {check_error}"
                )));
            }
        }
        return Err(import_error);
    }

    if let Err(error) = paths.add_nova_wifi_profile_named(ssid, &profile_name) {
        // Roll back the new profile so it cannot be left unmanaged on a
        // read-only USB drive or after a manifest serialization failure.
        if let Err(cleanup_error) = rollback_new_profile(paths, ssid, &profile_name) {
            return Err(io::Error::other(format!(
                "{error}. NOVA could not remove the newly imported Windows profile: {cleanup_error}"
            )));
        }
        return Err(error);
    }

    let connect_result = run_netsh(&[
        "wlan",
        "connect",
        &format!("name=\"{profile_name}\""),
        &format!("ssid=\"{ssid}\""),
    ])
    .and_then(|output| {
        if output.timed_out || output.drain_timed_out || !output.status.success() {
            Err(io::Error::other(output.combined_text()))
        } else {
            Ok(())
        }
    });

    if let Err(connect_error) = connect_result {
        // This attempt only imported a new profile (existing profiles are
        // refused above), so roll back Windows and the NOVA manifest together.
        if let Err(cleanup_error) = rollback_new_profile(paths, ssid, &profile_name) {
            return Err(io::Error::other(format!(
                "{connect_error}. NOVA could not fully roll back the new Wi-Fi profile: {cleanup_error}"
            )));
        }
        return Err(connect_error);
    }

    let mode = if password.is_some() { "secured" } else { "open" };
    Ok(format!(
        "Windows accepted the connection request for '{ssid}' ({mode} NOVA profile). Connection may take a few seconds."
    ))
}

/// Check profile names through the Windows WLAN API instead of parsing
/// localized `netsh` text. Errors are fail-closed: NOVA will not import over a
/// profile when it cannot prove that the reserved name is unused.
#[cfg(windows)]
fn windows_profile_exists(profile_name: &str) -> io::Result<bool> {
    struct ClientHandle(*mut std::ffi::c_void);
    impl Drop for ClientHandle {
        fn drop(&mut self) {
            unsafe { WlanCloseHandle(self.0, std::ptr::null_mut()); }
        }
    }
    struct WlanMemory(*mut std::ffi::c_void);
    impl Drop for WlanMemory {
        fn drop(&mut self) {
            if !self.0.is_null() { unsafe { WlanFreeMemory(self.0); } }
        }
    }

    let mut negotiated_version = 0u32;
    let mut client = std::ptr::null_mut();
    let status = unsafe { WlanOpenHandle(2, std::ptr::null_mut(), &mut negotiated_version, &mut client) };
    if status != 0 {
        return Err(io::Error::from_raw_os_error(status as i32));
    }
    let _client_guard = ClientHandle(client);

    let mut raw_interfaces = std::ptr::null_mut();
    let status = unsafe { WlanEnumInterfaces(client, std::ptr::null_mut(), &mut raw_interfaces) };
    if status != 0 {
        if !raw_interfaces.is_null() {
            unsafe { WlanFreeMemory(raw_interfaces.cast()); }
        }
        return Err(io::Error::from_raw_os_error(status as i32));
    }
    let interfaces_guard = WlanMemory(raw_interfaces.cast());
    if raw_interfaces.is_null() {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "Windows WLAN returned a null interface list."));
    }
    let interface_list = unsafe { &*raw_interfaces };
    let interface_count = interface_list.number_of_items as usize;
    let interface_ptr = std::ptr::addr_of!(interface_list.interface_info).cast::<WlanInterfaceInfo>();
    let mut found = false;

    for index in 0..interface_count {
        let interface = unsafe { &*interface_ptr.add(index) };
        let mut raw_profiles = std::ptr::null_mut();
        let status = unsafe {
            WlanGetProfileList(client, &interface.interface_guid, std::ptr::null_mut(), &mut raw_profiles)
        };
        if status != 0 {
            // Fail closed on every API error. Without a complete profile list,
            // NOVA cannot prove the reserved name is unused on this adapter.
            if !raw_profiles.is_null() {
                unsafe { WlanFreeMemory(raw_profiles.cast()); }
            }
            return Err(io::Error::from_raw_os_error(status as i32));
        }
        let profiles_guard = WlanMemory(raw_profiles.cast());
        if raw_profiles.is_null() {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "Windows WLAN returned a null profile list."));
        }
        let profile_list = unsafe { &*raw_profiles };
        let profile_count = profile_list.number_of_items as usize;
        let profile_ptr = std::ptr::addr_of!(profile_list.profile_info).cast::<WlanProfileInfo>();
        for profile_index in 0..profile_count {
            let profile = unsafe { &*profile_ptr.add(profile_index) };
            let length = profile.profile_name.iter().position(|unit| *unit == 0).unwrap_or(profile.profile_name.len());
            let name = String::from_utf16_lossy(&profile.profile_name[..length]);
            if name.eq_ignore_ascii_case(profile_name) {
                found = true;
                break;
            }
        }
        drop(profiles_guard);
        if found { break; }
    }
    drop(interfaces_guard);
    Ok(found)
}

#[cfg(not(windows))]
fn windows_profile_exists(_profile_name: &str) -> io::Result<bool> {
    Err(io::Error::new(io::ErrorKind::Unsupported, "Windows WLAN profiles are available only in the Windows build of NOVA."))
}

/// Remove a profile created by this connect attempt. Keep the manifest entry
/// if Windows refuses deletion, so the user can still manage/retry cleanup.
fn delete_windows_profile_if_present(profile_name: &str) -> io::Result<bool> {
    if !windows_profile_exists(profile_name)? {
        return Ok(false);
    }

    let delete_result = run_netsh(&[
        "wlan",
        "delete",
        "profile",
        &format!("name=\"{profile_name}\""),
    ]);
    let delete_error = match delete_result {
        Ok(output) if !output.timed_out && !output.drain_timed_out && output.status.success() => None,
        Ok(output) => Some(io::Error::other(output.combined_text())),
        Err(error) => Some(error),
    };

    if let Some(error) = delete_error {
        // netsh may have deleted the profile before a timeout or output-capture
        // error. Re-query Windows so a stale manifest entry is not retained.
        if windows_profile_exists(profile_name)? {
            return Err(error);
        }
    }
    Ok(true)
}

fn rollback_new_profile(paths: &AppPaths, ssid: &str, profile_name: &str) -> io::Result<()> {
    delete_windows_profile_if_present(profile_name)?;
    paths.remove_nova_wifi_profile(ssid)
}

pub fn delete_profile(paths: &AppPaths, ssid: &str) -> io::Result<String> {
    validate_ssid(ssid)?;
    if !paths.is_nova_wifi_profile(ssid)? {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "NOVA has no managed profile for this SSID; refusing to delete another Windows profile.",
        ));
    }

    let profile_name = paths.nova_wifi_profile_name(ssid)?
        .unwrap_or_else(|| format!("{PROFILE_PREFIX}{ssid}"));
    let profile_existed = delete_windows_profile_if_present(&profile_name)?;
    paths.remove_nova_wifi_profile(ssid)?;
    if profile_existed {
        Ok(format!("Deleted NOVA-managed Wi-Fi profile for '{ssid}'."))
    } else {
        Ok(format!("Removed the stale NOVA Wi-Fi record for '{ssid}'; the Windows profile was already absent."))
    }
}

fn validate_password(password: &str) -> io::Result<()> {
    let valid_passphrase = (8..=63).contains(&password.len())
        && password.bytes().all(|byte| (0x20..=0x7E).contains(&byte));
    let valid_hex_key = password.len() == 64 && password.bytes().all(|byte| byte.is_ascii_hexdigit());
    if !valid_passphrase && !valid_hex_key {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "WPA2 password must be 8–63 printable ASCII characters, or a 64-character hexadecimal key.",
        ));
    }
    Ok(())
}

fn build_profile(ssid: &str, profile_name: &str, password: Option<&str>) -> String {
    let ssid_xml = xml_escape(ssid);
    let name_xml = xml_escape(profile_name);
    match password {
        Some(secret) => {
            let key_type = if secret.len() == 64 && secret.chars().all(|ch| ch.is_ascii_hexdigit()) {
                "networkKey"
            } else {
                "passPhrase"
            };
            format!(
                r#"<?xml version="1.0"?>
<WLANProfile xmlns="http://www.microsoft.com/networking/WLAN/profile/v1">
  <name>{name_xml}</name>
  <SSIDConfig><SSID><name>{ssid_xml}</name></SSID></SSIDConfig>
  <connectionType>ESS</connectionType><connectionMode>auto</connectionMode>
  <MSM><security><authEncryption><authentication>WPA2PSK</authentication><encryption>AES</encryption><useOneX>false</useOneX></authEncryption>
  <sharedKey><keyType>{key_type}</keyType><protected>false</protected><keyMaterial>{}</keyMaterial></sharedKey></security></MSM>
</WLANProfile>"#,
                xml_escape(secret)
            )
        },
        None => format!(
            r#"<?xml version="1.0"?>
<WLANProfile xmlns="http://www.microsoft.com/networking/WLAN/profile/v1">
  <name>{name_xml}</name>
  <SSIDConfig><SSID><name>{ssid_xml}</name></SSID></SSIDConfig>
  <connectionType>ESS</connectionType><connectionMode>auto</connectionMode>
  <MSM><security><authEncryption><authentication>open</authentication><encryption>none</encryption><useOneX>false</useOneX></authEncryption></security></MSM>
</WLANProfile>"#
        ),
    }
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

struct TempProfileGuard {
    path: std::path::PathBuf,
    file: Option<File>,
}

impl TempProfileGuard {
    fn file_mut(&mut self) -> &mut File {
        self.file.as_mut().expect("temporary profile file is already closed")
    }

    fn close_for_import(&mut self) {
        drop(self.file.take());
    }
}

impl Drop for TempProfileGuard {
    fn drop(&mut self) {
        // Remove after netsh has exited. If NOVA crashes before this runs, the
        // restrictive file DACL protects the orphan until startup cleanup.
        drop(self.file.take());
        let _ = fs::remove_file(&self.path);
    }
}

/// Build netsh's `filename="..."` token without converting a Windows path
/// through UTF-8/lossy text. This preserves non-ASCII user profile paths.
fn netsh_filename_argument(path: &Path) -> OsString {
    let mut argument = OsString::from("filename=\"");
    argument.push(path.as_os_str());
    argument.push("\"");
    argument
}

fn temp_profile_path() -> io::Result<std::path::PathBuf> {
    static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_nanos();
    let sequence = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    Ok(std::env::temp_dir().join(format!(
        "nova-wlan-{}-{stamp}-{sequence}.xml",
        std::process::id()
    )))
}

const STALE_TEMP_PROFILE_AGE: Duration = Duration::from_secs(10 * 60);

/// Best-effort cleanup for protected WLAN XML files left by an abnormal exit.
/// Only regular files matching NOVA's exact generated filename format and
/// older than the maximum expected WLAN operation window are eligible.
pub fn cleanup_stale_temp_profiles() -> io::Result<usize> {
    let temp_dir = std::env::temp_dir();
    let now = SystemTime::now();
    let mut removed = 0usize;

    for entry in fs::read_dir(temp_dir)? {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        if !is_nova_temp_profile_name(&entry.file_name()) {
            continue;
        }
        let metadata = match fs::symlink_metadata(entry.path()) {
            Ok(metadata) if metadata.file_type().is_file() => metadata,
            _ => continue,
        };
        let is_stale = metadata
            .modified()
            .ok()
            .and_then(|modified| now.duration_since(modified).ok())
            .is_some_and(|age| age >= STALE_TEMP_PROFILE_AGE);
        if is_stale && fs::remove_file(entry.path()).is_ok() {
            removed += 1;
        }
    }
    Ok(removed)
}

fn is_nova_temp_profile_name(name: &std::ffi::OsStr) -> bool {
    let Some(name) = name.to_str() else { return false; };
    let Some(rest) = name.strip_prefix("nova-wlan-") else { return false; };
    let mut parts = rest.split('-');
    let (Some(pid), Some(timestamp), Some(sequence_xml), None) =
        (parts.next(), parts.next(), parts.next(), parts.next()) else {
            return false;
        };
    let Some(sequence) = sequence_xml.strip_suffix(".xml") else { return false; };
    !pid.is_empty()
        && pid.bytes().all(|byte| byte.is_ascii_digit())
        && !timestamp.is_empty()
        && timestamp.bytes().all(|byte| byte.is_ascii_digit())
        && !sequence.is_empty()
        && sequence.bytes().all(|byte| byte.is_ascii_digit())
}

fn create_temp_profile_file() -> io::Result<TempProfileGuard> {
    for _ in 0..8 {
        let path = temp_profile_path()?;
        #[cfg(windows)]
        let created = create_restricted_temp_file(&path);
        #[cfg(not(windows))]
        let created = OpenOptions::new().write(true).create_new(true).open(&path);

        match created {
            Ok(file) => return Ok(TempProfileGuard { path, file: Some(file) }),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "Could not allocate a unique temporary WLAN profile file.",
    ))
}

/// Create a TEMP file with a protected DACL for the file owner, SYSTEM and
/// local Administrators. It is deliberately not held with DELETE access while
/// `netsh` imports it, avoiding share-mode conflicts with the WLAN importer.
#[cfg(windows)]
fn create_restricted_temp_file(path: &std::path::Path) -> io::Result<File> {
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::FromRawHandle;
    use std::ptr;


    const SDDL_REVISION_1: u32 = 1;
    const GENERIC_WRITE: u32 = 0x4000_0000;
    const FILE_SHARE_READ: u32 = 0x0000_0001;
    const FILE_SHARE_WRITE: u32 = 0x0000_0002;
    const FILE_SHARE_DELETE: u32 = 0x0000_0004;
    const CREATE_NEW: u32 = 1;
    const FILE_ATTRIBUTE_TEMPORARY: u32 = 0x0000_0100;

    // OW = Owner Rights; SY = Local System; BA = Built-in Administrators.
    // D:P protects the DACL from inheriting broader permissions from TEMP.
    let sddl: Vec<u16> = "D:P(A;;FA;;;OW)(A;;FA;;;SY)(A;;FA;;;BA)\0".encode_utf16().collect();
    let mut descriptor = ptr::null_mut();
    let converted = unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            SDDL_REVISION_1,
            &mut descriptor,
            ptr::null_mut(), // The descriptor size is optional in the Win32 API.
        )
    };
    if converted == 0 {
        return Err(io::Error::last_os_error());
    }

    let mut security = SecurityAttributes {
        length: std::mem::size_of::<SecurityAttributes>() as u32,
        security_descriptor: descriptor,
        inherit_handle: 0,
    };
    let name: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let handle = unsafe {
        CreateFileW(
            name.as_ptr(),
            GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            &mut security,
            CREATE_NEW,
            FILE_ATTRIBUTE_TEMPORARY,
            ptr::null_mut(),
        )
    };
    let creation_error = if handle == (-1isize as *mut std::ffi::c_void) {
        Some(io::Error::last_os_error())
    } else {
        None
    };
    unsafe { let _ = LocalFree(descriptor); }

    if let Some(error) = creation_error {
        return Err(error);
    }
    // SAFETY: CreateFileW returned a valid, uniquely-owned file handle.
    Ok(unsafe { File::from_raw_handle(handle) })
}

#[cfg(windows)]
fn run_netsh_import_profile(path: &Path) -> io::Result<crate::windows_process::CapturedOutput> {
    let mut command = Command::new(crate::windows_process::system_binary("netsh.exe")?);
    command.args(["wlan", "add", "profile"]);
    command.arg(netsh_filename_argument(path));
    command.arg("user=current");
    crate::windows_process::run_captured(command, Duration::from_secs(20))
}

#[cfg(not(windows))]
fn run_netsh_import_profile(_path: &Path) -> io::Result<crate::windows_process::CapturedOutput> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "Windows WLAN commands are available only in the Windows build of NOVA.",
    ))
}

#[cfg(windows)]
fn run_netsh(args: &[&str]) -> io::Result<crate::windows_process::CapturedOutput> {
    let mut command = Command::new(crate::windows_process::system_binary("netsh.exe")?);
    command.args(args);
    crate::windows_process::run_captured(command, Duration::from_secs(20))
}

#[cfg(not(windows))]
fn run_netsh(_args: &[&str]) -> io::Result<crate::windows_process::CapturedOutput> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "Windows WLAN commands are available only in the Windows build.",
    ))
}

#[cfg(test)]
mod tests {
    use super::{build_profile, create_temp_profile_file, is_nova_temp_profile_name, validate_password, validate_ssid};
    use std::fs;
    use std::io::Write;

    #[test]
    fn xml_escapes_ssid_and_password() {
        let xml = build_profile("Home & Lab", "NOVA::Home & Lab", Some("pass&word123"));
        assert!(xml.contains("Home &amp; Lab"));
        assert!(xml.contains("pass&amp;word123"));
    }

    #[test]
    fn profile_xml_preserves_vietnamese_ssid_and_escapes_xml_metacharacters() {
        let xml = build_profile("Wi-Fi Việt & Lab", "NOVA::Wi-Fi Việt & Lab", Some("password123"));
        assert!(xml.contains("Wi-Fi Việt &amp; Lab"));
        assert!(xml.contains("NOVA::Wi-Fi Việt &amp; Lab"));
        assert!(xml.contains("<keyMaterial>password123</keyMaterial>"));
    }

    #[test]
    fn validates_wpa2_password_length() {
        assert!(validate_password("12345678").is_ok());
        assert!(validate_password("short").is_err());
        assert!(validate_password("pass\nword123").is_err());
        assert!(validate_password("pässword123").is_err());
        // A 64-character hexadecimal string is a valid raw WPA2 PSK,
        // not an overlong passphrase. Non-hex 64-character values are invalid.
        assert!(validate_password(&"a".repeat(64)).is_ok());
        assert!(validate_password(&"g".repeat(64)).is_err());
        assert!(validate_password(&"a1".repeat(32)).is_ok());
    }

    #[test]
    fn temp_profile_guard_closes_and_removes_file_on_drop() {
        let mut guard = create_temp_profile_file().unwrap();
        let path = guard.path.clone();
        guard.file_mut().write_all(b"temporary").unwrap();
        guard.file_mut().flush().unwrap();
        assert!(path.exists());
        // Verify the handoff closes the writer before the importer opens it.
        guard.close_for_import();
        assert_eq!(fs::read(&path).unwrap(), b"temporary");
        drop(guard);
        assert!(!path.exists());
    }

    #[test]
    fn netsh_filename_argument_preserves_unicode_paths() {
        let path = std::path::Path::new(r#"C:\Users\Nguyễn Văn A\AppData\Local\Temp\nova-wlan-1-123-0.xml"#);
        assert_eq!(super::netsh_filename_argument(path).to_string_lossy(), r#"filename="C:\Users\Nguyễn Văn A\AppData\Local\Temp\nova-wlan-1-123-0.xml""#);
    }

    #[test]
    fn generated_profile_names_are_unique_and_keep_the_ssid_context() {
        let first = super::new_profile_name("Home WiFi").unwrap();
        let second = super::new_profile_name("Home WiFi").unwrap();
        assert!(first.starts_with("NOVA::Home WiFi::"));
        assert_ne!(first, second);
    }

    #[test]
    fn stale_profile_name_matcher_accepts_only_generated_names() {
        assert!(is_nova_temp_profile_name(std::ffi::OsStr::new("nova-wlan-123-456789-0.xml")));
        assert!(!is_nova_temp_profile_name(std::ffi::OsStr::new("nova-wlan-not-a-profile.xml")));
        assert!(!is_nova_temp_profile_name(std::ffi::OsStr::new("other-file.xml")));
    }

    #[test]
    fn temp_profile_guard_removes_file_when_dropped_with_handle_open() {
        let guard = create_temp_profile_file().unwrap();
        let path = guard.path.clone();
        assert!(path.exists());
        drop(guard);
        assert!(!path.exists());
    }

    #[test]
    fn validates_ssid_byte_length() {
        assert!(validate_ssid("Home WiFi").is_ok());
        assert!(validate_ssid("").is_err());
        assert!(validate_ssid(&"a".repeat(33)).is_err());
        assert!(validate_ssid("bad\nssid").is_err());
        assert!(validate_ssid("bad\"ssid").is_err());
    }
}

#[cfg(test)]
mod connection_tests {
    #[test]
    fn expected_ssid_comparison_is_case_sensitive() {
        assert!(super::matches_expected_ssid("MyNetwork", "MyNetwork"));
        assert!(!super::matches_expected_ssid("MyNetwork", "mynetwork"));
    }
}

#[cfg(test)]
mod scan_tests {
    use super::decode_ssid;

    #[test]
    fn decodes_utf8_ssid_including_vietnamese() {
        assert_eq!(decode_ssid("NOVA Wi-Fi Việt Nam".as_bytes()).as_deref(), Some("NOVA Wi-Fi Việt Nam"));
    }

    #[test]
    fn rejects_empty_oversized_control_and_non_utf8_ssids() {
        assert!(decode_ssid(&[]).is_none());
        assert!(decode_ssid(&[b'A'; 33]).is_none());
        assert!(decode_ssid(b"bad\nname").is_none());
        assert!(decode_ssid(&[0xff, 0xfe]).is_none());
    }

    #[cfg(windows)]
    #[test]
    fn access_denied_from_scan_api_has_location_permission_guidance() {
        let error = super::wlan_api_error(5, "WlanScan");
        assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
        assert!(error.to_string().contains("Let desktop apps access your location"));
    }
}
