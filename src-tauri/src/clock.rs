//! Reloj mal puesto: antes de navegar se mide el desfase contra el `Date` del servidor.
//!
//! Se pide por HTTP plano (`http://api.asistentealfredo.cl/api/health`) para leer la hora
//! aunque el reloj corrido rompa el TLS: Vercel contesta (308 a https) con `Date`.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const HEALTH_HTTP: &str = "http://api.asistentealfredo.cl/api/health";
/// Por sobre esto se muestra reloj.html (|desfase| > 5 min).
pub const MAX_SKEW_MS: i64 = 5 * 60 * 1000;

/// Desfase en ms del servidor menos el equipo (+ = el equipo va atrasado), corrigiendo
/// la mitad del viaje.
pub fn skew_ms(server: SystemTime, sent: SystemTime, received: SystemTime) -> i64 {
    let ms = |t: SystemTime| t.duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0);
    let rtt = (ms(received) - ms(sent)).max(0);
    ms(server) - (ms(sent) + rtt / 2)
}

pub fn parse_http_date(value: &str) -> Option<SystemTime> {
    httpdate::parse_http_date(value).ok()
}

pub fn is_skewed(skew: i64) -> bool {
    skew.abs() > MAX_SKEW_MS
}

/// Mide el desfase. `None` si no hubo respuesta (sin red: lo resuelve offline.html).
pub async fn measure(client: &reqwest::Client) -> Option<i64> {
    let sent = SystemTime::now();
    let res = client.head(HEALTH_HTTP).timeout(Duration::from_secs(6)).send().await.ok()?;
    let received = SystemTime::now();
    let date = res.headers().get(reqwest::header::DATE)?.to_str().ok()?;
    let server = parse_http_date(date)?;
    // `Date` tiene resolución de 1 s: se suma medio segundo.
    Some(skew_ms(server + Duration::from_millis(500), sent, received))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desfase_con_latencia() {
        let t0 = UNIX_EPOCH + Duration::from_secs(1_000_000);
        let server = t0 + Duration::from_millis(100);
        assert_eq!(skew_ms(server, t0, t0 + Duration::from_millis(200)), 0);
        // El equipo va un día adelantado.
        let server = t0 - Duration::from_secs(86_400);
        let s = skew_ms(server, t0, t0 + Duration::from_millis(50));
        assert!(is_skewed(s));
        assert!(s < 0);
        assert!(!is_skewed(60_000));
    }

    #[test]
    fn fecha_http() {
        let t = parse_http_date("Thu, 15 Oct 2026 23:00:00 GMT").unwrap();
        assert_eq!(t.duration_since(UNIX_EPOCH).unwrap().as_secs(), 1_792_105_200);
        assert!(parse_http_date("ayer").is_none());
    }
}
