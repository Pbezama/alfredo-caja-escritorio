//! Identidad estable del equipo (RF9.13): `win-` / `mac-` + sha256(id de la máquina +
//! ":alfredo-escritorio")[0..16]. No expone el MachineGuid (ni el IOPlatformUUID) crudo
//! y sobrevive a desinstalar y reinstalar.

use sha2::{Digest, Sha256};

pub fn prefix() -> &'static str {
    if cfg!(target_os = "windows") {
        "win-"
    } else if cfg!(target_os = "macos") {
        "mac-"
    } else {
        "lnx-"
    }
}

pub fn derive_device_id(prefix: &str, machine_id: &str) -> String {
    let mut h = Sha256::new();
    h.update(machine_id.trim().as_bytes());
    h.update(b":alfredo-escritorio");
    let hex = hex::encode(h.finalize());
    format!("{prefix}{}", &hex[..16])
}

/// Id de este equipo. Si el sistema no da id de máquina (raro), uno aleatorio persistido
/// por quien llama en device.json.
pub fn machine_device_id() -> Option<String> {
    machine_uid::get().ok().filter(|s| !s.trim().is_empty()).map(|m| derive_device_id(prefix(), &m))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deriva_estable_y_sin_exponer_el_id() {
        let a = derive_device_id("win-", "4c4c4544-0042-3510-8051-b4c04f4a4d32");
        let b = derive_device_id("win-", " 4c4c4544-0042-3510-8051-b4c04f4a4d32\n");
        assert_eq!(a, b);
        assert_eq!(a.len(), 4 + 16);
        assert!(a.starts_with("win-"));
        assert!(!a.contains("4c4c4544"));
        assert_ne!(a, derive_device_id("win-", "otra-maquina"));
        assert!(derive_device_id("mac-", "X").starts_with("mac-"));
    }
}
