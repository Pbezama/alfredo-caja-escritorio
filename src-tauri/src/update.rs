//! Auto-update (T9.7) con tauri-plugin-updater, ligado a la política de F0.
//!
//! El backend (`GET /api/desktop/update`) decide con `app_release_policies('escritorio', …)`
//! y `desktop_releases`: 204 si no hay nada; si no, el JSON del updater más `required` y
//! `grace_minutes`. Revisa al arrancar y cada 30 min, y con «Buscar actualización».
//! - Recomendada: descarga en segundo plano, punto/ítem en la bandeja, globo y evento
//!   `update-available` a la web. Se instala al hacer clic o al «Salir».
//! - Obligatoria: espera a que no haya pedidos pendientes (o al plazo), muestra
//!   actualizar.html, instala en modo pasivo y relanza.
//! Un binario sin la firma del updater (clave pública en tauri.conf.json) no se instala.

use crate::state::AppState;
use crate::tray;
use crate::window;
use serde::Serialize;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};

pub const UPDATE_ENDPOINT: &str = "https://api.asistentealfredo.cl/api/desktop/update";
pub const CHECK_EVERY: Duration = Duration::from_secs(30 * 60);
pub const RETRY_FAILED_INSTALL: Duration = Duration::from_secs(5 * 60);

#[derive(Default)]
pub struct UpdateState {
    pub available: Option<Update>,
    pub bytes: Option<Vec<u8>>,
    pub required: bool,
    pub grace_minutes: u64,
    pub required_since: Option<Instant>,
    pub notified_version: Option<String>,
    pub installing: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct CheckOut {
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    pub downloaded: bool,
}

/// URL del endpoint dinámico: Tauri reemplaza {{target}}, {{arch}} y {{current_version}}.
pub fn endpoint(device_id: &str, channel: &str) -> String {
    format!(
        "{UPDATE_ENDPOINT}?target={{{{target}}}}&arch={{{{arch}}}}&current_version={{{{current_version}}}}&device_id={}&channel={}",
        urlencode(device_id),
        urlencode(channel)
    )
}

fn urlencode(s: &str) -> String {
    s.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_' || *c == '.').collect()
}

/// ¿Ya toca forzar la obligatoria? Sin pedidos pendientes, o vencido el plazo.
pub fn should_force(incoming: u32, waited: Duration, grace_minutes: u64) -> bool {
    incoming == 0 || waited >= Duration::from_secs(grace_minutes.max(0) * 60)
}

pub async fn check(app: &AppHandle, user_initiated: bool) -> Result<CheckOut, String> {
    let st = app.state::<AppState>();
    let cfg = st.cfg();
    let url = endpoint(&st.device_id, &cfg.channel).parse().map_err(|e: url::ParseError| e.to_string())?;
    let updater = app.updater_builder().endpoints(vec![url]).map_err(|e| e.to_string())?.timeout(Duration::from_secs(20)).build().map_err(|e| e.to_string())?;
    let found = updater.check().await.map_err(|e| e.to_string())?;
    let Some(u) = found else {
        let mut g = st.update.lock().unwrap();
        *g = UpdateState::default();
        return Ok(CheckOut { status: "ok".into(), version: None, notes: None, downloaded: false });
    };
    let required = u.raw_json.get("required").and_then(|v| v.as_bool()).unwrap_or(false);
    let grace = u.raw_json.get("grace_minutes").and_then(|v| v.as_u64()).unwrap_or(10).min(120);
    let version = u.version.clone();
    let notes = u.body.clone();
    let (downloaded, newly) = {
        let mut g = st.update.lock().unwrap();
        let same = g.available.as_ref().map(|x| x.version == version).unwrap_or(false);
        if !same {
            g.bytes = None;
        }
        g.available = Some(u.clone());
        g.required = required;
        g.grace_minutes = grace;
        if required && g.required_since.is_none() {
            g.required_since = Some(Instant::now());
        }
        let newly = g.notified_version.as_deref() != Some(version.as_str());
        g.notified_version = Some(version.clone());
        (g.bytes.is_some(), newly)
    };
    let status = if required { "required" } else { "recommended" };
    let out = CheckOut { status: status.into(), version: Some(version.clone()), notes: notes.clone(), downloaded };
    window::emit_to_web(app, "update-available", serde_json::to_value(&out).unwrap_or_default());
    if let Some(items) = app.try_state::<tray::TrayItems>() {
        let _ = items.update.set_text(format!("Instalar actualización {version}"));
    }
    if newly || user_initiated {
        tray::notify(app, "Actualización disponible", "¡Haz clic aquí para instalarla! (menú de Alfredo en la bandeja)");
    }
    if !downloaded {
        let app2 = app.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(e) = download(&app2).await {
                log::warn!("descarga de la actualización falló: {e}");
            }
        });
    }
    if required {
        spawn_enforcer(app.clone());
    }
    Ok(out)
}

