//! Identidad estable del Mac (T10.7): `mac-` + los primeros 16 hex de
//! `sha256(IOPlatformUUID + ":alfredo-escritorio")`. Sobrevive a reinstalar la app y no expone el
//! valor crudo del equipo.

use sha2::{Digest, Sha256};

pub fn device_id_from_platform_uuid(platform_uuid: &str) -> Option<String> {
    let u = platform_uuid.trim().to_ascii_uppercase();
    if u.len() < 8 || !u.chars().all(|c| c.is_ascii_hexdigit() || c == '-') {
        return None;
    }
    let digest = Sha256::digest(format!("{u}:alfredo-escritorio").as_bytes());
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    Some(format!("mac-{}", &hex[..16]))
}

/// `"IOPlatformUUID" = "ABCD-…"` en la salida de `ioreg -rd1 -c IOPlatformExpertDevice`.
pub fn parse_ioreg_platform_uuid(output: &str) -> Option<String> {
    let line = output.lines().find(|l| l.contains("\"IOPlatformUUID\""))?;
    let value = line.split('=').nth(1)?.trim().trim_matches('"');
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

#[cfg(target_os = "macos")]
pub fn device_id() -> Option<String> {
    let out = std::process::Command::new("/usr/sbin/ioreg").args(["-rd1", "-c", "IOPlatformExpertDevice"]).output().ok()?;
    device_id_from_platform_uuid(&parse_ioreg_platform_uuid(&String::from_utf8_lossy(&out.stdout))?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deriva_el_id_sin_exponer_el_uuid() {
        let id = device_id_from_platform_uuid("1F2E3D4C-5B6A-7980-A1B2-C3D4E5F60718").unwrap();
        assert!(id.starts_with("mac-"));
        assert_eq!(id.len(), 20);
        assert!(!id.contains("1F2E"));
        // Mismo equipo, mismo id (mayúsculas o minúsculas da igual).
        assert_eq!(device_id_from_platform_uuid("1f2e3d4c-5b6a-7980-a1b2-c3d4e5f60718").unwrap(), id);
        assert_eq!(device_id_from_platform_uuid(""), None);
        assert_eq!(device_id_from_platform_uuid("no es un uuid"), None);
    }

    #[test]
    fn lee_ioreg() {
        let out = "+-o J414cAP  <class IOPlatformExpertDevice>\n    {\n      \"IOPlatformUUID\" = \"1F2E3D4C-5B6A-7980-A1B2-C3D4E5F60718\"\n    }\n";
        assert_eq!(parse_ioreg_platform_uuid(out).as_deref(), Some("1F2E3D4C-5B6A-7980-A1B2-C3D4E5F60718"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn este_mac_tiene_id_estable() {
        let a = device_id().expect("id");
        assert_eq!(device_id().unwrap(), a);
    }
}
