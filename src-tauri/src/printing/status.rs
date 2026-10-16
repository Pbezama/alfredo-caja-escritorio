//! Estado en tiempo real ESC/POS: `DLE EOT n` (0x10 0x04 n).
//!   n=1 estado de la impresora: bit 3 = offline.
//!   n=4 estado del papel: bits 5 y 6 = sin papel (bits 2 y 3 = casi sin papel).

pub const DLE_EOT_PRINTER: [u8; 3] = [0x10, 0x04, 0x01];
pub const DLE_EOT_PAPER: [u8; 3] = [0x10, 0x04, 0x04];

/// Traduce las respuestas a `ok`, `offline`, `no_paper` o `unknown`.
pub fn parse_dle_eot(printer: Option<u8>, paper: Option<u8>) -> &'static str {
    let valid = |b: u8| b & 0b1001_0011 == 0b0001_0010; // bits fijos: 1 y 4 en 1; 0 y 7 en 0
    match (printer, paper) {
        (None, None) => "unknown",
        (p, q) => {
            if let Some(q) = q.filter(|b| valid(*b)) {
                if q & 0b0110_0000 != 0 {
                    return "no_paper";
                }
            }
            if let Some(p) = p.filter(|b| valid(*b)) {
                if p & 0b0000_1000 != 0 {
                    return "offline";
                }
                return "ok";
            }
            if q.map(valid).unwrap_or(false) {
                "ok"
            } else {
                "unknown"
            }
        }
    }
}

/// Estado del spooler de Windows (PRINTER_STATUS_*): muchas térmicas USB reportan siempre 0.
pub fn spooler_status_from_flags(status: u32, attributes_work_offline: bool) -> &'static str {
    const PAUSED: u32 = 0x1;
    const ERROR: u32 = 0x2;
    const PAPER_JAM: u32 = 0x8;
    const PAPER_OUT: u32 = 0x10;
    const OFFLINE: u32 = 0x80;
    const BUSY: u32 = 0x200;
    const NOT_AVAILABLE: u32 = 0x1000;
    const DOOR_OPEN: u32 = 0x400000;
    if status & PAPER_OUT != 0 {
        "no_paper"
    } else if status & (OFFLINE | NOT_AVAILABLE) != 0 || attributes_work_offline {
        "offline"
    } else if status & (ERROR | PAPER_JAM | DOOR_OPEN | PAUSED) != 0 {
        "error"
    } else if status & BUSY != 0 {
        "busy"
    } else {
        "unknown"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dle_eot() {
        assert_eq!(parse_dle_eot(Some(0x12), Some(0x12)), "ok");
        assert_eq!(parse_dle_eot(Some(0x1a), Some(0x12)), "offline");
        assert_eq!(parse_dle_eot(Some(0x12), Some(0x72)), "no_paper");
        assert_eq!(parse_dle_eot(Some(0x12), Some(0x1e)), "ok"); // casi sin papel: sigue imprimiendo
        assert_eq!(parse_dle_eot(None, None), "unknown");
        assert_eq!(parse_dle_eot(Some(0xff), None), "unknown"); // basura
        assert_eq!(parse_dle_eot(None, Some(0x72)), "no_paper");
    }

    #[test]
    fn spooler() {
        assert_eq!(spooler_status_from_flags(0, false), "unknown");
        assert_eq!(spooler_status_from_flags(0x10, false), "no_paper");
        assert_eq!(spooler_status_from_flags(0x80, false), "offline");
        assert_eq!(spooler_status_from_flags(0, true), "offline");
        assert_eq!(spooler_status_from_flags(0x2, false), "error");
        assert_eq!(spooler_status_from_flags(0x200, false), "busy");
    }
}
