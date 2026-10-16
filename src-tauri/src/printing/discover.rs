//! Descubrimiento de impresoras (T9.5): spooler (o colas CUPS en macOS), puertos serie,
//! LPT y red (:9100 en las /24 de las interfaces IPv4 activas, más mDNS
//! `_pdl-datastream._tcp` y `_printer._tcp`). Termina en ≤ 15 s (RF9.11).

use super::{PrinterKind, PrinterTarget};
use std::collections::{BTreeSet, HashMap};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::time::{Duration, Instant};

pub const SCAN_PORT: u16 = 9100;
pub const SCAN_TIMEOUT: Duration = Duration::from_millis(300);
pub const SCAN_PARALLEL: usize = 64;
pub const MDNS_WINDOW: Duration = Duration::from_secs(4);
pub const MAX_SUBNETS: usize = 3;

/// Redes /24 a barrer: la /24 propia de cada interfaz IPv4 privada activa (si la máscara es
/// más ancha que /24, sólo la propia). Sin loopback ni link-local.
pub fn subnets_24(ifaces: &[(Ipv4Addr, Ipv4Addr)]) -> Vec<[u8; 3]> {
    let mut out = BTreeSet::new();
    for (ip, _mask) in ifaces {
        if ip.is_loopback() || ip.is_link_local() || ip.is_unspecified() || !ip.is_private() {
            continue;
        }
        let o = ip.octets();
        out.insert([o[0], o[1], o[2]]);
    }
    out.into_iter().take(MAX_SUBNETS).collect()
}

/// Hosts .1–.254 de una /24 sin la IP propia.
pub fn hosts_of(net: [u8; 3], own: &[Ipv4Addr]) -> Vec<Ipv4Addr> {
    (1..=254u8)
        .map(|h| Ipv4Addr::new(net[0], net[1], net[2], h))
        .filter(|ip| !own.contains(ip))
        .collect()
}

fn local_ifaces() -> Vec<(Ipv4Addr, Ipv4Addr)> {
    if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|i| match i.addr {
            if_addrs::IfAddr::V4(v4) if !i.is_loopback() => Some((v4.ip, v4.netmask)),
            _ => None,
        })
        .collect()
}

/// Barrido :9100 en paralelo (64 a la vez, 300 ms cada uno).
pub fn scan_tcp(deadline: Instant) -> Vec<Ipv4Addr> {
    let ifaces = local_ifaces();
    let own: Vec<Ipv4Addr> = ifaces.iter().map(|(ip, _)| *ip).collect();
    let mut targets: Vec<Ipv4Addr> = subnets_24(&ifaces).into_iter().flat_map(|n| hosts_of(n, &own)).collect();
    let found = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    while !targets.is_empty() && Instant::now() < deadline {
        let batch: Vec<Ipv4Addr> = targets.drain(..targets.len().min(SCAN_PARALLEL)).collect();
        let handles: Vec<_> = batch
            .into_iter()
            .map(|ip| {
                let found = found.clone();
                std::thread::spawn(move || {
                    if TcpStream::connect_timeout(&SocketAddr::from((ip, SCAN_PORT)), SCAN_TIMEOUT).is_ok() {
                        found.lock().unwrap().push(ip);
                    }
                })
            })
            .collect();
        for h in handles {
            let _ = h.join();
        }
    }
    let mut v = found.lock().unwrap().clone();
    v.sort();
    v
}

