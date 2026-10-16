//! Impresora instalada en el sistema.
//!
//! Windows: spooler RAW por winspool (`OpenPrinterW` → `StartDocPrinterW` con
//! `DOC_INFO_1 { pDatatype: "RAW" }` → `StartPagePrinter` → `WritePrinter` → `EndPagePrinter`
//! → `EndDocPrinter` → `ClosePrinter`). Si el driver es v4 y rechaza RAW, se reintenta con
//! `XPS_PASS`. Sirve el driver del fabricante o «Generic / Text Only».
//!
//! macOS (base para F10): cola de CUPS en modo raw con `lp -d <cola> -o raw`, los bytes por
//! stdin. F10 (T10.3) lo cambia por libcups y agrega el estado fino.

use super::PrintError;

/// Tipos de datos que se prueban en orden.
pub const DATATYPES: [&str; 2] = ["RAW", "XPS_PASS"];
/// Nombre del trabajo en la cola de Windows.
pub const DOC_NAME: &str = "Alfredo";

/// Cadena UTF-16 con NUL final (para las APIs W de Windows).
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Argumentos de `lp` para mandar bytes crudos a una cola (macOS/Linux).
pub fn lp_args(queue: &str) -> Vec<String> {
    vec!["-d".into(), queue.into(), "-o".into(), "raw".into(), "-t".into(), DOC_NAME.into()]
}

/// «request id is POS80-12 (0 file(s))» → `POS80-12`.
pub fn parse_lp_request_id(out: &str) -> Option<String> {
    let i = out.find("request id is ")?;
    out[i + 14..].split_whitespace().next().map(|s| s.to_string())
}

#[cfg(windows)]
pub fn send(name: &str, bytes: &[u8]) -> Result<(), PrintError> {
    if name.trim().is_empty() {
        return Err(PrintError::new("bad_request", "Falta el nombre de la impresora de Windows"));
    }
    let mut last = PrintError::new("error", "No se pudo imprimir");
    for dt in DATATYPES {
        match win::write_raw(name, dt, bytes) {
            Ok(()) => return Ok(()),
            // El driver v4 descarta RAW en StartDocPrinter: probar XPS_PASS.
            Err((stage, code, msg)) if stage == "startdoc" && dt == "RAW" => {
                last = PrintError::new(code, msg);
                continue;
            }
            Err((_, code, msg)) => return Err(PrintError::new(code, msg)),
        }
    }
    Err(last)
}

#[cfg(windows)]
pub fn status(name: &str) -> String {
    win::status(name).unwrap_or_else(|| "offline".into())
}

#[cfg(windows)]
pub fn list() -> Vec<(String, String, String)> {
    win::enum_printers()
}

#[cfg(not(windows))]
pub fn send(queue: &str, bytes: &[u8]) -> Result<(), PrintError> {
    use std::io::Write;
    use std::process::{Command, Stdio};
    if queue.trim().is_empty() {
        return Err(PrintError::new("bad_request", "Falta la cola de impresión"));
    }
    let mut child = Command::new("lp")
        .args(lp_args(queue))
        .env("LC_ALL", "C")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| PrintError::new("error", format!("No se pudo ejecutar lp: {e}")))?;
    child.stdin.take().map(|mut s| s.write_all(bytes)).transpose().map_err(|e| PrintError::new("error", e.to_string()))?;
    let out = child.wait_with_output().map_err(|e| PrintError::new("error", e.to_string()))?;
    if out.status.success() {
        Ok(())
    } else {
        let err = String::from_utf8_lossy(&out.stderr).to_string();
        let code = if err.contains("not exist") || err.contains("No such") { "offline" } else { "error" };
        Err(PrintError::new(code, format!("CUPS: {}", err.trim())))
    }
}

/// macOS (F10): estado y motivos de la cola con `lpstat -l -p` (sin papel, tapa, pausada…).
#[cfg(target_os = "macos")]
pub fn status(queue: &str) -> String {
    alfredo_macos::cups::state(queue).as_str().to_string()
}

#[cfg(all(not(windows), not(target_os = "macos")))]
pub fn status(queue: &str) -> String {
    let out = std::process::Command::new("lpstat").args(["-p", queue]).output();
    match out {
        Ok(o) if o.status.success() => {
            let s = String::from_utf8_lossy(&o.stdout).to_lowercase();
            if s.contains("disabled") || s.contains("deshabilitad") {
                "offline".into()
            } else if s.contains("media-empty") {
                "no_paper".into()
            } else {
                "unknown".into()
            }
        }
        _ => "offline".into(),
    }
}

/// Colas de CUPS: (nombre, uri, origen). `lpstat -v` → «device for POS80: usb://…».
#[cfg(not(windows))]
pub fn list() -> Vec<(String, String, String)> {
    let Ok(o) = std::process::Command::new("lpstat").arg("-v").output() else { return vec![] };
    parse_lpstat_v(&String::from_utf8_lossy(&o.stdout))
}

pub fn parse_lpstat_v(out: &str) -> Vec<(String, String, String)> {
    out.lines()
        .filter_map(|l| {
            let rest = l.strip_prefix("device for ").or_else(|| l.strip_prefix("dispositivo para "))?;
            let (name, uri) = rest.split_once(": ")?;
            Some((name.trim().to_string(), String::new(), uri.trim().to_string()))
        })
        .collect()
}

