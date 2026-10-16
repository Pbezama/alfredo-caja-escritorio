//! Lista blanca de orígenes (RF9.2 y RF9.3) y normalización de la dirección del local.
//!
//! El escritorio sólo navega dentro de la web staff del local. Todo otro enlace (Uber,
//! Mercado Pago, mapas) se abre en el navegador del sistema, y sólo estos orígenes
//! reciben `window.alfredoDesktop` (lo exige también la capability `remote`).

use url::Url;

/// Sufijos de la web staff: `{slug}.app.asistentealfredo.cl` y el alias histórico.
pub const STAFF_HOST_SUFFIXES: &[&str] = &[".app.asistentealfredo.cl", ".app.restaurantya.cl"];
/// Dominio canónico para armar el origen de un local nuevo.
pub const CANONICAL_SUFFIX: &str = ".app.asistentealfredo.cl";
/// API pública (validar el local, reloj, actualizaciones).
pub const API_BASE: &str = "https://api.asistentealfredo.cl/api";

/// ¿El host es subdominio (no vacío) de la web staff?
pub fn is_staff_host(host: &str) -> bool {
    let host = host.to_ascii_lowercase();
    STAFF_HOST_SUFFIXES.iter().any(|s| {
        host.len() > s.len() && host.ends_with(s) && {
            let label = &host[..host.len() - s.len()];
            !label.is_empty() && !label.contains("..") && !label.starts_with('.')
        }
    })
}

/// ¿Esta URL puede cargarse en la ventana principal y usar el puente? Sólo https staff.
pub fn is_allowed_url(url: &Url) -> bool {
    url.scheme() == "https" && url.host_str().map(is_staff_host).unwrap_or(false)
}

/// Páginas locales del escritorio (setup, offline, reloj, actualizar): `tauri://localhost`
/// en macOS/Linux y `http(s)://tauri.localhost` en Windows.
pub fn is_local_url(url: &Url) -> bool {
    match url.scheme() {
        "tauri" => url.host_str() == Some("localhost"),
        "http" | "https" => url.host_str() == Some("tauri.localhost"),
        _ => false,
    }
}

/// Qué hacer con una navegación de la ventana principal.
#[derive(Debug, PartialEq, Eq)]
pub enum NavDecision {
    Allow,
    /// Abrir en el navegador del sistema (y no navegar).
    OpenExternal,
    /// Ignorar (esquemas raros: javascript:, file:, data:…).
    Block,
}

pub fn decide_navigation(url: &Url) -> NavDecision {
    if is_allowed_url(url) || is_local_url(url) || url.scheme() == "about" {
        return NavDecision::Allow;
    }
    match url.scheme() {
        "http" | "https" | "mailto" | "tel" | "whatsapp" => NavDecision::OpenExternal,
        _ => NavDecision::Block,
    }
}

/// Lo que escribe el dueño en «Conecta este equipo con tu local»: `pausan`,
/// `pausan.app.asistentealfredo.cl`, `https://pausan.app.asistentealfredo.cl/cashier` o un
/// enlace del alias. Devuelve el slug normalizado.
pub fn slug_from_input(input: &str) -> Option<String> {
    let raw = input.trim().trim_end_matches('/').to_ascii_lowercase();
    if raw.is_empty() {
        return None;
    }
    let host = if raw.contains("://") {
        Url::parse(&raw).ok()?.host_str()?.to_string()
    } else {
        raw.split('/').next().unwrap_or("").to_string()
    };
    let slug = STAFF_HOST_SUFFIXES
        .iter()
        .find_map(|s| host.strip_suffix(s).map(|x| x.to_string()))
        .unwrap_or(host);
    let valid = !slug.is_empty()
        && slug.len() <= 63
        && slug.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        && !slug.starts_with('-')
        && !slug.ends_with('-');
    valid.then_some(slug)
}

/// `https://{slug}.app.asistentealfredo.cl`.
pub fn origin_for_slug(slug: &str) -> String {
    format!("https://{slug}{CANONICAL_SUFFIX}")
}

/// Slug del origen guardado.
pub fn slug_of_origin(origin: &str) -> Option<String> {
    let url = Url::parse(origin).ok()?;
    let host = url.host_str()?.to_string();
    STAFF_HOST_SUFFIXES.iter().find_map(|s| host.strip_suffix(s).map(|x| x.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    #[test]
    fn lista_blanca() {
        assert!(is_allowed_url(&u("https://pausan.app.asistentealfredo.cl/cashier")));
        assert!(is_allowed_url(&u("https://pausan-dev.app.restaurantya.cl/kitchen")));
        assert!(!is_allowed_url(&u("http://pausan.app.asistentealfredo.cl/")));
        assert!(!is_allowed_url(&u("https://app.asistentealfredo.cl/")));
        assert!(!is_allowed_url(&u("https://evil.com/?x=.app.asistentealfredo.cl")));
        assert!(!is_allowed_url(&u("https://pausan.app.asistentealfredo.cl.evil.com/")));
        assert!(!is_allowed_url(&u("https://maps.google.com/")));
    }

    #[test]
    fn navegacion() {
        assert_eq!(decide_navigation(&u("https://pausan.app.asistentealfredo.cl/orders")), NavDecision::Allow);
        assert_eq!(decide_navigation(&u("tauri://localhost/offline.html")), NavDecision::Allow);
        assert_eq!(decide_navigation(&u("http://tauri.localhost/setup.html")), NavDecision::Allow);
        assert_eq!(decide_navigation(&u("https://maps.google.com/?q=x")), NavDecision::OpenExternal);
        assert_eq!(decide_navigation(&u("https://www.mercadopago.cl/checkout")), NavDecision::OpenExternal);
        assert_eq!(decide_navigation(&u("tel:+56912345678")), NavDecision::OpenExternal);
        assert_eq!(decide_navigation(&u("file:///C:/Windows/win.ini")), NavDecision::Block);
    }

    #[test]
    fn slug_desde_lo_que_escribe_el_dueno() {
        assert_eq!(slug_from_input("pausan").as_deref(), Some("pausan"));
        assert_eq!(slug_from_input("  PauSan-Dev ").as_deref(), Some("pausan-dev"));
        assert_eq!(slug_from_input("pausan.app.asistentealfredo.cl").as_deref(), Some("pausan"));
        assert_eq!(slug_from_input("https://pausan.app.asistentealfredo.cl/cashier?x=1").as_deref(), Some("pausan"));
        assert_eq!(slug_from_input("https://loco.app.restaurantya.cl/").as_deref(), Some("loco"));
        assert_eq!(slug_from_input("pau san"), None);
        assert_eq!(slug_from_input(""), None);
        assert_eq!(slug_from_input("-x"), None);
        assert_eq!(origin_for_slug("pausan"), "https://pausan.app.asistentealfredo.cl");
        assert_eq!(slug_of_origin("https://pausan.app.asistentealfredo.cl").as_deref(), Some("pausan"));
    }
}
