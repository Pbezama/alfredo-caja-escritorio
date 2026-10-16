//! La sesión de la persona en el Llavero de macOS (T10.2, RF10.6): `session.rememberPerson` y
//! `recallPerson` del puente guardan el token acá, no en un archivo. Servicio
//! `cl.asistentealfredo.caja.escritorio`, una cuenta por local (slug).

/// Cuenta del Llavero para un local: `persona:<slug>`.
pub fn account_for(slug: &str) -> String {
    format!("persona:{}", slug.trim().to_ascii_lowercase())
}

#[cfg(target_os = "macos")]
mod sys {
    use security_framework::passwords::{delete_generic_password, get_generic_password, set_generic_password};

    pub fn save(service: &str, account: &str, secret: &str) -> Result<(), String> {
        set_generic_password(service, account, secret.as_bytes()).map_err(|e| e.to_string())
    }

    pub fn load(service: &str, account: &str) -> Option<String> {
        get_generic_password(service, account).ok().and_then(|b| String::from_utf8(b).ok())
    }

    pub fn forget(service: &str, account: &str) {
        let _ = delete_generic_password(service, account);
    }
}

#[cfg(target_os = "macos")]
pub use sys::{forget, load, save};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cuenta_por_local() {
        assert_eq!(account_for(" Pausan-Dev "), "persona:pausan-dev");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn ida_y_vuelta_en_el_llavero() {
        let service = "cl.asistentealfredo.caja.escritorio.test";
        let account = account_for("prueba-f10");
        forget(service, &account);
        assert_eq!(load(service, &account), None);
        save(service, &account, "token-de-prueba").expect("guardar");
        assert_eq!(load(service, &account).as_deref(), Some("token-de-prueba"));
        forget(service, &account);
        assert_eq!(load(service, &account), None);
    }
}
