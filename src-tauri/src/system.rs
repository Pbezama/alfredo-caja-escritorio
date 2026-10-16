//! Telemetría del equipo para `info()` (T9.8): SO, WebView, batería, volumen, red, memoria
//! y disco. La manda la WEB en el heartbeat de F0 (los tokens no salen de la página).

use serde::Serialize;

#[derive(Debug, Clone, Serialize, Default, PartialEq)]
pub struct Battery {
    pub level: u8,
    pub charging: bool,
}

#[derive(Debug, Clone, Serialize, Default, PartialEq)]
pub struct Volume {
    pub master: u8,
    pub muted: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Network {
    #[serde(rename = "type")]
    pub kind: String,
    pub wifi_strength: Option<u8>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub os: String,
    pub hostname: Option<String>,
    pub battery: Option<Battery>,
    pub volume: Option<Volume>,
    pub network: Network,
    pub memory_free_mb: u64,
    pub disk_free_mb: u64,
}

pub fn snapshot() -> Snapshot {
    use sysinfo::{Disks, System};
    let mut sys = System::new();
    sys.refresh_memory();
    let disks = Disks::new_with_refreshed_list();
    let disk_free = disks.list().iter().map(|d| d.available_space()).max().unwrap_or(0);
    Snapshot {
        os: System::long_os_version().unwrap_or_else(|| std::env::consts::OS.to_string()),
        hostname: System::host_name(),
        battery: battery(),
        volume: volume(),
        network: network(),
        memory_free_mb: sys.available_memory() / 1024 / 1024,
        disk_free_mb: disk_free / 1024 / 1024,
    }
}

// ── Batería ─────────────────────────────────────────────────────────────────

#[cfg(windows)]
fn battery() -> Option<Battery> {
    use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
    let mut s = SYSTEM_POWER_STATUS::default();
    unsafe { GetSystemPowerStatus(&mut s) }.ok()?;
    // BatteryFlag 128 = sin batería (PC de escritorio); 255 = desconocido.
    if s.BatteryFlag & 128 != 0 || s.BatteryFlag == 255 || s.BatteryLifePercent > 100 {
        return None;
    }
    Some(Battery { level: s.BatteryLifePercent, charging: s.ACLineStatus == 1 })
}

#[cfg(target_os = "macos")]
fn battery() -> Option<Battery> {
    let out = std::process::Command::new("/usr/bin/pmset").args(["-g", "batt"]).output().ok()?;
    parse_pmset(&String::from_utf8_lossy(&out.stdout))
}

#[cfg(not(any(windows, target_os = "macos")))]
fn battery() -> Option<Battery> {
    None
}

/// «Now drawing from 'AC Power' -InternalBattery-0 (id=…)	87%; charging; …».
pub fn parse_pmset(s: &str) -> Option<Battery> {
    let line = s.lines().find(|l| l.contains("InternalBattery"))?;
    let pct = line.split('\t').nth(1)?.split('%').next()?.trim().parse::<u8>().ok()?;
    let ac = s.contains("'AC Power'");
    Some(Battery { level: pct.min(100), charging: ac && !line.contains("discharging") })
}

// ── Volumen ─────────────────────────────────────────────────────────────────

#[cfg(windows)]
fn volume() -> Option<Volume> {
    use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
    use windows::Win32::Media::Audio::{eConsole, eRender, IMMDeviceEnumerator, MMDeviceEnumerator};
    use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED};
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let en: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
        let dev = en.GetDefaultAudioEndpoint(eRender, eConsole).ok()?;
        let vol: IAudioEndpointVolume = dev.Activate(CLSCTX_ALL, None).ok()?;
        let level = vol.GetMasterVolumeLevelScalar().ok()?;
        let muted = vol.GetMute().ok()?.as_bool();
        Some(Volume { master: (level * 100.0).round().clamp(0.0, 100.0) as u8, muted })
    }
}

#[cfg(target_os = "macos")]
fn volume() -> Option<Volume> {
    let out = std::process::Command::new("/usr/bin/osascript").args(["-e", "get volume settings"]).output().ok()?;
    parse_osascript_volume(&String::from_utf8_lossy(&out.stdout))
}

