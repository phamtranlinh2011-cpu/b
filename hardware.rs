//! Host hardware telemetry for the NOVA Hardware Center.
//!
//! Values are collected from Windows/system providers. If a driver or firmware
//! does not expose a counter, the corresponding value remains unavailable.

use std::io;
use std::sync::mpsc::{self, Receiver, Sender, SyncSender, TryRecvError, TrySendError};
use std::thread;
#[cfg(windows)]
use std::process::Command;
use std::time::{Duration, Instant};
use sysinfo::{Components, Disks, Gpus, Networks};

fn instant_ago(duration: Duration) -> Instant {
    let now = Instant::now();
    now.checked_sub(duration).unwrap_or(now)
}

fn bytes_per_second(delta_bytes: u64, elapsed_seconds: Option<f64>) -> Option<f64> {
    let seconds = elapsed_seconds?;
    (seconds.is_finite() && seconds > 0.0).then(|| delta_bytes as f64 / seconds)
}


#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HardwareProviderState {
    #[default]
    Unavailable,
    NoDevices,
    Available,
}

fn gpu_state(provider_available: bool, adapter_count: usize) -> HardwareProviderState {
    if !provider_available {
        HardwareProviderState::Unavailable
    } else if adapter_count == 0 {
        HardwareProviderState::NoDevices
    } else {
        HardwareProviderState::Available
    }
}

#[derive(Clone, Debug, Default)]
pub struct HardwareSnapshot {
    pub disks: Vec<DiskSnapshot>,
    pub networks: Vec<NetworkSnapshot>,
    pub gpus: Vec<GpuSnapshot>,
    pub sensors: Vec<SensorSnapshot>,
    pub battery: Option<BatterySnapshot>,
    pub disk_provider_available: bool,
    pub network_provider_available: bool,
    pub gpu_provider_state: HardwareProviderState,
    pub sensor_provider_available: bool,
}

#[derive(Clone, Debug)]
pub struct DiskSnapshot {
    pub name: String,
    pub mount_point: String,
    pub file_system: String,
    pub total_bytes: u64,
    pub available_bytes: u64,
    pub removable: bool,
    pub read_only: bool,
}

#[derive(Clone, Debug)]
pub struct NetworkSnapshot {
    pub name: String,
    pub received_per_sec: f64,
    pub transmitted_per_sec: f64,
    pub total_received: u64,
    pub total_transmitted: u64,
    /// False until at least two provider samples exist to calculate a real rate.
    pub rate_available: bool,
}

#[derive(Clone, Debug)]
pub struct GpuSnapshot {
    pub name: String,
    pub vendor: Option<String>,
    pub usage_percent: Option<f32>,
    pub total_memory: Option<u64>,
    pub used_memory: Option<u64>,
}

#[derive(Clone, Debug)]
pub struct SensorSnapshot {
    pub name: String,
    pub temperature_c: f32,
}

#[derive(Clone, Debug)]
pub struct BatterySnapshot {
    pub ac_online: Option<bool>,
    /// `None` means Windows reported unknown battery status, not that no battery exists.
    pub battery_present: Option<bool>,
    pub charge_percent: Option<u8>,
    pub charging: bool,
    pub battery_saver: bool,
    pub remaining_seconds: Option<u32>,
}

impl BatterySnapshot {
    fn from_windows_fields(
        ac_line_status: u8,
        battery_flag: u8,
        battery_life_percent: u8,
        system_status_flag: u8,
        battery_life_time: u32,
    ) -> Self {
        let battery_present = match battery_flag {
            0xff => None, // BATTERY_FLAG_UNKNOWN
            flag if flag & 0x80 != 0 => Some(false), // BATTERY_FLAG_NO_BATTERY
            _ => Some(true),
        };
        let has_battery = battery_present == Some(true);
        Self {
            ac_online: match ac_line_status { 0 => Some(false), 1 => Some(true), _ => None },
            battery_present,
            charge_percent: (battery_life_percent <= 100 && has_battery).then_some(battery_life_percent),
            charging: has_battery && (battery_flag & 8) != 0,
            battery_saver: system_status_flag & 1 != 0,
            remaining_seconds: (battery_life_time != u32::MAX && has_battery).then_some(battery_life_time),
        }
    }
}

