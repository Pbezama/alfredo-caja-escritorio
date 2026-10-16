//! Sin red: calendario de reintentos de offline.html y chequeo de alcance del local.

use std::time::Duration;

/// Segundos hasta el próximo intento: 5, 10, 20, 30, 30… (como el banner de F0).
pub fn retry_delay_s(attempt: u32) -> u64 {
    match attempt {
        0 => 5,
        1 => 10,
        2 => 20,
        _ => 30,
    }
}

/// Tiempo máximo sin `DOMContentLoaded` antes de mostrar offline.html (Error_LoadTimeout).
pub const LOAD_TIMEOUT: Duration = Duration::from_secs(30);

/// ¿Responde la web del local? `GET {origin}/version.json` (lo publica la web desde F0).
pub async fn origin_reachable(client: &reqwest::Client, origin: &str) -> Result<(), String> {
    let url = format!("{}/version.json", origin.trim_end_matches('/'));
    match client.get(&url).timeout(Duration::from_secs(8)).send().await {
        Ok(r) if r.status().is_success() || r.status().is_redirection() || r.status().as_u16() == 404 => Ok(()),
        Ok(r) => Err(format!("La web respondió {}", r.status())),
        Err(e) => Err(classify_error(&e.to_string())),
    }
}

/// Distingue el certificado inválido por fecha (reloj corrido) del resto.
pub fn classify_error(msg: &str) -> String {
    let m = msg.to_ascii_lowercase();
    if m.contains("notvalidyet") || m.contains("not valid yet") || m.contains("expired") || m.contains("certificate") && m.contains("time") {
        "certificate_date".to_string()
    } else {
        "offline".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calendario_de_reintentos() {
        let v: Vec<u64> = (0..6).map(retry_delay_s).collect();
        assert_eq!(v, vec![5, 10, 20, 30, 30, 30]);
    }

    #[test]
    fn certificado_por_fecha() {
        assert_eq!(classify_error("invalid peer certificate: NotValidYet"), "certificate_date");
        assert_eq!(classify_error("invalid peer certificate: Expired"), "certificate_date");
        assert_eq!(classify_error("dns error: failed to lookup address"), "offline");
    }
}
