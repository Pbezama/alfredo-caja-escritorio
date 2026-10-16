//! Impresión por CUPS en macOS (T10.3–T10.4).
//!
//! - ESC/POS a una térmica: `lp -d <cola> -o raw` con los bytes por stdin (CUPS no los filtra).
//! - PDF o A4 a una cola con driver (la láser del local, una AirPrint que macOS ya tiene):
//!   `lp -d <cola>` con el archivo.
//! - Estado: `lpstat -l -p <cola>` (estado y motivos) y la lista de colas con `lpstat -v`.
//! - Una térmica USB sin cola: `lpadmin -p Alfredo_<modelo> -E -v <uri> -m raw` con privilegios de
//!   administrador (`osascript … with administrator privileges`, que muestra el diálogo de macOS).
//!
//! Se usan los binarios de CUPS que trae macOS en vez de libcups por FFI: mismo resultado, sin
//! enlazar contra una API que Apple marca como obsoleta, y los argumentos se prueban como texto.

use std::fmt;

/// Estado de una impresora, con los mismos nombres que la Caja (`PrinterStatus`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrinterState {
    Ok,
    NoPaper,
    CoverOpen,
    Offline,
    Busy,
    Unknown,
}

impl PrinterState {
    pub fn as_str(&self) -> &'static str {
        match self {
            PrinterState::Ok => "ok",
            PrinterState::NoPaper => "no_paper",
            PrinterState::CoverOpen => "cover_open",
            PrinterState::Offline => "offline",
            PrinterState::Busy => "busy",
            PrinterState::Unknown => "unknown",
        }
    }
}

impl fmt::Display for PrinterState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Una cola de CUPS que se puede elegir en «Este equipo».
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CupsQueue {
    pub name: String,
    /// URI del dispositivo (`usb://…`, `ipp://…`, `dnssd://…`, `socket://…`).
    pub device_uri: String,
}

impl CupsQueue {
    /// Las térmicas suelen ir por USB o socket; las de `ipp`/`dnssd` son AirPrint (PDF, no ESC/POS).
    pub fn looks_like_airprint(&self) -> bool {
        let u = self.device_uri.to_ascii_lowercase();
        u.starts_with("ipp") || u.starts_with("dnssd") || u.contains("_ipp")
    }
}

