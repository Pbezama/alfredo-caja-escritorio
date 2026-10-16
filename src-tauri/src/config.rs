//! config.json del escritorio (en la carpeta de datos de la app):
//! origen del local, impresoras, inicio automático, kiosco, pantalla encendida y canal.

use crate::printing::PrinterConfig;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct AppConfig {
    /// `https://{slug}.app.asistentealfredo.cl`; None = mostrar setup.html.
    pub origin: Option<String>,
    /// Última URL del local (para volver tras offline.html).
    pub last_url: Option<String>,
    pub printers: Vec<PrinterConfig>,
    pub autostart: bool,
    pub kiosk: bool,
    /// Ya se mostró el globo «Alfredo sigue recibiendo pedidos en la bandeja».
    pub tray_hint_shown: bool,
    /// Canal del updater: beta (sin QA en Windows real ni Authenticode) o stable.
    pub channel: String,
    /// Herramienta de soporte remoto (RustDesk portable): URL y SHA-256 esperados.
    pub support_tool_url: Option<String>,
    pub support_tool_sha256: Option<String>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            origin: None,
            last_url: None,
            printers: vec![],
            autostart: true,
            kiosk: false,
            tray_hint_shown: false,
            channel: "beta".into(),
            support_tool_url: None,
            support_tool_sha256: None,
        }
    }
}

pub fn path_in(dir: &Path) -> PathBuf {
    dir.join("config.json")
}

pub fn load(dir: &Path) -> AppConfig {
    fs::read_to_string(path_in(dir))
        .ok()
        .and_then(|s| serde_json::from_str::<AppConfig>(&s).ok())
        .unwrap_or_default()
}

/// Escritura atómica (tmp + rename): un corte de luz no deja el archivo a medias.
pub fn save(dir: &Path, cfg: &AppConfig) -> std::io::Result<()> {
    fs::create_dir_all(dir)?;
    let tmp = dir.join("config.json.tmp");
    fs::write(&tmp, serde_json::to_vec_pretty(cfg).unwrap_or_default())?;
    fs::rename(tmp, path_in(dir))
}

/// La última URL sólo se recuerda si es del mismo local.
pub fn remember_url(cfg: &mut AppConfig, url: &str) {
    if let Some(origin) = &cfg.origin {
        if url.starts_with(origin.as_str()) {
            cfg.last_url = Some(url.to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::printing::PrinterKind;

    #[test]
    fn ida_y_vuelta_y_defaults() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(load(dir.path()), AppConfig::default());
        let mut c = AppConfig { origin: Some("https://pausan.app.asistentealfredo.cl".into()), ..Default::default() };
        c.printers.push(PrinterConfig::new_test("p1", PrinterKind::Tcp));
        save(dir.path(), &c).unwrap();
        assert_eq!(load(dir.path()), c);
        // Un archivo viejo sin campos nuevos carga con defaults.
        fs::write(path_in(dir.path()), r#"{"origin":"https://x.app.asistentealfredo.cl"}"#).unwrap();
        let old = load(dir.path());
        assert!(old.autostart);
        assert_eq!(old.channel, "beta");
        // Un archivo roto no tumba el escritorio.
        fs::write(path_in(dir.path()), "{roto").unwrap();
        assert_eq!(load(dir.path()), AppConfig::default());
    }

    #[test]
    fn ultima_url_del_mismo_local() {
        let mut c = AppConfig { origin: Some("https://pausan.app.asistentealfredo.cl".into()), ..Default::default() };
        remember_url(&mut c, "https://pausan.app.asistentealfredo.cl/kitchen");
        remember_url(&mut c, "https://maps.google.com/");
        assert_eq!(c.last_url.as_deref(), Some("https://pausan.app.asistentealfredo.cl/kitchen"));
    }
}
