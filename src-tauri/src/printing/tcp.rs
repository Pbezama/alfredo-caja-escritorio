//! Impresora de red, puerto 9100 (RAW / JetDirect).

use super::status::{parse_dle_eot, DLE_EOT_PAPER, DLE_EOT_PRINTER};
use super::PrintError;
use std::io::{Read, Write};
use std::net::{Shutdown, SocketAddr, TcpStream, ToSocketAddrs};
use std::time::Duration;

fn addr(host: &str, port: u16) -> Result<SocketAddr, PrintError> {
    if host.trim().is_empty() {
        return Err(PrintError::new("bad_request", "Falta la IP de la impresora"));
    }
    (host.trim(), port)
        .to_socket_addrs()
        .map_err(|e| PrintError::new("offline", format!("No se encontró {host}: {e}")))?
        .next()
        .ok_or_else(|| PrintError::new("offline", format!("No se encontró {host}")))
}

pub fn send(host: &str, port: u16, bytes: &[u8]) -> Result<(), PrintError> {
    let a = addr(host, port)?;
    let mut s = TcpStream::connect_timeout(&a, Duration::from_secs(3)).map_err(|e| map_io(&e, host))?;
    s.set_write_timeout(Some(Duration::from_secs(5))).ok();
    s.write_all(bytes).map_err(|e| map_io(&e, host))?;
    s.flush().ok();
    s.shutdown(Shutdown::Write).ok();
    Ok(())
}

fn map_io(e: &std::io::Error, host: &str) -> PrintError {
    use std::io::ErrorKind::*;
    match e.kind() {
        ConnectionRefused => PrintError::new("busy", format!("La impresora {host} rechazó la conexión (¿ocupada?)")),
        TimedOut | WouldBlock | HostUnreachable | NetworkUnreachable => PrintError::new("offline", format!("La impresora {host} no responde")),
        _ => PrintError::new("error", format!("Error con la impresora {host}: {e}")),
    }
}

fn ask(s: &mut TcpStream, cmd: &[u8]) -> Option<u8> {
    s.write_all(cmd).ok()?;
    let mut b = [0u8; 1];
    match s.read(&mut b) {
        Ok(1) => Some(b[0]),
        _ => None,
    }
}

pub fn status(host: &str, port: u16) -> String {
    let Ok(a) = addr(host, port) else { return "offline".into() };
    let Ok(mut s) = TcpStream::connect_timeout(&a, Duration::from_millis(1500)) else { return "offline".into() };
    s.set_read_timeout(Some(Duration::from_millis(500))).ok();
    let p = ask(&mut s, &DLE_EOT_PRINTER);
    let q = ask(&mut s, &DLE_EOT_PAPER);
    match parse_dle_eot(p, q) {
        "unknown" => "ok".into(), // aceptó la conexión pero no contesta estado: muchas no lo hacen
        x => x.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn manda_los_bytes_a_un_listener_local() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let h = std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            let mut buf = vec![];
            s.read_to_end(&mut buf).unwrap();
            buf
        });
        send("127.0.0.1", port, &[0x1b, 0x40, b'h', b'i']).unwrap();
        assert_eq!(h.join().unwrap(), vec![0x1b, 0x40, b'h', b'i']);
    }

    #[test]
    fn sin_impresora_es_offline_o_busy() {
        // Puerto cerrado en localhost: rechazo inmediato.
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        drop(l);
        let e = send("127.0.0.1", port, b"x").unwrap_err();
        assert!(e.code == "busy" || e.code == "offline", "{e:?}");
        assert_eq!(send("", 9100, b"x").unwrap_err().code, "bad_request");
    }

    #[test]
    fn estado_por_dle_eot() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            let mut b = [0u8; 3];
            s.read_exact(&mut b).unwrap();
            s.write_all(&[0x12]).unwrap();
            s.read_exact(&mut b).unwrap();
            s.write_all(&[0x72]).unwrap(); // sin papel
        });
        assert_eq!(status("127.0.0.1", port), "no_paper");
    }
}