/// mDNS: (ip, nombre) de `_pdl-datastream._tcp` y `_printer._tcp`.
pub fn browse_mdns(window: Duration) -> Vec<(Ipv4Addr, String, u16)> {
    let Ok(daemon) = mdns_sd::ServiceDaemon::new() else { return vec![] };
    let mut out = vec![];
    let mut receivers = vec![];
    for ty in ["_pdl-datastream._tcp.local.", "_printer._tcp.local."] {
        if let Ok(r) = daemon.browse(ty) {
            receivers.push(r);
        }
    }
    let end = Instant::now() + window;
    while Instant::now() < end {
        for r in &receivers {
            while let Ok(ev) = r.try_recv() {
                if let mdns_sd::ServiceEvent::ServiceResolved(info) = ev {
                    let name = info.get_fullname().split("._").next().unwrap_or("").replace("\\032", " ");
                    for ip in info.get_addresses_v4() {
                        out.push((*ip, name.clone(), info.get_port()));
                    }
                }
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let _ = daemon.shutdown();
    out
}

/// Junta barrido y mDNS: una fila por IP, con el nombre de mDNS si lo hay.
pub fn merge_network(scan: &[Ipv4Addr], mdns: &[(Ipv4Addr, String, u16)]) -> Vec<PrinterTarget> {
    let mut by_ip: HashMap<Ipv4Addr, (String, u16, &'static str)> = HashMap::new();
    for ip in scan {
        by_ip.insert(*ip, (format!("Impresora de red {ip}"), SCAN_PORT, "scan"));
    }
    for (ip, name, port) in mdns {
        let port = if *port == 0 { SCAN_PORT } else { *port };
        let label = if name.trim().is_empty() { format!("Impresora de red {ip}") } else { format!("{} ({ip})", name.trim()) };
        by_ip.insert(*ip, (label, port, "mdns"));
    }
    let mut v: Vec<_> = by_ip
        .into_iter()
        .map(|(ip, (name, port, source))| PrinterTarget {
            key: format!("tcp-{ip}"),
            kind: Some(PrinterKind::Tcp),
            name,
            host: Some(ip.to_string()),
            port: Some(port),
            source: Some(source.into()),
            ..Default::default()
        })
        .collect();
    v.sort_by(|a, b| a.host.cmp(&b.host));
    v
}

/// Nombres que no son impresoras de tickets (para ponerlas al final).
pub fn looks_virtual(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    ["pdf", "xps", "onenote", "fax", "send to", "enviar a"].iter().any(|k| n.contains(k))
}

pub fn system_printers() -> Vec<PrinterTarget> {
    let kind = if cfg!(windows) { PrinterKind::Spooler } else { PrinterKind::Cups };
    let mut v: Vec<PrinterTarget> = super::spooler::list()
        .into_iter()
        .map(|(name, driver, port)| PrinterTarget {
            key: format!("{}-{}", if cfg!(windows) { "spl" } else { "cups" }, slug(&name)),
            kind: Some(kind),
            name: if port.is_empty() { name.clone() } else { format!("{name} ({port})") },
            spooler_name: Some(name),
            driver: (!driver.is_empty()).then_some(driver),
            source: Some(if cfg!(windows) { "spooler" } else { "cups" }.into()),
            ..Default::default()
        })
        .collect();
    v.sort_by_key(|p| looks_virtual(&p.name));
    v
}

pub fn serial_ports() -> Vec<PrinterTarget> {
    serialport::available_ports()
        .unwrap_or_default()
        .into_iter()
        .filter(|p| keep_serial(&p.port_name))
        .map(|p| {
            let desc = match &p.port_type {
                serialport::SerialPortType::UsbPort(u) => u.product.clone().map(|x| format!("USB-serie {x}")).unwrap_or_else(|| "USB-serie".into()),
                serialport::SerialPortType::BluetoothPort => "Bluetooth".into(),
                _ => "Serie".into(),
            };
            PrinterTarget {
                key: format!("com-{}", slug(&p.port_name)),
                kind: Some(PrinterKind::Serial),
                name: format!("{} · {desc}", p.port_name),
                com: Some(p.port_name),
                baud: Some(9600),
                data_bits: Some(8),
                parity: Some("none".into()),
                stop_bits: Some(1),
                flow: Some("none".into()),
                source: Some("serial".into()),
                ..Default::default()
            }
        })
        .collect()
}

/// En macOS sólo /dev/cu.* (no tty.*, que espera DCD) y sin los puertos internos.
pub fn keep_serial(path: &str) -> bool {
    if cfg!(windows) {
        return true;
    }
    path.starts_with("/dev/cu.") && !path.contains("Bluetooth-Incoming-Port") && !path.contains("debug-console") && !path.contains("wlan-debug")
}

pub fn parallel_ports() -> Vec<PrinterTarget> {
    ["LPT1", "LPT2"]
        .iter()
        .filter(|l| super::parallel::exists(l))
        .map(|l| PrinterTarget { key: format!("lpt-{}", l.to_lowercase()), kind: Some(PrinterKind::Parallel), name: format!("Puerto paralelo {l}"), lpt: Some(l.to_string()), source: Some("parallel".into()), ..Default::default() })
        .collect()
}

fn slug(s: &str) -> String {
    let mut out: String = s.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).collect();
    out.truncate(48);
    out
}

/// Todo el descubrimiento (bloqueante: correr en spawn_blocking). Red opcional.
pub fn discover(network: bool) -> Vec<PrinterTarget> {
    let start = Instant::now();
    let mut v = system_printers();
    v.extend(serial_ports());
    v.extend(parallel_ports());
    if network {
        let mdns = std::thread::spawn(|| browse_mdns(MDNS_WINDOW));
        let scan = scan_tcp(start + Duration::from_secs(13));
        let mdns = mdns.join().unwrap_or_default();
        v.extend(merge_network(&scan, &mdns));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subredes_24_desde_las_interfaces() {
        let ifs = [
            (Ipv4Addr::new(192, 168, 1, 34), Ipv4Addr::new(255, 255, 255, 0)),
            (Ipv4Addr::new(10, 0, 5, 7), Ipv4Addr::new(255, 255, 0, 0)), // /16 → sólo su /24
            (Ipv4Addr::new(127, 0, 0, 1), Ipv4Addr::new(255, 0, 0, 0)),
            (Ipv4Addr::new(169, 254, 3, 3), Ipv4Addr::new(255, 255, 0, 0)),
            (Ipv4Addr::new(8, 8, 8, 8), Ipv4Addr::new(255, 255, 255, 0)), // pública: no se barre
            (Ipv4Addr::new(192, 168, 1, 200), Ipv4Addr::new(255, 255, 255, 0)), // misma /24
        ];
        assert_eq!(subnets_24(&ifs), vec![[10, 0, 5], [192, 168, 1]]);
        let hosts = hosts_of([192, 168, 1], &[Ipv4Addr::new(192, 168, 1, 34)]);
        assert_eq!(hosts.len(), 253);
        assert!(!hosts.contains(&Ipv4Addr::new(192, 168, 1, 34)));
    }

    #[test]
    fn mdns_y_barrido_en_una_fila() {
        let ip = Ipv4Addr::new(192, 168, 1, 50);
        let v = merge_network(&[ip, Ipv4Addr::new(192, 168, 1, 51)], &[(ip, "EPSON TM-T20III".into(), 9100)]);
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].name, "EPSON TM-T20III (192.168.1.50)");
        assert_eq!(v[0].source.as_deref(), Some("mdns"));
        assert_eq!(v[1].name, "Impresora de red 192.168.1.51");
        assert_eq!(v[1].key, "tcp-192.168.1.51");
    }

    #[test]
    fn filtros() {
        assert!(looks_virtual("Microsoft Print to PDF"));
        assert!(!looks_virtual("POS-80"));
        if !cfg!(windows) {
            assert!(keep_serial("/dev/cu.usbserial-1420"));
            assert!(!keep_serial("/dev/tty.usbserial-1420"));
            assert!(!keep_serial("/dev/cu.Bluetooth-Incoming-Port"));
        }
    }
}
