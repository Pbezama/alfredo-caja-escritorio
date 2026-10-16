//! Puerto paralelo (LPT1…): `CreateFileW("\\\\.\\LPT1", GENERIC_WRITE)` + `WriteFile`.

use super::PrintError;

/// `LPT1` → `\\.\LPT1`.
pub fn device_path(lpt: &str) -> Result<String, PrintError> {
    let l = lpt.trim().to_ascii_uppercase();
    let ok = l.len() == 4 && l.starts_with("LPT") && l.as_bytes()[3].is_ascii_digit() && l.as_bytes()[3] != b'0';
    if !ok {
        return Err(PrintError::new("bad_request", format!("Puerto paralelo inválido: {lpt}")));
    }
    Ok(format!(r"\\.\{l}"))
}

#[cfg(windows)]
pub fn send(lpt: &str, bytes: &[u8]) -> Result<(), PrintError> {
    use std::io::Write;
    let path = device_path(lpt)?;
    // OpenOptions con write en Windows usa CreateFileW(GENERIC_WRITE, OPEN_EXISTING).
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .map_err(|e| PrintError::new("offline", format!("No se pudo abrir {lpt}: {e}")))?;
    f.write_all(bytes).map_err(|e| PrintError::new("error", format!("Error escribiendo en {lpt}: {e}")))?;
    f.flush().ok();
    Ok(())
}

#[cfg(not(windows))]
pub fn send(lpt: &str, _bytes: &[u8]) -> Result<(), PrintError> {
    device_path(lpt)?;
    Err(PrintError::new("bad_request", "El puerto paralelo sólo existe en Windows"))
}

/// ¿Existe el LPT? (para el descubrimiento).
#[cfg(windows)]
pub fn exists(lpt: &str) -> bool {
    device_path(lpt).ok().map(|p| std::fs::OpenOptions::new().write(true).open(p).is_ok()).unwrap_or(false)
}

#[cfg(not(windows))]
pub fn exists(_lpt: &str) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ruta_del_dispositivo() {
        assert_eq!(device_path("lpt1").unwrap(), r"\\.\LPT1");
        assert!(device_path("LPT0").is_err());
        assert!(device_path("COM1").is_err());
        assert!(device_path("..\\x").is_err());
    }
}
