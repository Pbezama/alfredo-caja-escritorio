//! Puertos serie del Mac (T10.3–T10.4). Se usan los `/dev/cu.*` (no los `/dev/tty.*`, que esperan
//! la señal DCD y se quedan colgados con un adaptador USB-serie). La escritura con baudios, paridad,
//! *stop bits*, bits de datos y control de flujo es el mismo código de F9 (crate `serialport`).

/// Puertos que no son impresoras: el Bluetooth entrante y la consola de depuración.
const DESCARTAR: &[&str] = &["Bluetooth-Incoming-Port", "debug-console", "wlan-debug"];

/// Filtra la lista de `/dev`: sólo `cu.*` útiles, ordenados (los USB-serie primero).
pub fn filter_cu_ports<I: IntoIterator<Item = String>>(entries: I) -> Vec<String> {
    let mut out: Vec<String> = entries
        .into_iter()
        .filter_map(|e| {
            let name = e.rsplit('/').next()?.to_string();
            let rest = name.strip_prefix("cu.")?;
            if rest.is_empty() || DESCARTAR.iter().any(|d| rest.eq_ignore_ascii_case(d)) {
                return None;
            }
            Some(format!("/dev/{name}"))
        })
        .collect();
    out.sort_by_key(|p| {
        let usb = p.contains("usbserial") || p.contains("usbmodem") || p.contains("SLAB") || p.contains("wchusbserial");
        (!usb, p.clone())
    });
    out.dedup();
    out
}

/// Los puertos `/dev/cu.*` del Mac.
pub fn list_ports() -> Vec<String> {
    match std::fs::read_dir("/dev") {
        Ok(rd) => filter_cu_ports(rd.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned())),
        Err(_) => vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solo_cu_y_sin_bluetooth_ni_consola() {
        let entries = ["tty.usbserial-1410", "cu.usbserial-1410", "cu.Bluetooth-Incoming-Port", "cu.debug-console", "cu.wchusbserial220", "null", "cu."]
            .map(String::from);
        assert_eq!(filter_cu_ports(entries), vec!["/dev/cu.usbserial-1410", "/dev/cu.wchusbserial220"]);
    }

    #[test]
    fn usb_serie_primero() {
        let entries = ["cu.MALS", "cu.usbmodem14101"].map(String::from);
        assert_eq!(filter_cu_ports(entries), vec!["/dev/cu.usbmodem14101", "/dev/cu.MALS"]);
    }
}
