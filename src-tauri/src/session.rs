//! Persona durante el turno (T9.2 punto 10): el token de la persona se guarda cifrado por el
//! sistema (Administrador de credenciales de Windows, protegido con DPAPI por usuario; en
//! macOS, el Llavero) hasta `until` (ahora + horas de turno, 12 h por defecto). Así el
//! escritorio que arranca solo con Windows entra a /cashier sin pedir PIN y la alarma corre.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub const SERVICE: &str = "cl.asistentealfredo.caja.escritorio";
pub const ACCOUNT: &str = "persona-del-turno";
/// Tope duro: nunca más de 16 h aunque la web pida más.
pub const MAX_HOURS: i64 = 16;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Remembered {
    pub token: String,
    pub until: String,
}

/// Normaliza `until`: válido, futuro y como máximo MAX_HOURS desde ahora.
pub fn clamp_until(until: &str, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
    let t = DateTime::parse_from_rfc3339(until).ok()?.with_timezone(&Utc);
    if t <= now {
        return None;
    }
    Some(t.min(now + chrono::Duration::hours(MAX_HOURS)))
}

pub fn still_valid(r: &Remembered, now: DateTime<Utc>) -> bool {
    DateTime::parse_from_rfc3339(&r.until).map(|t| t.with_timezone(&Utc) > now).unwrap_or(false) && !r.token.is_empty()
}

fn entry() -> Option<keyring::Entry> {
    keyring::Entry::new(SERVICE, ACCOUNT).ok()
}

pub fn remember(token: Option<&str>, until: Option<&str>) -> Result<(), String> {
    let Some(e) = entry() else { return Err("Sin almacén de credenciales".into()) };
    match (token, until.and_then(|u| clamp_until(u, Utc::now()))) {
        (Some(tok), Some(u)) if !tok.is_empty() && tok.len() < 8192 => {
            let r = Remembered { token: tok.to_string(), until: u.to_rfc3339() };
            e.set_password(&serde_json::to_string(&r).unwrap_or_default()).map_err(|x| x.to_string())
        }
        _ => {
            let _ = e.delete_credential();
            Ok(())
        }
    }
}

pub fn recall() -> Option<Remembered> {
    let e = entry()?;
    let r: Remembered = serde_json::from_str(&e.get_password().ok()?).ok()?;
    if still_valid(&r, Utc::now()) {
        Some(r)
    } else {
        let _ = e.delete_credential();
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vencimiento() {
        let now = DateTime::parse_from_rfc3339("2026-10-16T12:00:00Z").unwrap().with_timezone(&Utc);
        assert!(clamp_until("2026-10-16T11:00:00Z", now).is_none());
        assert_eq!(clamp_until("2026-10-16T20:00:00Z", now).unwrap().to_rfc3339(), "2026-10-16T20:00:00+00:00");
        // Más de 16 h se recorta.
        assert_eq!(clamp_until("2026-10-20T00:00:00Z", now).unwrap().to_rfc3339(), "2026-10-17T04:00:00+00:00");
        assert!(clamp_until("mañana", now).is_none());
        let r = Remembered { token: "t".into(), until: "2026-10-16T13:00:00Z".into() };
        assert!(still_valid(&r, now));
        assert!(!still_valid(&r, now + chrono::Duration::hours(2)));
        assert!(!still_valid(&Remembered { token: "".into(), until: r.until.clone() }, now));
    }
}
