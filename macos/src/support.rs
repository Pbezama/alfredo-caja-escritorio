//! Soporte remoto en Mac (T10.8): la herramienta (la que elija F8) se baja a
//! `~/Library/Application Support/cl.asistentealfredo.caja.escritorio/soporte/`, se verifica su
//! SHA-256 contra la política y sólo entonces se monta (`hdiutil attach -nobrowse`) y se abre.
//! macOS exige dar a mano los permisos de Grabación de pantalla y Accesibilidad: la app abre esas
//! pantallas de Ajustes del Sistema.

use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

pub const SETTINGS_SCREEN_CAPTURE: &str = "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture";
pub const SETTINGS_ACCESSIBILITY: &str = "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility";
pub const SETTINGS_NOTIFICATIONS: &str = "x-apple.systempreferences:com.apple.Notifications-Settings.extension";
pub const SETTINGS_DATE_TIME: &str = "x-apple.systempreferences:com.apple.Date-Time-Settings.extension";
pub const SETTINGS_LOCAL_NETWORK: &str = "x-apple.systempreferences:com.apple.preference.security?Privacy_LocalNetwork";

/// Carpeta donde se baja la herramienta.
pub fn support_dir(home: &Path) -> PathBuf {
    home.join("Library/Application Support").join(crate::APP_ID).join("soporte")
}

/// ¿El archivo bajado es el que dice la política? (hex en minúsculas o mayúsculas).
pub fn sha256_matches(bytes: &[u8], expected_hex: &str) -> bool {
    let got: String = Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect();
    let want = expected_hex.trim().to_ascii_lowercase();
    want.len() == 64 && got == want
}

/// Nombre de archivo seguro para lo que se baja (sin rutas ni caracteres raros).
pub fn safe_file_name(url: &str) -> String {
    let last = url.split(['?', '#']).next().unwrap_or("").rsplit('/').next().unwrap_or("");
    let clean: String = last.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_')).collect();
    if clean.is_empty() || clean.starts_with('.') {
        "soporte.dmg".into()
    } else {
        clean
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_correcto_y_alterado() {
        let data = b"herramienta de soporte";
        let ok: String = Sha256::digest(data).iter().map(|b| format!("{b:02x}")).collect();
        assert!(sha256_matches(data, &ok));
        assert!(sha256_matches(data, &ok.to_uppercase()));
        assert!(!sha256_matches(b"herramienta alterada", &ok));
        assert!(!sha256_matches(data, "corto"));
    }

    #[test]
    fn rutas() {
        assert_eq!(support_dir(Path::new("/Users/caja")), PathBuf::from("/Users/caja/Library/Application Support/cl.asistentealfredo.caja.escritorio/soporte"));
        assert_eq!(safe_file_name("https://x.com/dl/Soporte%20Mac.dmg?v=2"), "Soporte20Mac.dmg");
        assert_eq!(safe_file_name("https://x.com/../../etc/passwd"), "passwd");
        assert_eq!(safe_file_name("https://x.com/"), "soporte.dmg");
    }
}