#[cfg(not(any(windows, target_os = "macos")))]
fn volume() -> Option<Volume> {
    None
}

/// «output volume:44, input volume:50, alert volume:100, output muted:false».
pub fn parse_osascript_volume(s: &str) -> Option<Volume> {
    let get = |k: &str| s.split(',').map(str::trim).find_map(|p| p.strip_prefix(k)).map(str::trim);
    let master = get("output volume:")?.parse::<u8>().ok()?;
    let muted = get("output muted:").map(|m| m == "true").unwrap_or(false);
    Some(Volume { master: master.min(100), muted })
}

// ── Red ─────────────────────────────────────────────────────────────────────

#[cfg(windows)]
fn network() -> Network {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let out = std::process::Command::new("netsh").args(["wlan", "show", "interfaces"]).creation_flags(CREATE_NO_WINDOW).output();
    let wifi = out.ok().and_then(|o| parse_netsh_signal(&String::from_utf8_lossy(&o.stdout)));
    match wifi {
        Some(s) => Network { kind: "wifi".into(), wifi_strength: Some(s) },
        None => Network { kind: if has_ipv4() { "ethernet".into() } else { "none".into() }, wifi_strength: None },
    }
}

#[cfg(not(windows))]
fn network() -> Network {
    Network { kind: if has_ipv4() { "unknown".into() } else { "none".into() }, wifi_strength: None }
}

fn has_ipv4() -> bool {
    if_addrs::get_if_addrs()
        .map(|v| v.iter().any(|i| !i.is_loopback() && matches!(i.addr, if_addrs::IfAddr::V4(ref a) if !a.ip.is_link_local())))
        .unwrap_or(false)
}

/// `netsh wlan show interfaces`: «State : connected» y «Signal : 80%» (también en español:
/// «Estado : conectado», «Señal : 80%»).
pub fn parse_netsh_signal(s: &str) -> Option<u8> {
    let connected = s.lines().any(|l| {
        let l = l.trim().to_lowercase();
        (l.starts_with("state") || l.starts_with("estado")) && (l.ends_with("connected") || l.ends_with("conectado"))
    });
    if !connected {
        return None;
    }
    s.lines().find_map(|l| {
        let l = l.trim();
        let lower = l.to_lowercase();
        if lower.starts_with("signal") || lower.starts_with("señal") || lower.starts_with("se�al") {
            l.split(':').nth(1)?.trim().trim_end_matches('%').trim().parse::<u8>().ok()
        } else {
            None
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pmset() {
        let s = "Now drawing from 'AC Power'\n -InternalBattery-0 (id=123)\t87%; charging; 0:45 remaining present: true\n";
        assert_eq!(parse_pmset(s), Some(Battery { level: 87, charging: true }));
        let s = "Now drawing from 'Battery Power'\n -InternalBattery-0 (id=1)\t40%; discharging; 3:00 remaining\n";
        assert_eq!(parse_pmset(s), Some(Battery { level: 40, charging: false }));
        assert_eq!(parse_pmset("Now drawing from 'AC Power'\n"), None);
    }

    #[test]
    fn volumen_mac() {
        assert_eq!(parse_osascript_volume("output volume:44, input volume:50, alert volume:100, output muted:false\n"), Some(Volume { master: 44, muted: false }));
        assert_eq!(parse_osascript_volume("output volume:0, input volume:50, alert volume:100, output muted:true"), Some(Volume { master: 0, muted: true }));
        assert_eq!(parse_osascript_volume("missing value"), None);
    }

    #[test]
    fn senal_wifi() {
        let en = "    Name                   : Wi-Fi\n    State                  : connected\n    Signal                 : 80%\n";
        assert_eq!(parse_netsh_signal(en), Some(80));
        let es = "    Nombre                 : Wi-Fi\n    Estado                 : conectado\n    Señal                  : 63%\n";
        assert_eq!(parse_netsh_signal(es), Some(63));
        let off = "    State                  : disconnected\n";
        assert_eq!(parse_netsh_signal(off), None);
    }
}