pub struct HardwareTelemetry {
    disks: Option<Disks>,
    networks: Option<Networks>,
    gpus: Option<Gpus>,
    components: Option<Components>,
    snapshot: HardwareSnapshot,
    last_disk_refresh: Instant,
    last_network_refresh: Instant,
    last_gpu_refresh: Instant,
    last_sensor_refresh: Instant,
    last_power_refresh: Instant,
    last_network_sample: Option<Instant>,
}

impl HardwareTelemetry {
    pub fn new() -> Self {
        Self {
            disks: Disks::new_with_refreshed_list().ok(),
            networks: Networks::new_with_refreshed_list().ok(),
            gpus: Gpus::new_with_refreshed_list().ok(),
            components: Components::new_with_refreshed_list().ok(),
            snapshot: HardwareSnapshot::default(),
            last_disk_refresh: instant_ago(Duration::from_secs(10)),
            last_network_refresh: instant_ago(Duration::from_secs(2)),
            last_gpu_refresh: instant_ago(Duration::from_secs(3)),
            last_sensor_refresh: instant_ago(Duration::from_secs(10)),
            last_power_refresh: instant_ago(Duration::from_secs(10)),
            last_network_sample: None,
        }
    }

    /// Force the next provider update without rebuilding providers or resetting
    /// the network byte-rate baseline.
    pub fn request_refresh(&mut self) {
        self.last_disk_refresh = instant_ago(Duration::from_secs(10));
        self.last_network_refresh = instant_ago(Duration::from_secs(2));
        self.last_gpu_refresh = instant_ago(Duration::from_secs(3));
        self.last_sensor_refresh = instant_ago(Duration::from_secs(10));
        self.last_power_refresh = instant_ago(Duration::from_secs(10));
    }