/// Nombre válido de cola: CUPS acepta letras, números, `_`, `-` y `.` (sin espacios ni `/`, `#`).
pub fn sanitize_queue_name(model: &str) -> String {
    let base: String = model
        .chars()
        .map(|c| {
            let c = deaccent(c);
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let trimmed = base.trim_matches('_');
    let name = if trimmed.is_empty() { "Termica".to_string() } else { trimmed.to_string() };
    let mut out = format!("Alfredo_{name}");
    out.truncate(127);
    out
}

fn deaccent(c: char) -> char {
    match c {
        'á' | 'à' | 'ä' | 'â' => 'a',
        'é' | 'è' | 'ë' | 'ê' => 'e',
        'í' | 'ì' | 'ï' | 'î' => 'i',
        'ó' | 'ò' | 'ö' | 'ô' => 'o',
        'ú' | 'ù' | 'ü' | 'û' => 'u',
        'ñ' => 'n',
        'Á' | 'À' | 'Ä' | 'Â' => 'A',
        'É' | 'È' | 'Ë' | 'Ê' => 'E',
        'Í' | 'Ì' | 'Ï' | 'Î' => 'I',
        'Ó' | 'Ò' | 'Ö' | 'Ô' => 'O',
        'Ú' | 'Ù' | 'Ü' | 'Û' => 'U',
        'Ñ' => 'N',
        other => other,
    }
}

/// Argumentos de `lp` para mandar bytes ESC/POS sin filtro (los bytes van por stdin).
pub fn lp_raw_args(queue: &str, copies: u8, title: &str) -> Vec<String> {
    vec![
        "-d".into(),
        queue.into(),
        "-n".into(),
        copies.clamp(1, 9).to_string(),
        "-o".into(),
        "raw".into(),
        "-t".into(),
        title.into(),
    ]
}

/// Argumentos de `lp` para un PDF a una cola con driver (A4 por defecto).
pub fn lp_pdf_args(queue: &str, file: &str, copies: u8, media: Option<&str>) -> Vec<String> {
    let mut a = vec!["-d".into(), queue.into(), "-n".into(), copies.clamp(1, 9).to_string()];
    a.push("-o".into());
    a.push(format!("media={}", media.unwrap_or("A4")));
    a.push("-o".into());
    a.push("fit-to-page".into());
    a.push(file.into());
    a
}

/// `request id is POS80-123 (1 file(s))` → `POS80-123`.
pub fn parse_request_id(stdout: &str) -> Option<String> {
    let line = stdout.lines().find(|l| l.contains("request id is"))?;
    let rest = line.split("request id is").nth(1)?.trim();
    let id = rest.split_whitespace().next()?;
    if id.is_empty() {
        None
    } else {
        Some(id.to_string())
    }
}

/// Estado a partir de `lpstat -l -p <cola>`: «disabled» = offline; motivos `media-empty` →
/// sin papel, `cover-open`/`door-open` → tapa, `offline-report`/`connecting-to-device` →
/// offline, `paused` → offline; «now printing» → ocupada. Sin motivos conocidos → ok.
pub fn parse_lpstat_state(output: &str) -> PrinterState {
    let lower = output.to_ascii_lowercase();
    let reasons = lower
        .lines()
        .find_map(|l| l.trim().strip_prefix("alerts:").map(|r| r.trim().to_string()))
        .unwrap_or_default();
    let has = |r: &str| reasons.split([',', ' ']).any(|x| x.trim().starts_with(r));
    if has("media-empty") || has("media-needed") {
        return PrinterState::NoPaper;
    }
    if has("cover-open") || has("door-open") {
        return PrinterState::CoverOpen;
    }
    if has("offline") || has("connecting-to-device") || has("paused") || lower.contains(" disabled") {
        return PrinterState::Offline;
    }
    if lower.contains("now printing") {
        return PrinterState::Busy;
    }
    if lower.contains("is idle") || lower.contains("enabled") {
        return PrinterState::Ok;
    }
    PrinterState::Unknown
}

/// `lpstat -v`: `device for POS80: usb://Xprinter/XP-80?serial=123` → colas con su URI.
/// (En español macOS imprime «dispositivo para POS80: …».)
pub fn parse_lpstat_devices(output: &str) -> Vec<CupsQueue> {
    output
        .lines()
        .filter_map(|l| {
            let l = l.trim();
            let rest = l.strip_prefix("device for ").or_else(|| l.strip_prefix("dispositivo para "))?;
            let (name, uri) = rest.split_once(':')?;
            Some(CupsQueue { name: name.trim().to_string(), device_uri: uri.trim().to_string() })
        })
        .collect()
}

/// Comando de shell (para `osascript … with administrator privileges`) que crea una cola *raw*
/// para una térmica USB. Todo va entre comillas simples escapadas: nombres con espacios y
/// tildes no rompen el comando.
pub fn lpadmin_raw_queue_command(queue: &str, device_uri: &str) -> String {
    format!(
        "/usr/sbin/lpadmin -p {} -E -v {} -m raw && /usr/sbin/cupsenable {} && /usr/sbin/cupsaccept {}",
        shell_quote(queue),
        shell_quote(device_uri),
        shell_quote(queue),
        shell_quote(queue)
    )
}

/// AppleScript que corre `command` con el diálogo de administrador de macOS.
pub fn osascript_admin(command: &str) -> String {
    let escaped = command.replace('\\', "\\\\").replace('"', "\\\"");
    format!("do shell script \"{escaped}\" with administrator privileges")
}

pub fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

#[cfg(target_os = "macos")]
mod sys {
    use super::*;
    use std::io::Write;
    use std::process::{Command, Stdio};

    /// Manda bytes ESC/POS a una cola por `lp -o raw`; devuelve el id del trabajo.
    pub fn print_raw(queue: &str, bytes: &[u8], copies: u8) -> Result<String, String> {
        let mut child = Command::new("/usr/bin/lp")
            .args(lp_raw_args(queue, copies, "Alfredo"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| e.to_string())?;
        child.stdin.as_mut().ok_or("sin stdin")?.write_all(bytes).map_err(|e| e.to_string())?;
        let out = child.wait_with_output().map_err(|e| e.to_string())?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
        }
        parse_request_id(&String::from_utf8_lossy(&out.stdout)).ok_or_else(|| "lp no devolvió el id del trabajo".into())
    }

    pub fn print_pdf(queue: &str, file: &str, copies: u8, media: Option<&str>) -> Result<String, String> {
        let out = Command::new("/usr/bin/lp").args(lp_pdf_args(queue, file, copies, media)).output().map_err(|e| e.to_string())?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
        }
        parse_request_id(&String::from_utf8_lossy(&out.stdout)).ok_or_else(|| "lp no devolvió el id del trabajo".into())
    }

    pub fn state(queue: &str) -> PrinterState {
        match Command::new("/usr/bin/lpstat").args(["-l", "-p", queue]).env("LANG", "C").output() {
            Ok(o) if o.status.success() => parse_lpstat_state(&String::from_utf8_lossy(&o.stdout)),
            _ => PrinterState::Offline,
        }
    }

    pub fn queues() -> Vec<CupsQueue> {
        match Command::new("/usr/bin/lpstat").arg("-v").env("LANG", "C").output() {
            Ok(o) => parse_lpstat_devices(&String::from_utf8_lossy(&o.stdout)),
            Err(_) => vec![],
        }
    }

    /// Cancela un trabajo retenido (no imprimir 10 minutos tarde).
    pub fn cancel(job_id: &str) {
        let _ = Command::new("/usr/bin/cancel").arg(job_id).output();
    }

    /// Crea la cola *raw* con el diálogo de administrador de macOS.
    pub fn create_raw_queue(model: &str, device_uri: &str) -> Result<String, String> {
        let queue = sanitize_queue_name(model);
        let script = osascript_admin(&lpadmin_raw_queue_command(&queue, device_uri));
        let out = Command::new("/usr/bin/osascript").args(["-e", &script]).output().map_err(|e| e.to_string())?;
        if out.status.success() {
            Ok(queue)
        } else {
            Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
        }
    }
}

#[cfg(target_os = "macos")]
pub use sys::{cancel, create_raw_queue, print_pdf, print_raw, queues, state};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nombres_de_cola_sin_espacios_ni_tildes() {
        assert_eq!(sanitize_queue_name("XP-80 Térmica Ñuñoa"), "Alfredo_XP-80_Termica_Nunoa");
        assert_eq!(sanitize_queue_name("   "), "Alfredo_Termica");
        assert_eq!(sanitize_queue_name("EPSON TM/T20#III"), "Alfredo_EPSON_TM_T20_III");
    }

    #[test]
    fn argumentos_de_lp() {
        assert_eq!(lp_raw_args("POS 80", 2, "Alfredo"), ["-d", "POS 80", "-n", "2", "-o", "raw", "-t", "Alfredo"]);
        assert_eq!(lp_raw_args("q", 0, "t")[3], "1");
        assert_eq!(lp_pdf_args("Brother", "/tmp/cierre del día.pdf", 1, None), ["-d", "Brother", "-n", "1", "-o", "media=A4", "-o", "fit-to-page", "/tmp/cierre del día.pdf"]);
    }

    #[test]
    fn id_del_trabajo() {
        assert_eq!(parse_request_id("request id is POS80-123 (1 file(s))\n").as_deref(), Some("POS80-123"));
        assert_eq!(parse_request_id("lp: error"), None);
    }

    #[test]
    fn estado_de_la_cola() {
        assert_eq!(parse_lpstat_state("printer POS80 is idle.  enabled since …\n\tAlerts: none\n"), PrinterState::Ok);
        assert_eq!(parse_lpstat_state("printer POS80 is idle.\n\tAlerts: media-empty-error\n"), PrinterState::NoPaper);
        assert_eq!(parse_lpstat_state("printer POS80 disabled since …\n\tAlerts: paused\n"), PrinterState::Offline);
        assert_eq!(parse_lpstat_state("printer POS80 is idle.\n\tAlerts: offline-report\n"), PrinterState::Offline);
        assert_eq!(parse_lpstat_state("printer POS80 is idle.\n\tAlerts: cover-open-warning\n"), PrinterState::CoverOpen);
        assert_eq!(parse_lpstat_state("printer POS80 now printing POS80-9.\n\tAlerts: none\n"), PrinterState::Busy);
        assert_eq!(parse_lpstat_state(""), PrinterState::Unknown);
    }

    #[test]
    fn colas_y_airprint() {
        let q = parse_lpstat_devices("device for POS80: usb://Xprinter/XP-80?serial=1\ndispositivo para Brother_HL_1210W_series: dnssd://Brother%20HL-1210W._pdl-datastream._tcp.local./\n");
        assert_eq!(q.len(), 2);
        assert_eq!(q[0], CupsQueue { name: "POS80".into(), device_uri: "usb://Xprinter/XP-80?serial=1".into() });
        assert!(!q[0].looks_like_airprint());
        assert!(q[1].looks_like_airprint());
    }

    #[test]
    fn lpadmin_con_comillas() {
        let c = lpadmin_raw_queue_command("Alfredo_XP-80", "usb://Xprinter/XP 80?serial=1'2");
        assert!(c.starts_with("/usr/sbin/lpadmin -p 'Alfredo_XP-80' -E -v 'usb://Xprinter/XP 80?serial=1'\\''2' -m raw"));
        assert_eq!(osascript_admin("echo \"hola\""), "do shell script \"echo \\\"hola\\\"\" with administrator privileges");
    }
}
