//! Soporte remoto (T9.9) con RustDesk (gratis, sin licencias; decisión de F8/F9). «Iniciar
//! soporte remoto» baja el ejecutable portable de RustDesk a la carpeta de soporte si no está,
//! verifica su SHA-256 (si no coincide, no lo ejecuta), lo abre y muestra el ID que hay que
//! dictarle a soporte (lo lee de la configuración que RustDesk escribe al arrancar).
//! No se incluye TeamViewer QS: su uso comercial exige licencia.

use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// RustDesk 1.5.0 (github.com/rustdesk/rustdesk/releases). Se puede cambiar sin sacar
/// versión con el flag remoto `support.remote_tool` = {url, sha256} o en config.json.
#[cfg(windows)]
pub const DEFAULT_URL: &str = "https://github.com/rustdesk/rustdesk/releases/download/1.5.0/rustdesk-1.5.0-x86_64.exe";
#[cfg(windows)]
pub const DEFAULT_SHA256: &str = "8555777215510d83d2d61c9dc984e4fcc838bd7e79f9d18a42585431f5e8bb47";
#[cfg(target_os = "macos")]
pub const DEFAULT_URL: &str = "https://github.com/rustdesk/rustdesk/releases/download/1.5.0/rustdesk-1.5.0-aarch64.dmg";
#[cfg(target_os = "macos")]
pub const DEFAULT_SHA256: &str = "3929b0a4321e7d0f561a317059798be882d65decc59681eb77061f8efe56fbf4";
#[cfg(not(any(windows, target_os = "macos")))]
pub const DEFAULT_URL: &str = "";
#[cfg(not(any(windows, target_os = "macos")))]
pub const DEFAULT_SHA256: &str = "";

pub const DOWNLOAD_PAGE: &str = "https://rustdesk.com/es/";

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

pub fn file_matches(path: &Path, expected: &str) -> bool {
    std::fs::read(path).map(|b| sha256_hex(&b).eq_ignore_ascii_case(expected.trim())).unwrap_or(false)
}

/// El ID de RustDesk sale de su RustDesk.toml («id = '123456789'»).
pub fn parse_rustdesk_id(toml: &str) -> Option<String> {
    toml.lines().find_map(|l| {
        let l = l.trim();
        let v = l.strip_prefix("id")?.trim_start().strip_prefix('=')?.trim().trim_matches(|c| c == '\'' || c == '"');
        (v.len() >= 6 && v.chars().all(|c| c.is_ascii_digit())).then(|| v.to_string())
    })
}

/// «123456789» → «123 456 789» para dictarlo.
pub fn format_id(id: &str) -> String {
    id.as_bytes().chunks(3).map(|c| String::from_utf8_lossy(c).to_string()).collect::<Vec<_>>().join(" ")
}

fn rustdesk_config_paths() -> Vec<PathBuf> {
    let mut v = vec![];
    if let Some(d) = std::env::var_os("APPDATA") {
        v.push(PathBuf::from(d).join("RustDesk").join("config").join("RustDesk.toml"));
    }
    if let Some(h) = std::env::var_os("HOME") {
        v.push(PathBuf::from(&h).join("Library/Preferences/com.carriez.RustDesk/RustDesk.toml"));
    }
    v
}

pub fn read_rustdesk_id() -> Option<String> {
    rustdesk_config_paths().iter().find_map(|p| std::fs::read_to_string(p).ok().and_then(|s| parse_rustdesk_id(&s)))
}

/// Texto de «Copiar diagnóstico».
pub fn diagnostics_text(lines: &[(&str, String)], log_tail: &str) -> String {
    let mut s = String::from("Alfredo Caja · diagnóstico\n");
    for (k, v) in lines {
        s.push_str(&format!("{k}: {v}\n"));
    }
    if !log_tail.is_empty() {
        s.push_str("\nÚltimas líneas del registro:\n");
        s.push_str(log_tail);
    }
    s
}

/// Últimas `n` líneas de un archivo de registro.
pub fn tail(path: &Path, n: usize) -> String {
    let Ok(s) = std::fs::read_to_string(path) else { return String::new() };
    let lines: Vec<&str> = s.lines().collect();
    lines[lines.len().saturating_sub(n)..].join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn verifica_el_hash() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(b"rustdesk falso").unwrap();
        let good = sha256_hex(b"rustdesk falso");
        assert!(file_matches(f.path(), &good));
        assert!(file_matches(f.path(), &good.to_uppercase()));
        // Un byte distinto (archivo alterado) no pasa.
        assert!(!file_matches(f.path(), &sha256_hex(b"rustdesk falsO")));
        assert!(!file_matches(Path::new("/no/existe"), &good));
        assert_eq!(DEFAULT_SHA256.len() % 64, 0);
    }

    #[test]
    fn id_de_rustdesk() {
        assert_eq!(parse_rustdesk_id("enc_id = 'x'\nid = '123456789'\npassword = ''\n").as_deref(), Some("123456789"));
        assert_eq!(parse_rustdesk_id("id = \"1234567890\""), Some("1234567890".into()));
        assert_eq!(parse_rustdesk_id("id = ''"), None);
        assert_eq!(format_id("123456789"), "123 456 789");
        assert_eq!(format_id("1234567890"), "123 456 789 0");
    }

    #[test]
    fn diagnostico() {
        let t = diagnostics_text(&[("Versión", "0.1.0".into()), ("Equipo", "win-abc".into())], "linea 1\nlinea 2");
        assert!(t.contains("Versión: 0.1.0\n"));
        assert!(t.ends_with("linea 2"));
    }
}
