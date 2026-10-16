//! Impresión nativa (T9.4): spooler RAW (Windows) o cola CUPS raw (macOS, base para F10),
//! TCP 9100, serie y paralelo. Los bytes ya vienen en ESC/POS desde la web (@repo/escpos).
//!
//! Un mutex por impresora: dos documentos a la misma térmica no se intercalan. Las copias
//! se mandan N veces. `busy` se reintenta una vez a los 2 s; el resto lo informa la web
//! al backend (`POST /api/tickets/:jobId/result`) y ahí queda en print_jobs.

pub mod discover;
pub mod parallel;
pub mod serial;
pub mod spooler;
pub mod status;
pub mod tcp;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PrinterKind {
    Spooler,
    Tcp,
    Serial,
    Parallel,
    Cups,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct PrinterTarget {
    pub key: String,
    pub kind: Option<PrinterKind>,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spooler_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub driver: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub com: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub baud: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data_bits: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parity: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_bits: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flow: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lpt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PrinterConfig {
    #[serde(flatten)]
    pub target: PrinterTarget,
    pub width: u8,
    pub code_page: String,
    pub docs: Vec<String>,
    pub copies: u8,
    pub cut: bool,
    pub drawer: bool,
}

impl PrinterConfig {
    #[cfg(test)]
    pub fn new_test(key: &str, kind: PrinterKind) -> Self {
        Self {
            target: PrinterTarget { key: key.into(), kind: Some(kind), name: key.into(), host: Some("127.0.0.1".into()), port: Some(9100), ..Default::default() },
            width: 42,
            code_page: "auto".into(),
            docs: vec!["bill".into()],
            copies: 1,
            cut: true,
            drawer: false,
        }
    }

    /// Valida lo que llega de la web antes de guardarlo.
    pub fn validate(&self) -> Result<(), PrintError> {
        let bad = |m: &str| Err(PrintError::new("bad_request", m));
        let t = &self.target;
        if t.key.is_empty() || t.key.len() > 64 {
            return bad("Falta el identificador de la impresora");
        }
        if t.name.trim().is_empty() || t.name.len() > 80 {
            return bad("Falta el nombre de la impresora");
        }
        if ![32, 42, 48].contains(&self.width) {
            return bad("Ancho inválido (32, 42 o 48 columnas)");
        }
        if !(1..=3).contains(&self.copies) {
            return bad("Copias inválidas (1 a 3)");
        }
        match t.kind {
            Some(PrinterKind::Spooler) | Some(PrinterKind::Cups) if t.spooler_name.as_deref().unwrap_or("").is_empty() => bad("Falta el nombre de la impresora del sistema"),
            Some(PrinterKind::Tcp) if t.host.as_deref().unwrap_or("").is_empty() => bad("Falta la IP de la impresora"),
            Some(PrinterKind::Serial) if t.com.as_deref().unwrap_or("").is_empty() => bad("Falta el puerto serie"),
            None => bad("Falta el tipo de impresora"),
            _ => Ok(()),
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PrintError {
    pub code: String,
    pub message: String,
}

impl PrintError {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self { code: code.into(), message: message.into() }
    }
}

impl std::fmt::Display for PrintError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

/// Mutex por impresora (clave = key).
#[derive(Default, Clone)]
pub struct PrintLocks(Arc<std::sync::Mutex<HashMap<String, Arc<Mutex<()>>>>>);

impl PrintLocks {
    pub fn for_key(&self, key: &str) -> Arc<Mutex<()>> {
        let mut m = self.0.lock().unwrap();
        m.entry(key.to_string()).or_insert_with(|| Arc::new(Mutex::new(()))).clone()
    }
}

/// Envío de una vez por el transporte de la impresora (bloqueante: va en spawn_blocking).
pub fn send_once(cfg: &PrinterTarget, bytes: &[u8]) -> Result<(), PrintError> {
    match cfg.kind {
        Some(PrinterKind::Tcp) => tcp::send(cfg.host.as_deref().unwrap_or(""), cfg.port.unwrap_or(9100), bytes),
        Some(PrinterKind::Serial) => serial::send(&serial::SerialSettings::from_target(cfg), bytes),
        Some(PrinterKind::Spooler) | Some(PrinterKind::Cups) => spooler::send(cfg.spooler_name.as_deref().unwrap_or(""), bytes),
        Some(PrinterKind::Parallel) => parallel::send(cfg.lpt.as_deref().unwrap_or("LPT1"), bytes),
        None => Err(PrintError::new("bad_request", "Impresora sin tipo")),
    }
}

/// Imprime `copies` veces con el mutex de la impresora tomado. `busy` reintenta una vez.
pub async fn print(locks: &PrintLocks, cfg: PrinterTarget, bytes: Vec<u8>, copies: u8) -> Result<(), PrintError> {
    if bytes.is_empty() {
        return Err(PrintError::new("bad_request", "Documento vacío"));
    }
    if bytes.len() > 2 * 1024 * 1024 {
        return Err(PrintError::new("bad_request", "Documento demasiado grande"));
    }
    let lock = locks.for_key(&cfg.key);
    let _guard = lock.lock().await;
    let copies = copies.clamp(1, 5);
    for _ in 0..copies {
        let mut attempt = 0;
        loop {
            let c = cfg.clone();
            let b = bytes.clone();
            let r = tokio::task::spawn_blocking(move || send_once(&c, &b))
                .await
                .map_err(|e| PrintError::new("error", e.to_string()))?;
            match r {
                Err(e) if e.code == "busy" && attempt == 0 => {
                    attempt += 1;
                    tokio::time::sleep(Duration::from_secs(2)).await;
                }
                other => {
                    other?;
                    break;
                }
            }
        }
    }
    Ok(())
}

/// Estado de una impresora (DLE EOT en tcp/serie, estado del spooler en Windows).
pub async fn printer_status(cfg: PrinterTarget) -> String {
    tokio::task::spawn_blocking(move || match cfg.kind {
        Some(PrinterKind::Tcp) => tcp::status(cfg.host.as_deref().unwrap_or(""), cfg.port.unwrap_or(9100)),
        Some(PrinterKind::Serial) => serial::status(&serial::SerialSettings::from_target(&cfg)),
        Some(PrinterKind::Spooler) | Some(PrinterKind::Cups) => spooler::status(cfg.spooler_name.as_deref().unwrap_or("")),
        _ => "unknown".to_string(),
    })
    .await
    .unwrap_or_else(|_| "unknown".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::net::TcpListener;

    #[test]
    fn valida_la_configuracion() {
        let mut c = PrinterConfig::new_test("p", PrinterKind::Tcp);
        assert!(c.validate().is_ok());
        c.width = 40;
        assert_eq!(c.validate().unwrap_err().code, "bad_request");
        c.width = 32;
        c.target.host = None;
        assert!(c.validate().is_err());
        let mut s = PrinterConfig::new_test("s", PrinterKind::Spooler);
        assert!(s.validate().is_err());
        s.target.spooler_name = Some("POS-80".into());
        assert!(s.validate().is_ok());
    }

    #[test]
    fn json_igual_al_contrato_de_la_web() {
        let json = r#"{"key":"k1","kind":"serial","name":"Térmica COM3","com":"COM3","baud":9600,"dataBits":8,"parity":"none","stopBits":1,"flow":"none","width":32,"codePage":"cp858","docs":["bill","order_ticket"],"copies":2,"cut":true,"drawer":true}"#;
        let c: PrinterConfig = serde_json::from_str(json).unwrap();
        assert_eq!(c.target.kind, Some(PrinterKind::Serial));
        assert_eq!(c.target.com.as_deref(), Some("COM3"));
        assert_eq!(c.copies, 2);
        let back = serde_json::to_value(&c).unwrap();
        assert_eq!(back["dataBits"], 8);
        assert_eq!(back["codePage"], "cp858");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn copias_y_sin_intercalar() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let mut chunks = vec![];
            for _ in 0..4 {
                let (mut s, _) = listener.accept().unwrap();
                let mut buf = vec![];
                s.read_to_end(&mut buf).unwrap();
                chunks.push(buf);
            }
            chunks
        });
        let mut t = PrinterConfig::new_test("p", PrinterKind::Tcp).target;
        t.port = Some(port);
        let locks = PrintLocks::default();
        let a = print(&locks, t.clone(), vec![b'A'; 3000], 2);
        let b = print(&locks, t.clone(), vec![b'B'; 3000], 2);
        let (ra, rb) = tokio::join!(a, b);
        ra.unwrap();
        rb.unwrap();
        let chunks = server.join().unwrap();
        assert_eq!(chunks.len(), 4);
        for c in &chunks {
            assert_eq!(c.len(), 3000);
            assert!(c.iter().all(|x| *x == c[0]), "una conexión mezcló dos documentos");
        }
        // Las dos copias de un documento salen seguidas.
        assert_eq!(chunks[0][0], chunks[1][0]);
        assert_eq!(chunks[2][0], chunks[3][0]);
    }
}