    /// Refresh providers on different schedules. Returns true only when at least
    /// one provider was sampled, allowing the UI to clone the snapshot at sample
    /// cadence rather than on every egui repaint.
    pub fn refresh(&mut self) -> bool {
        let now = Instant::now();
        let mut changed = false;

        if self.last_disk_refresh.elapsed() >= Duration::from_secs(5) {
            if let Some(disks) = self.disks.as_mut() {
                disks.refresh(true);
                self.snapshot.disks = disks.iter().map(|disk| DiskSnapshot {
                    name: disk.name().to_string_lossy().into_owned(),
                    mount_point: disk.mount_point().display().to_string(),
                    file_system: disk.file_system().to_string_lossy().into_owned(),
                    total_bytes: disk.total_space(),
                    available_bytes: disk.available_space(),
                    removable: disk.is_removable(),
                    read_only: disk.is_read_only(),
                }).collect();
                self.snapshot.disk_provider_available = true;
            } else {
                self.snapshot.disk_provider_available = false;
            }
            changed = true;
            self.last_disk_refresh = now;
        }

        if self.last_network_refresh.elapsed() >= Duration::from_secs(1) {
            let elapsed = self.last_network_sample.map(|sample| now.duration_since(sample).as_secs_f64().max(0.001));
            if let Some(networks) = self.networks.as_mut() {
                networks.refresh(true);
                self.snapshot.networks = networks.iter().map(|(name, data)| {
                    let received_rate = bytes_per_second(data.received(), elapsed);
                    let transmitted_rate = bytes_per_second(data.transmitted(), elapsed);
                    NetworkSnapshot {
                        name: name.to_string_lossy().into_owned(),
                        received_per_sec: received_rate.unwrap_or(0.0),
                        transmitted_per_sec: transmitted_rate.unwrap_or(0.0),
                        total_received: data.total_received(),
                        total_transmitted: data.total_transmitted(),
                        rate_available: received_rate.is_some() && transmitted_rate.is_some(),
                    }
                }).collect();
                self.snapshot.network_provider_available = true;
            } else {
                self.snapshot.network_provider_available = false;
            }
            self.last_network_sample = Some(now);
            changed = true;
            self.last_network_refresh = now;
        }

        if self.last_gpu_refresh.elapsed() >= Duration::from_secs(2) {
            if let Some(gpus) = self.gpus.as_mut() {
                gpus.refresh(true);
                self.snapshot.gpus = gpus.iter().map(|gpu| {
                    let vendor = gpu.vendor().map(str::to_owned);
                    let model = gpu.model().map(str::to_owned);
                    let name = match (&vendor, &model) {
                        (Some(vendor), Some(model)) if !model.to_ascii_lowercase().contains(&vendor.to_ascii_lowercase()) => format!("{vendor} {model}"),
                        (_, Some(model)) => model.clone(),
                        (Some(vendor), None) => vendor.clone(),
                        (None, None) => "GPU adapter".to_owned(),
                    };
                    GpuSnapshot {
                        name,
                        vendor,
                        usage_percent: gpu.usage().filter(|value| value.is_finite()).map(|value| value.clamp(0.0, 100.0)),
                        total_memory: gpu.total_memory(),
                        used_memory: gpu.used_memory(),
                    }
                }).collect();
                self.snapshot.gpu_provider_state = gpu_state(true, self.snapshot.gpus.len());
            } else {
                self.snapshot.gpus.clear();
                self.snapshot.gpu_provider_state = gpu_state(false, 0);
            }
            changed = true;
            self.last_gpu_refresh = now;
        }

        if self.last_sensor_refresh.elapsed() >= Duration::from_secs(5) {
            if let Some(components) = self.components.as_mut() {
                components.refresh(true);
                self.snapshot.sensors = components.iter().filter_map(|component| {
                    let temperature = component.temperature()?;
                    (temperature.is_finite() && (-40.0..=150.0).contains(&temperature)).then(|| SensorSnapshot {
                        name: component.label().to_owned(),
                        temperature_c: temperature,
                    })
                }).collect();
                self.snapshot.sensor_provider_available = true;
            } else {
                self.snapshot.sensor_provider_available = false;
            }
            changed = true;
            self.last_sensor_refresh = now;
        }

        if self.last_power_refresh.elapsed() >= Duration::from_secs(2) {
            self.snapshot.battery = read_power_status();
            changed = true;
            self.last_power_refresh = now;
        }

        changed
    }

    pub fn snapshot(&self) -> &HardwareSnapshot {
        &self.snapshot
    }
}

impl Default for HardwareTelemetry {
    fn default() -> Self { Self::new() }
}

/// Owns the potentially slow hardware providers on a dedicated worker thread.
/// Only plain snapshots cross back to the UI thread; sysinfo provider objects
/// never need to be Send or be refreshed during an egui frame.
enum HardwareWorkerCommand {
    Refresh,
    SetActive(bool),
}

pub struct HardwareWorker {
    snapshot_receiver: Receiver<HardwareSnapshot>,
    command_sender: Sender<HardwareWorkerCommand>,
    active: bool,
}

impl HardwareWorker {
    pub fn start() -> io::Result<Self> {
        let (snapshot_sender, snapshot_receiver): (SyncSender<HardwareSnapshot>, Receiver<HardwareSnapshot>) = mpsc::sync_channel(1);
        let (command_sender, command_receiver) = mpsc::channel();
        thread::Builder::new().name("nova-hardware-telemetry".to_owned()).spawn(move || {
            let mut telemetry = HardwareTelemetry::new();
            let mut active = true;
            loop {
                if active {
                    if telemetry.refresh() {
                        match snapshot_sender.try_send(telemetry.snapshot().clone()) {
                            Ok(()) | Err(TrySendError::Full(_)) => {}
                            Err(TrySendError::Disconnected(_)) => break,
                        }
                    }
                    match command_receiver.recv_timeout(Duration::from_millis(500)) {
                        Ok(HardwareWorkerCommand::Refresh) => telemetry.request_refresh(),
                        Ok(HardwareWorkerCommand::SetActive(next)) => {
                            active = next;
                            if active { telemetry.request_refresh(); }
                        }
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    }
                } else {
                    match command_receiver.recv() {
                        Ok(HardwareWorkerCommand::Refresh) => telemetry.request_refresh(),
                        Ok(HardwareWorkerCommand::SetActive(next)) => {
                            active = next;
                            if active { telemetry.request_refresh(); }
                        }
                        Err(_) => break,
                    }
                }
            }
        })?;
        Ok(Self { snapshot_receiver, command_sender, active: true })
    }

