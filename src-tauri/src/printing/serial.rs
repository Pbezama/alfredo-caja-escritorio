//! Impresora serie (COM en Windows, incluidas las Bluetooth SPP emparejadas; /dev/cu.* en macOS).

use super::status::{parse_dle_eot, DLE_EOT_PAPER, DLE_EOT_PRINTER};
use super::{PrintError, PrinterTarget};
use serialport::{DataBits, FlowControl, Parity, StopBits};
use std::io::{Read, Write};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq)]
pub struct SerialSettings {
    pub path: String,
    pub baud: u32,
    pub data_bits: DataBits,
    pub parity: Parity,
    pub stop_bits: StopBits,
    pub flow: FlowControl,
}

impl SerialSettings {
    pub fn from_target(t: &PrinterTarget) -> Self {
        Self {
            path: t.com.clone().unwrap_or_default(),
            baud: t.baud.filter(|b| *b >= 1200 && *b <= 921_600).unwrap_or(9600),
            data_bits: if t.data_bits == Some(7) { DataBits::Seven } else { DataBits::Eight },
            parity: match t.parity.as_deref() {
                Some("even") => Parity::Even,
                Some("odd") => Parity::Odd,
                _ => Parity::None,
            },
            stop_bits: if t.stop_bits == Some(2) { StopBits::Two } else { StopBits::One },
            flow: match t.flow.as_deref() {
                Some("rtscts") => FlowControl::Hardware,
                Some("xonxoff") => FlowControl::Software,
                _ => FlowControl::None,
            },
        }
    }
}

fn open(s: &SerialSettings) -> Result<Box<dyn serialport::SerialPort>, PrintError> {
    if s.path.is_empty() {
        return Err(PrintError::new("bad_request", "Falta el puerto serie"));
    }
    serialport::new(&s.path, s.baud)
        .data_bits(s.data_bits)
        .parity(s.parity)
        .stop_bits(s.stop_bits)
        .flow_control(s.flow)
        .timeout(Duration::from_secs(3))
        .open()
        .map_err(|e| match e.kind() {
            serialport::ErrorKind::NoDevice => PrintError::new("offline", format!("No está el puerto {}", s.path)),
            serialport::ErrorKind::Io(std::io::ErrorKind::PermissionDenied) => PrintError::new("busy", format!("El puerto {} está en uso por otro programa", s.path)),
            _ => PrintError::new("error", format!("No se pudo abrir {}: {e}", s.path)),
        })
}

pub fn send(s: &SerialSettings, bytes: &[u8]) -> Result<(), PrintError> {
    let mut port = open(s)?;
    port.write_all(bytes).map_err(|e| match e.kind() {
        std::io::ErrorKind::TimedOut => PrintError::new("offline", format!("La impresora en {} no recibe datos", s.path)),
        _ => PrintError::new("error", format!("Error escribiendo en {}: {e}", s.path)),
    })?;
    port.flush().ok();
    Ok(())
}

pub fn status(s: &SerialSettings) -> String {
    let Ok(mut port) = open(s) else { return "offline".into() };
    port.set_timeout(Duration::from_millis(500)).ok();
    let mut ask = |cmd: &[u8]| -> Option<u8> {
        port.write_all(cmd).ok()?;
        let mut b = [0u8; 1];
        port.read_exact(&mut b).ok().map(|_| b[0])
    };
    let p = ask(&DLE_EOT_PRINTER);
    let q = ask(&DLE_EOT_PAPER);
    parse_dle_eot(p, q).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mapea_la_configuracion_a_serialport() {
        let t = PrinterTarget {
            com: Some("COM3".into()),
            baud: Some(115_200),
            data_bits: Some(7),
            parity: Some("even".into()),
            stop_bits: Some(2),
            flow: Some("rtscts".into()),
            ..Default::default()
        };
        let s = SerialSettings::from_target(&t);
        assert_eq!(s, SerialSettings { path: "COM3".into(), baud: 115_200, data_bits: DataBits::Seven, parity: Parity::Even, stop_bits: StopBits::Two, flow: FlowControl::Hardware });
        let d = SerialSettings::from_target(&PrinterTarget { com: Some("/dev/cu.usbserial".into()), baud: Some(5), ..Default::default() });
        assert_eq!((d.baud, d.data_bits, d.parity, d.stop_bits, d.flow), (9600, DataBits::Eight, Parity::None, StopBits::One, FlowControl::None));
    }

    #[test]
    fn puerto_inexistente() {
        let s = SerialSettings::from_target(&PrinterTarget { com: Some("COM_NO_EXISTE_99".into()), ..Default::default() });
        assert!(send(&s, b"x").is_err());
    }
}
