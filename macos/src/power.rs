//! Que el Mac no se duerma ni frene la app (T10.5).
//!
//! - `keep_awake(display)`: *assertions* de IOKit `PreventUserIdleSystemSleep` y, con `display`,
//!   `PreventUserIdleDisplaySleep` (razón «Alfredo Caja recibiendo pedidos»). Se ven con
//!   `pmset -g assertions`. Al soltarlas, macOS vuelve a su política.
//! - App Nap: el crate de Tauri llama `NSProcessInfo.beginActivity(options: [.userInitiated,
//!   .idleSystemSleepDisabled])` (objc2) mientras haya sesión de caja (flag `escritorio.mac.app_nap_off`).
//!   Con la tapa cerrada un MacBook se duerme igual (salvo con pantalla externa, teclado y cargador).

pub const REASON: &str = "Alfredo Caja recibiendo pedidos";
pub const SYSTEM_SLEEP: &str = "PreventUserIdleSystemSleep";
pub const DISPLAY_SLEEP: &str = "PreventUserIdleDisplaySleep";

/// Tipos de *assertion* según si además hay que dejar la pantalla encendida.
pub fn assertion_types(display: bool) -> Vec<&'static str> {
    if display {
        vec![SYSTEM_SLEEP, DISPLAY_SLEEP]
    } else {
        vec![SYSTEM_SLEEP]
    }
}

#[cfg(target_os = "macos")]
mod sys {
    use super::*;
    use core_foundation::base::TCFType;
    use core_foundation::string::{CFString, CFStringRef};

    #[link(name = "IOKit", kind = "framework")]
    extern "C" {
        fn IOPMAssertionCreateWithName(assertion_type: CFStringRef, level: u32, name: CFStringRef, id: *mut u32) -> i32;
        fn IOPMAssertionRelease(id: u32) -> i32;
    }
    const K_IOPM_ASSERTION_LEVEL_ON: u32 = 255;

    /// Las *assertions* activas; al soltar (`Drop`) macOS vuelve a su política.
    pub struct KeepAwake {
        ids: Vec<u32>,
    }

    impl KeepAwake {
        pub fn new(display: bool) -> Result<Self, String> {
            let name = CFString::new(REASON);
            let mut ids = vec![];
            for t in assertion_types(display) {
                let mut id: u32 = 0;
                let kind = CFString::new(t);
                let r = unsafe { IOPMAssertionCreateWithName(kind.as_concrete_TypeRef(), K_IOPM_ASSERTION_LEVEL_ON, name.as_concrete_TypeRef(), &mut id) };
                if r != 0 {
                    for i in &ids {
                        unsafe { IOPMAssertionRelease(*i) };
                    }
                    return Err(format!("IOPMAssertionCreateWithName({t}) = {r}"));
                }
                ids.push(id);
            }
            Ok(Self { ids })
        }
    }

    impl Drop for KeepAwake {
        fn drop(&mut self) {
            for id in &self.ids {
                unsafe { IOPMAssertionRelease(*id) };
            }
        }
    }
}

#[cfg(target_os = "macos")]
pub use sys::KeepAwake;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tipos_segun_pantalla() {
        assert_eq!(assertion_types(false), vec![SYSTEM_SLEEP]);
        assert_eq!(assertion_types(true), vec![SYSTEM_SLEEP, DISPLAY_SLEEP]);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn las_assertions_se_ven_en_pmset_y_se_sueltan() {
        let a = KeepAwake::new(true).expect("assertions");
        let out = std::process::Command::new("/usr/bin/pmset").args(["-g", "assertions"]).output().unwrap();
        let s = String::from_utf8_lossy(&out.stdout);
        assert!(s.contains(REASON), "pmset no muestra la assertion: {s}");
        drop(a);
        let out = std::process::Command::new("/usr/bin/pmset").args(["-g", "assertions"]).output().unwrap();
        assert!(!String::from_utf8_lossy(&out.stdout).contains(REASON));
    }
}