async fn download(app: &AppHandle) -> Result<(), String> {
    let st = app.state::<AppState>();
    let u = st.update.lock().unwrap().available.clone().ok_or("sin actualización")?;
    let bytes = u.download(|_, _| {}, || {}).await.map_err(|e| e.to_string())?;
    let mut g = st.update.lock().unwrap();
    if g.available.as_ref().map(|x| x.version == u.version).unwrap_or(false) {
        g.bytes = Some(bytes);
    }
    Ok(())
}

/// Instala la actualización descargada (o la descarga) y relanza.
pub async fn install(app: &AppHandle) -> Result<(), String> {
    let st = app.state::<AppState>();
    {
        let mut g = st.update.lock().unwrap();
        if g.installing {
            return Ok(());
        }
        g.installing = true;
    }
    let r = async {
        if st.update.lock().unwrap().bytes.is_none() {
            download(app).await?;
        }
        let (u, bytes) = {
            let g = st.update.lock().unwrap();
            (g.available.clone().ok_or("No hay actualización")?, g.bytes.clone().ok_or("No se descargó")?)
        };
        u.install(bytes).map_err(|e| e.to_string())
    }
    .await;
    st.update.lock().unwrap().installing = false;
    match r {
        Ok(()) => {
            st.quitting.store(true, std::sync::atomic::Ordering::SeqCst);
            app.restart();
        }
        Err(e) => {
            log::error!("no se pudo instalar la actualización: {e}");
            Err(format!("No se puede instalar la actualización. Verifica tu conexión a internet. ({e})"))
        }
    }
}

fn spawn_enforcer(app: AppHandle) {
    static RUNNING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if RUNNING.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    tauri::async_runtime::spawn(async move {
        loop {
            let (since, grace, required) = {
                let st = app.state::<AppState>();
                let g = st.update.lock().unwrap();
                (g.required_since, g.grace_minutes, g.required)
            };
            if !required {
                break;
            }
            let incoming = app.state::<AppState>().incoming();
            if should_force(incoming, since.map(|s| s.elapsed()).unwrap_or_default(), grace) {
                window::bring_to_front(&app);
                window::navigate_local(&app, "actualizar.html");
                tokio::time::sleep(Duration::from_secs(3)).await;
                if let Err(e) = install(&app).await {
                    window::navigate_local(&app, &format!("actualizar.html?error={}", urlencode_text(&e)));
                    tokio::time::sleep(RETRY_FAILED_INSTALL).await;
                    continue;
                }
                break;
            }
            tokio::time::sleep(Duration::from_secs(15)).await;
        }
        RUNNING.store(false, std::sync::atomic::Ordering::SeqCst);
    });
}

fn urlencode_text(s: &str) -> String {
    s.bytes()
        .map(|b| if b.is_ascii_alphanumeric() { (b as char).to_string() } else { format!("%{b:02X}") })
        .collect()
}

/// Revisión al arrancar (a los 20 s) y cada 30 min.
pub fn spawn_loop(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(20)).await;
        loop {
            if let Err(e) = check(&app, false).await {
                log::info!("revisión de actualización: {e}");
            }
            tokio::time::sleep(CHECK_EVERY).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_con_equipo_y_canal() {
        let e = endpoint("win-0123456789abcdef", "beta");
        assert_eq!(
            e,
            "https://api.asistentealfredo.cl/api/desktop/update?target={{target}}&arch={{arch}}&current_version={{current_version}}&device_id=win-0123456789abcdef&channel=beta"
        );
        assert!(endpoint("x&y=1", "beta").contains("device_id=xy1&"));
    }

    #[test]
    fn obligatoria_espera_pedidos_o_plazo() {
        assert!(should_force(0, Duration::ZERO, 10));
        assert!(!should_force(2, Duration::from_secs(60), 10));
        assert!(should_force(2, Duration::from_secs(600), 10));
        assert!(should_force(1, Duration::ZERO, 0));
    }
}