    /// Returns the newest queued snapshot, or None while the provider worker
    /// is sampling. A disconnected worker is reported explicitly.
    pub fn try_snapshot(&self) -> Result<Option<HardwareSnapshot>, TryRecvError> {
        match self.snapshot_receiver.try_recv() {
            Ok(snapshot) => Ok(Some(snapshot)),
            Err(TryRecvError::Empty) => Ok(None),
            Err(error @ TryRecvError::Disconnected) => Err(error),
        }
    }

    pub fn request_refresh(&self) -> bool {
        self.command_sender.send(HardwareWorkerCommand::Refresh).is_ok()
    }

    pub fn set_active(&mut self, active: bool) {
        if self.active != active {
            self.active = active;
            let _ = self.command_sender.send(HardwareWorkerCommand::SetActive(active));
        }
    }
}

/// Opens the corresponding Windows control surface. Device-specific controls
/// remain with Windows rather than writing undocumented hardware values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostControl {
    Display,
    Sound,
    Network,
    Bluetooth,
    Power,
}

/// Open the Windows privacy page where desktop-app location access can be
/// enabled after Windows blocks an explicit nearby Wi-Fi scan.
pub fn open_location_privacy_settings() -> io::Result<()> {
    open_settings_uri("ms-settings:privacy-location")
}

pub fn open_host_control(control: HostControl) -> io::Result<()> {
    match control {
        HostControl::Sound => {
            #[cfg(windows)]
            { Command::new(crate::windows_process::system_binary("sndvol.exe")?).spawn().map(|_| ()) }
            #[cfg(not(windows))]
            { Err(io::Error::new(io::ErrorKind::Unsupported, "Windows volume mixer is only available on Windows.")) }
        }
        HostControl::Display => open_settings_uri("ms-settings:display"),
        HostControl::Network => open_settings_uri("ms-settings:network-status"),
        HostControl::Bluetooth => open_settings_uri("ms-settings:bluetooth"),
        HostControl::Power => open_settings_uri("ms-settings:powersleep"),
    }
}

