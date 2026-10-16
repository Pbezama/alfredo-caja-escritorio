//! Anti-suspensión (T9.6): mientras haya sesión de caja o cocina, ni el equipo se suspende
//! ni (por defecto) se apaga la pantalla.
//!
//! Windows: `SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED [| ES_DISPLAY_REQUIRED])`.
//! El estado es POR HILO: se llama siempre desde el hilo principal (vive lo que la app).
//! macOS (base de F10): `caffeinate -i [-d] -w <pid>` como proceso hijo; F10 lo cambia por
//! IOPMAssertion.
//!
//! Vuelta de una suspensión: un hilo marca la hora cada 5 s; si entre dos marcas pasaron
//! más de 30 s de reloj, el equipo durmió y se emite `resume-from-sleep` a la web.

use std::time::{Duration, SystemTime};

pub const ES_SYSTEM_REQUIRED: u32 = 0x0000_0001;
pub const ES_DISPLAY_REQUIRED: u32 = 0x0000_0002;
pub const ES_CONTINUOUS: u32 = 0x8000_0000;

/// Flags de SetThreadExecutionState. Apagado = sólo ES_CONTINUOUS (vuelve a la política).
pub fn execution_flags(on: bool, display: bool) -> u32 {
    if !on {
        return ES_CONTINUOUS;
    }
    ES_CONTINUOUS | ES_SYSTEM_REQUIRED | if display { ES_DISPLAY_REQUIRED } else { 0 }
}

/// Argumentos de caffeinate (macOS).
pub fn caffeinate_args(display: bool, pid: u32) -> Vec<String> {
    let mut v = vec!["-i".to_string()];
    if display {
        v.push("-d".into());
    }
    v.push("-w".into());
    v.push(pid.to_string());
    v
}

/// ¿Hubo una suspensión entre dos marcas? (el tick es de 5 s).
pub fn slept_between(prev: SystemTime, now: SystemTime) -> bool {
    now.duration_since(prev).map(|d| d > Duration::from_secs(30)).unwrap_or(false)
}

#[cfg(windows)]
pub fn apply(on: bool, display: bool) {
    use windows::Win32::System::Power::{SetThreadExecutionState, EXECUTION_STATE};
    unsafe {
        SetThreadExecutionState(EXECUTION_STATE(execution_flags(on, display)));
    }
}

#[cfg(target_os = "macos")]
pub fn apply(on: bool, display: bool) {
    use std::sync::Mutex;
    static CHILD: Mutex<Option<std::process::Child>> = Mutex::new(None);
    let mut g = CHILD.lock().unwrap();
    if let Some(mut c) = g.take() {
        let _ = c.kill();
        let _ = c.wait();
    }
    if on {
        *g = std::process::Command::new("/usr/bin/caffeinate").args(caffeinate_args(display, std::process::id())).spawn().ok();
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
pub fn apply(_on: bool, _display: bool) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags() {
        assert_eq!(execution_flags(true, true), 0x8000_0003);
        assert_eq!(execution_flags(true, false), 0x8000_0001);
        assert_eq!(execution_flags(false, true), 0x8000_0000);
        assert_eq!(caffeinate_args(true, 42), vec!["-i", "-d", "-w", "42"]);
        assert_eq!(caffeinate_args(false, 7), vec!["-i", "-w", "7"]);
    }

    #[test]
    fn suspension() {
        let t = SystemTime::UNIX_EPOCH + Duration::from_secs(1000);
        assert!(!slept_between(t, t + Duration::from_secs(6)));
        assert!(slept_between(t, t + Duration::from_secs(600)));
        assert!(!slept_between(t + Duration::from_secs(5), t)); // reloj hacia atrás: no
    }
}