#[cfg(windows)]
mod win {
    use super::{wide, DOC_NAME};
    use windows::core::{PCWSTR, PWSTR};
    use windows::Win32::Foundation::GetLastError;
    use windows::Win32::Graphics::Printing::{
        ClosePrinter, EndDocPrinter, EndPagePrinter, EnumPrintersW, GetPrinterW, OpenPrinterW, StartDocPrinterW, StartPagePrinter, WritePrinter, DOC_INFO_1W,
        PRINTER_ENUM_CONNECTIONS, PRINTER_ENUM_LOCAL, PRINTER_HANDLE, PRINTER_INFO_2W,
    };

    type WinErr = (&'static str, &'static str, String);

    struct Handle(PRINTER_HANDLE);
    impl Drop for Handle {
        fn drop(&mut self) {
            unsafe {
                let _ = ClosePrinter(self.0);
            }
        }
    }

    fn open(name: &str) -> Result<Handle, WinErr> {
        let wname = wide(name);
        let mut h = PRINTER_HANDLE::default();
        unsafe { OpenPrinterW(PCWSTR(wname.as_ptr()), &mut h, None) }
            .map_err(|e| ("open", "offline", format!("Windows no encuentra la impresora «{name}»: {e}")))?;
        Ok(Handle(h))
    }

    pub fn write_raw(name: &str, datatype: &str, bytes: &[u8]) -> Result<(), WinErr> {
        let h = open(name)?;
        let mut doc_name = wide(DOC_NAME);
        let mut dt = wide(datatype);
        let info = DOC_INFO_1W { pDocName: PWSTR(doc_name.as_mut_ptr()), pOutputFile: PWSTR::null(), pDatatype: PWSTR(dt.as_mut_ptr()) };
        let job = unsafe { StartDocPrinterW(h.0, 1, &info as *const DOC_INFO_1W as *const _) };
        if job == 0 {
            let e = unsafe { GetLastError() };
            return Err(("startdoc", "error", format!("La impresora «{name}» no aceptó el trabajo ({datatype}, error {})", e.0)));
        }
        let ok_page = unsafe { StartPagePrinter(h.0) }.as_bool();
        let mut written: u32 = 0;
        let ok = unsafe { WritePrinter(h.0, bytes.as_ptr() as *const _, bytes.len() as u32, &mut written) }.as_bool();
        unsafe {
            if ok_page {
                let _ = EndPagePrinter(h.0);
            }
            let _ = EndDocPrinter(h.0);
        }
        if !ok || written as usize != bytes.len() {
            let e = unsafe { GetLastError() };
            return Err(("write", "error", format!("No se pudo escribir en «{name}» (error {}, {written}/{} bytes)", e.0, bytes.len())));
        }
        Ok(())
    }

    pub fn status(name: &str) -> Option<String> {
        let h = open(name).ok()?;
        let mut needed: u32 = 0;
        unsafe {
            let _ = GetPrinterW(h.0, 2, None, &mut needed);
        }
        if needed == 0 {
            return Some("unknown".into());
        }
        let mut buf = vec![0u8; needed as usize];
        unsafe { GetPrinterW(h.0, 2, Some(&mut buf), &mut needed) }.ok()?;
        let info = unsafe { &*(buf.as_ptr() as *const PRINTER_INFO_2W) };
        const PRINTER_ATTRIBUTE_WORK_OFFLINE: u32 = 0x400;
        Some(crate::printing::status::spooler_status_from_flags(info.Status, info.Attributes & PRINTER_ATTRIBUTE_WORK_OFFLINE != 0).to_string())
    }

    /// (nombre, driver, puerto) de las impresoras locales y conectadas.
    pub fn enum_printers() -> Vec<(String, String, String)> {
        let flags = PRINTER_ENUM_LOCAL | PRINTER_ENUM_CONNECTIONS;
        let mut needed: u32 = 0;
        let mut count: u32 = 0;
        unsafe {
            let _ = EnumPrintersW(flags, PCWSTR::null(), 2, None, &mut needed, &mut count);
        }
        if needed == 0 {
            return vec![];
        }
        let mut buf = vec![0u8; needed as usize];
        if unsafe { EnumPrintersW(flags, PCWSTR::null(), 2, Some(&mut buf), &mut needed, &mut count) }.is_err() {
            return vec![];
        }
        let infos = unsafe { std::slice::from_raw_parts(buf.as_ptr() as *const PRINTER_INFO_2W, count as usize) };
        let s = |p: PWSTR| if p.is_null() { String::new() } else { unsafe { p.to_string().unwrap_or_default() } };
        infos.iter().map(|i| (s(i.pPrinterName), s(i.pDriverName), s(i.pPortName))).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doc_info_y_argumentos() {
        assert_eq!(wide("RAW"), vec![b'R' as u16, b'A' as u16, b'W' as u16, 0]);
        assert_eq!(wide("Cocina ñ").last(), Some(&0));
        assert_eq!(DATATYPES, ["RAW", "XPS_PASS"]);
        assert_eq!(lp_args("POS 80 Ñuñoa"), vec!["-d", "POS 80 Ñuñoa", "-o", "raw", "-t", "Alfredo"]);
        assert_eq!(parse_lp_request_id("request id is POS80-12 (0 file(s))\n").as_deref(), Some("POS80-12"));
        assert_eq!(parse_lp_request_id("lp: error"), None);
    }

    #[test]
    fn lee_lpstat() {
        let v = parse_lpstat_v("device for POS80: usb://EPSON/TM-T20?serial=1\ndevice for Brother_HL: dnssd://Brother%20HL\n");
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].0, "POS80");
        assert_eq!(v[1].2, "dnssd://Brother%20HL");
    }
}