#[cfg(windows)]
fn open_settings_uri(uri: &str) -> io::Result<()> {
    use std::ffi::{c_void, OsStr};
    use std::os::windows::ffi::OsStrExt;
    let verb: Vec<u16> = OsStr::new("open").encode_wide().chain(Some(0)).collect();
    let target: Vec<u16> = OsStr::new(uri).encode_wide().chain(Some(0)).collect();
    #[link(name = "shell32")]
    unsafe extern "system" {
        fn ShellExecuteW(hwnd: *mut c_void, operation: *const u16, file: *const u16, parameters: *const u16, directory: *const u16, show: i32) -> *mut c_void;
    }
    let result = unsafe { ShellExecuteW(std::ptr::null_mut(), verb.as_ptr(), target.as_ptr(), std::ptr::null(), std::ptr::null(), 1) } as isize;
    if result <= 32 {
        Err(io::Error::other(format!("Windows could not open {uri} (ShellExecuteW={result}).")))
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn open_settings_uri(uri: &str) -> io::Result<()> {
    Err(io::Error::new(io::ErrorKind::Unsupported, format!("{uri} is only available on Windows.")))
}

#[cfg(windows)]
fn read_power_status() -> Option<BatterySnapshot> {
    #[repr(C)]
    #[derive(Default)]
    struct SystemPowerStatus {
        ac_line_status: u8,
        battery_flag: u8,
        battery_life_percent: u8,
        system_status_flag: u8,
        battery_life_time: u32,
        battery_full_life_time: u32,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetSystemPowerStatus(status: *mut SystemPowerStatus) -> i32;
    }
    let mut status = SystemPowerStatus::default();
    if unsafe { GetSystemPowerStatus(&mut status) } == 0 { return None; }
    Some(BatterySnapshot::from_windows_fields(
        status.ac_line_status,
        status.battery_flag,
        status.battery_life_percent,
        status.system_status_flag,
        status.battery_life_time,
    ))
}

#[cfg(not(windows))]
fn read_power_status() -> Option<BatterySnapshot> { None }

pub fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0usize;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 { format!("{} {}", bytes, UNITS[unit]) } else { format!("{value:.1} {}", UNITS[unit]) }
}

pub fn format_rate(bytes_per_second: f64) -> String {
    if !bytes_per_second.is_finite() || bytes_per_second < 0.0 { return "—".to_owned(); }
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes_per_second;
    let mut unit = 0usize;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 { format!("{value:.0} {}/s", UNITS[unit]) } else { format!("{value:.1} {}/s", UNITS[unit]) }
}

pub fn format_duration(seconds: u32) -> String {
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    if hours > 0 { format!("{hours}h {minutes}m") } else { format!("{minutes}m") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instant_backdating_is_safe_even_very_early_after_boot() {
        let before = instant_ago(Duration::from_secs(60 * 60 * 24));
        assert!(before <= Instant::now());
    }

    #[test]
    fn byte_format_uses_binary_units_and_handles_zero() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(1024), "1.0 KiB");
        assert_eq!(format_bytes(1024 * 1024), "1.0 MiB");
    }

    #[test]
    fn rate_format_rejects_negative_and_non_finite_values() {
        assert_eq!(format_rate(-1.0), "—");
        assert_eq!(format_rate(f64::NAN), "—");
        assert_eq!(format_rate(1024.0), "1.0 KiB/s");
    }

    #[test]
    fn network_rate_requires_a_valid_elapsed_sample() {
        assert_eq!(bytes_per_second(1024, None), None);
        assert_eq!(bytes_per_second(1024, Some(0.0)), None);
        assert_eq!(bytes_per_second(1024, Some(f64::NAN)), None);
        assert_eq!(bytes_per_second(1024, Some(2.0)), Some(512.0));
    }

    #[test]
    fn unknown_windows_battery_state_is_not_misreported_as_no_battery() {
        let status = BatterySnapshot::from_windows_fields(1, 0xff, 0xff, 0, u32::MAX);
        assert_eq!(status.battery_present, None);
        assert_eq!(status.charge_percent, None);
        assert!(!status.charging);
    }

    #[test]
    fn windows_no_battery_flag_is_reported_explicitly() {
        let status = BatterySnapshot::from_windows_fields(1, 0x80, 0xff, 0, u32::MAX);
        assert_eq!(status.battery_present, Some(false));
        assert_eq!(status.charge_percent, None);
    }

    #[test]
    fn windows_charging_state_and_battery_saver_are_decoded() {
        let status = BatterySnapshot::from_windows_fields(1, 0x08, 76, 1, 5400);
        assert_eq!(status.battery_present, Some(true));
        assert_eq!(status.charge_percent, Some(76));
        assert!(status.charging);
        assert!(status.battery_saver);
        assert_eq!(status.remaining_seconds, Some(5400));
    }

    #[test]
    fn duration_format_is_compact_and_deterministic() {
        assert_eq!(format_duration(0), "0m");
        assert_eq!(format_duration(3599), "59m");
        assert_eq!(format_duration(3661), "1h 1m");
    }

    #[test]
    fn gpu_provider_state_distinguishes_missing_adapter_from_unsupported_provider() {
        assert_eq!(super::gpu_state(false, 0), super::HardwareProviderState::Unavailable);
        assert_eq!(super::gpu_state(true, 0), super::HardwareProviderState::NoDevices);
        assert_eq!(super::gpu_state(true, 1), super::HardwareProviderState::Available);
    }

}
