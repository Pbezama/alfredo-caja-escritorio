//! Comandos del puente `window.alfredoDesktop` (T9.3) y de las páginas locales.
//!
//! Doble control de origen (RF9.3): las capabilities sólo dejan invocar desde la web staff
//! (remote.json) o desde las páginas locales (local.json), y además cada comando revisa la
//! URL del webview que lo llama.

use crate::origins;
use crate::printing::{self, PrintError, PrinterConfig};
use crate::state::AppState;
use crate::{power, session, support, system, tray, update, window};
use base64::Engine;
use serde::Serialize;
use std::sync::atomic::Ordering;
use tauri::{AppHandle, Manager, State, Webview};
use tauri_plugin_autostart::ManagerExt as _;
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_opener::OpenerExt;

pub const BRIDGE_VERSION: u32 = 1;

fn forbidden() -> PrintError {
    PrintError::new("forbidden", "Origen no permitido")
}

fn require_staff(w: &Webview) -> Result<(), PrintError> {
    match w.url() {
        Ok(u) if origins::is_allowed_url(&u) => Ok(()),
        _ => Err(forbidden()),
    }
}

fn require_local(w: &Webview) -> Result<(), PrintError> {
    match w.url() {
        Ok(u) if origins::is_local_url(&u) => Ok(()),
        _ => Err(forbidden()),
    }
}

fn require_any(w: &Webview) -> Result<(), PrintError> {
    require_staff(w).or_else(|_| require_local(w))
}

#[tauri::command]
pub fn ping(webview: Webview) -> Result<String, PrintError> {
    require_any(&webview)?;
    Ok("pong".into())
}

#[tauri::command]
pub fn page_ready(webview: Webview, state: State<'_, AppState>) -> Result<(), PrintError> {
    require_staff(&webview)?;
    log::info!("la web del local cargó con el puente: {}", webview.url().map(|u| u.to_string()).unwrap_or_default());
    let gen = state.nav_gen.load(Ordering::SeqCst);
    state.ready_gen.store(gen.max(1), Ordering::SeqCst);
    state.offline_attempt.store(0, Ordering::SeqCst);
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Info {
    bridge_version: u32,
    app_version: String,
    device_id: String,
    platform: &'static str,
    os: String,
    webview: String,
    webview2: String,
    hostname: Option<String>,
    battery: Option<system::Battery>,
    volume: Option<system::Volume>,
    network: system::Network,
    memory_free_mb: u64,
    disk_free_mb: u64,
    clock_skew_ms: Option<i64>,
    autostart: bool,
    kiosk: bool,
    origin: Option<String>,
    channel: String,
}

pub async fn build_info(app: &AppHandle) -> Info {
    let st = app.state::<AppState>();
    let snap = tauri::async_runtime::spawn_blocking(system::snapshot).await.ok();
    let cfg = st.cfg();
    let skew = *st.clock_skew_ms.lock().unwrap();
    let kiosk = st.kiosk.load(Ordering::SeqCst);
    let device_id = st.device_id.clone();
    let wv = tauri::webview_version().unwrap_or_default();
    let snap = snap.unwrap_or_else(|| system::Snapshot {
        os: std::env::consts::OS.into(),
        hostname: None,
        battery: None,
        volume: None,
        network: system::Network { kind: "unknown".into(), wifi_strength: None },
        memory_free_mb: 0,
        disk_free_mb: 0,
    });
    Info {
        bridge_version: BRIDGE_VERSION,
        app_version: app.package_info().version.to_string(),
        device_id,
        platform: if cfg!(target_os = "macos") { "macos" } else { "windows" },
        os: snap.os,
        webview: wv.clone(),
        webview2: wv,
        hostname: snap.hostname,
        battery: snap.battery,
        volume: snap.volume,
        network: snap.network,
        memory_free_mb: snap.memory_free_mb,
        disk_free_mb: snap.disk_free_mb,
        clock_skew_ms: skew,
        autostart: app.autolaunch().is_enabled().unwrap_or(false),
        kiosk,
        origin: cfg.origin,
        channel: cfg.channel,
    }
}

#[tauri::command]
pub async fn desktop_info(webview: Webview, app: AppHandle) -> Result<Info, PrintError> {
    require_staff(&webview)?;
    Ok(build_info(&app).await)
}

// ── Impresoras ──────────────────────────────────────────────────────────────

#[tauri::command]
pub fn printers_list(webview: Webview, state: State<'_, AppState>) -> Result<Vec<PrinterConfig>, PrintError> {
    require_staff(&webview)?;
    Ok(state.cfg().printers)
}

#[tauri::command]
pub fn printers_save(webview: Webview, state: State<'_, AppState>, cfg: PrinterConfig) -> Result<(), PrintError> {
    require_staff(&webview)?;
    cfg.validate()?;
    state
        .update_cfg(|c| {
            if let Some(p) = c.printers.iter_mut().find(|p| p.target.key == cfg.target.key) {
                *p = cfg.clone();
            } else if c.printers.len() < 8 {
                c.printers.push(cfg.clone());
            }
        })
        .map_err(|e| PrintError::new("error", e.to_string()))
}

#[tauri::command]
pub fn printers_remove(webview: Webview, state: State<'_, AppState>, key: String) -> Result<(), PrintError> {
    require_staff(&webview)?;
    state.update_cfg(|c| c.printers.retain(|p| p.target.key != key)).map_err(|e| PrintError::new("error", e.to_string()))
}

#[tauri::command]
pub async fn printers_discover(webview: Webview, network: Option<bool>) -> Result<Vec<printing::PrinterTarget>, PrintError> {
    require_staff(&webview)?;
    let network = network.unwrap_or(true);
    tauri::async_runtime::spawn_blocking(move || printing::discover::discover(network)).await.map_err(|e| PrintError::new("error", e.to_string()))
}

#[tauri::command]
pub async fn printers_print(webview: Webview, app: AppHandle, key: String, data: String, copies: Option<u8>) -> Result<(), PrintError> {
    require_staff(&webview)?;
    let st = app.state::<AppState>();
    let cfg = st.cfg().printers.into_iter().find(|p| p.target.key == key).ok_or_else(|| PrintError::new("not_found", "Esa impresora no está configurada en este equipo"))?;
    let bytes = base64::engine::general_purpose::STANDARD.decode(data.as_bytes()).map_err(|_| PrintError::new("bad_request", "Datos inválidos"))?;
    let copies = copies.unwrap_or(cfg.copies).max(1);
    let r = printing::print(&st.locks, cfg.target.clone(), bytes, copies).await;
    if let Err(e) = &r {
        log::warn!("impresión en «{}» falló: {e}", cfg.target.name);
        window::emit_to_web(&app, "printer-status", serde_json::json!({"key": key, "status": e.code}));
    }
    r
}

#[tauri::command]
pub async fn printers_status(webview: Webview, state: State<'_, AppState>, key: String) -> Result<String, PrintError> {
    require_staff(&webview)?;
    let cfg = state.cfg().printers.into_iter().find(|p| p.target.key == key).ok_or_else(|| PrintError::new("not_found", "Esa impresora no está configurada"))?;
    Ok(printing::printer_status(cfg.target).await)
}

// ── Bandeja, ventana y energía ─────────────────────────────────────────────

#[tauri::command]
pub fn notify_incoming(webview: Webview, app: AppHandle, count: u32, text: Option<String>) -> Result<(), PrintError> {
    require_staff(&webview)?;
    let st = app.state::<AppState>();
    let prev = st.incoming.swap(count, Ordering::SeqCst);
    tray::set_pending(&app, count);
    if count > prev {
        let body = text.filter(|t| !t.trim().is_empty() && t.len() < 200).unwrap_or_else(|| "Tienes nuevas órdenes".into());
        tray::notify(&app, "Nuevo pedido", &body);
    }
    Ok(())
}

#[tauri::command]
pub fn notify_clear(webview: Webview, app: AppHandle) -> Result<(), PrintError> {
    require_staff(&webview)?;
    app.state::<AppState>().incoming.store(0, Ordering::SeqCst);
    tray::set_pending(&app, 0);
    Ok(())
}

#[tauri::command]
pub fn window_bring_to_front(webview: Webview, app: AppHandle) -> Result<(), PrintError> {
    require_staff(&webview)?;
    window::bring_to_front(&app);
    Ok(())
}

#[tauri::command]
pub fn window_set_kiosk(webview: Webview, app: AppHandle, on: bool) -> Result<(), PrintError> {
    require_staff(&webview)?;
    window::set_kiosk(&app, on);
    Ok(())
}

#[tauri::command]
pub fn power_keep_awake(webview: Webview, app: AppHandle, on: bool, display: Option<bool>) -> Result<(), PrintError> {
    require_staff(&webview)?;
    let display = display.unwrap_or(true);
    *app.state::<AppState>().keep_awake.lock().unwrap() = Some((on, display));
    // SetThreadExecutionState es por hilo: siempre en el hilo principal.
    let _ = app.run_on_main_thread(move || power::apply(on, display));
    Ok(())
}

// ── Actualización ───────────────────────────────────────────────────────────

#[tauri::command]
pub async fn update_check(webview: Webview, app: AppHandle) -> Result<update::CheckOut, PrintError> {
    require_staff(&webview)?;
    update::check(&app, true).await.map_err(|e| PrintError::new("error", e))
}

#[tauri::command]
pub async fn update_install(webview: Webview, app: AppHandle) -> Result<(), PrintError> {
    require_any(&webview)?;
    update::install(&app).await.map_err(|e| PrintError::new("error", e))
}

// ── Soporte ─────────────────────────────────────────────────────────────────

#[derive(Serialize, Default)]
pub struct SupportOut {
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

pub async fn start_support(app: &AppHandle) -> SupportOut {
    let st = app.state::<AppState>();
    let cfg = st.cfg();
    let url = cfg.support_tool_url.clone().unwrap_or_else(|| support::DEFAULT_URL.to_string());
    let sha = cfg.support_tool_sha256.clone().unwrap_or_else(|| support::DEFAULT_SHA256.to_string());
    if url.is_empty() {
        let _ = app.opener().open_url(support::DOWNLOAD_PAGE, None::<&str>);
        return SupportOut { id: None, message: Some("Descarga RustDesk desde la página que se abrió y dicta su ID a soporte.".into()) };
    }
    let dir = st.data_dir.join("soporte");
    let file = dir.join(url.rsplit('/').next().unwrap_or("rustdesk.exe"));
    if !support::file_matches(&file, &sha) {
        let _ = std::fs::create_dir_all(&dir);
        let client = reqwest::Client::builder().redirect(reqwest::redirect::Policy::limited(10)).build().unwrap_or_default();
        let bytes = match client.get(&url).send().await {
            Ok(r) if r.status().is_success() => r.bytes().await.ok(),
            _ => None,
        };
        let Some(bytes) = bytes else {
            return SupportOut { id: None, message: Some("No se pudo bajar la herramienta de soporte. Revisa la conexión.".into()) };
        };
        if !support::sha256_hex(&bytes).eq_ignore_ascii_case(sha.trim()) {
            log::error!("hash de la herramienta de soporte no coincide: no se ejecuta");
            return SupportOut { id: None, message: Some("La herramienta de soporte descargada no es la esperada y no se abrió. Avísale a soporte.".into()) };
        }
        let _ = std::fs::write(&file, &bytes);
    }
    if let Err(e) = app.opener().open_path(file.to_string_lossy().to_string(), None::<&str>) {
        return SupportOut { id: None, message: Some(format!("No se pudo abrir la herramienta de soporte: {e}")) };
    }
    // RustDesk escribe su ID en su configuración al arrancar.
    for _ in 0..20 {
        if let Some(id) = support::read_rustdesk_id() {
            let shown = support::format_id(&id);
            return SupportOut { id: Some(id), message: Some(format!("Dile este número a soporte: {shown}")) };
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    SupportOut { id: None, message: Some("Se abrió RustDesk: dicta a soporte el ID que aparece en su ventana.".into()) }
}

#[tauri::command]
pub async fn support_start_remote(webview: Webview, app: AppHandle) -> Result<SupportOut, PrintError> {
    require_any(&webview)?;
    Ok(start_support(&app).await)
}

pub async fn copy_diagnostics(app: &AppHandle, extra: Option<String>) -> Result<(), String> {
    let info = build_info(app).await;
    let st = app.state::<AppState>();
    let printers = st.cfg().printers;
    let mut lines: Vec<(&str, String)> = vec![
        ("Versión", info.app_version.clone()),
        ("Equipo", info.device_id.clone()),
        ("Sistema", info.os.clone()),
        ("WebView", info.webview.clone()),
        ("Local", info.origin.clone().unwrap_or_else(|| "(sin configurar)".into())),
        ("Canal", info.channel.clone()),
        ("Red", format!("{} {}", info.network.kind, info.network.wifi_strength.map(|s| format!("{s}%")).unwrap_or_default())),
        ("Reloj", info.clock_skew_ms.map(|s| format!("{} s de desfase", s / 1000)).unwrap_or_else(|| "sin medir".into())),
        ("Pedidos pendientes", st.incoming().to_string()),
    ];
    for p in &printers {
        let status = printing::printer_status(p.target.clone()).await;
        lines.push(("Impresora", format!("{} ({:?}) · {} · {}", p.target.name, p.target.kind, p.docs.join(","), status)));
    }
    if let Some(x) = extra.filter(|x| x.len() < 4000) {
        lines.push(("Web", x));
    }
    let log = std::fs::read_dir(&st.log_dir)
        .ok()
        .and_then(|d| d.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().map(|x| x == "log").unwrap_or(false)).max_by_key(|p| p.metadata().and_then(|m| m.modified()).ok()))
        .map(|p| support::tail(&p, 50))
        .unwrap_or_default();
    let text = support::diagnostics_text(&lines, &log);
    app.clipboard().write_text(text).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn support_copy_diagnostics(webview: Webview, app: AppHandle, extra: Option<String>) -> Result<(), PrintError> {
    require_any(&webview)?;
    copy_diagnostics(&app, extra).await.map_err(|e| PrintError::new("error", e))
}

// ── Sesión de la persona ───────────────────────────────────────────────────

#[tauri::command]
pub fn session_remember(webview: Webview, token: Option<String>, until: Option<String>) -> Result<(), PrintError> {
    require_staff(&webview)?;
    session::remember(token.as_deref(), until.as_deref()).map_err(|e| PrintError::new("error", e))
}

#[tauri::command]
pub fn session_recall(webview: Webview) -> Result<Option<session::Remembered>, PrintError> {
    require_staff(&webview)?;
    Ok(session::recall())
}

pub fn apply_autostart(app: &AppHandle, on: bool) {
    let r = if on { app.autolaunch().enable() } else { app.autolaunch().disable() };
    if let Err(e) = r {
        log::warn!("inicio automático: {e}");
    }
    let _ = app.state::<AppState>().update_cfg(|c| c.autostart = on);
    if let Some(items) = app.try_state::<tray::TrayItems>() {
        let _ = items.autostart.set_checked(on);
    }
}

#[tauri::command]
pub fn settings_set_autostart(webview: Webview, app: AppHandle, on: bool) -> Result<(), PrintError> {
    require_staff(&webview)?;
    apply_autostart(&app, on);
    Ok(())
}

// ── Páginas locales ─────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct SetupOut {
    slug: String,
    origin: String,
    name: Option<String>,
}

/// Verifica el local con `GET /api/settings/store-config` y `X-Tenant` (mismo chequeo
/// que ConfigurarDispositivoScreen de la Caja Android).
#[tauri::command]
pub async fn setup_validate(webview: Webview, state: State<'_, AppState>, input: String) -> Result<SetupOut, PrintError> {
    require_local(&webview)?;
    let slug = origins::slug_from_input(&input).ok_or_else(|| PrintError::new("bad_request", "Escribe la dirección de tu local, por ejemplo «milocal» o milocal.app.asistentealfredo.cl"))?;
    let res = state
        .http
        .get(format!("{}/settings/store-config", origins::API_BASE))
        .header("X-Tenant", &slug)
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .map_err(|_| PrintError::new("offline", "No hay conexión con Alfredo. Revisa el internet del equipo."))?;
    if res.status().as_u16() == 404 || res.status().as_u16() == 400 {
        return Err(PrintError::new("not_found", format!("No encontramos el local «{slug}». Revisa cómo se escribe.")));
    }
    if !res.status().is_success() {
        return Err(PrintError::new("error", format!("Alfredo respondió {}. Intenta de nuevo.", res.status())));
    }
    let body: serde_json::Value = res.json().await.unwrap_or_default();
    let name = ["name", "restaurant_name", "store_name"].iter().find_map(|k| body.get(*k).and_then(|v| v.as_str()).map(String::from));
    Ok(SetupOut { origin: origins::origin_for_slug(&slug), slug, name })
}

#[tauri::command]
pub async fn setup_save(webview: Webview, app: AppHandle, origin: String) -> Result<(), PrintError> {
    require_local(&webview)?;
    let slug = origins::slug_of_origin(&origin).ok_or_else(|| PrintError::new("bad_request", "Origen inválido"))?;
    let origin = origins::origin_for_slug(&slug);
    let st = app.state::<AppState>();
    let changed = st.cfg().origin.as_deref() != Some(origin.as_str());
    st.update_cfg(|c| {
        c.origin = Some(origin.clone());
        c.last_url = Some(format!("{origin}/account-login"));
        if changed {
            c.printers.clear();
        }
    })
    .map_err(|e| PrintError::new("error", e.to_string()))?;
    if changed {
        let _ = session::remember(None, None);
    }
    window::go_to_store(app.clone()).await;
    Ok(())
}

#[tauri::command]
pub async fn shell_retry(webview: Webview, app: AppHandle) -> Result<(), PrintError> {
    require_local(&webview)?;
    window::go_to_store(app).await;
    Ok(())
}

#[tauri::command]
pub fn shell_open_clock_settings(webview: Webview, app: AppHandle) -> Result<(), PrintError> {
    require_local(&webview)?;
    let target = if cfg!(windows) { "ms-settings:dateandtime" } else { "x-apple.systempreferences:com.apple.Date-Time-Settings.extension" };
    app.opener().open_url(target, None::<&str>).map_err(|e| PrintError::new("error", e.to_string()))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShellState {
    origin: Option<String>,
    version: String,
    device_id: String,
    update_version: Option<String>,
    platform: &'static str,
}

#[tauri::command]
pub fn shell_state(webview: Webview, app: AppHandle) -> Result<ShellState, PrintError> {
    require_local(&webview)?;
    let st = app.state::<AppState>();
    let uv = st.update.lock().unwrap().available.as_ref().map(|u| u.version.clone());
    Ok(ShellState { origin: st.cfg().origin, version: app.package_info().version.to_string(), device_id: st.device_id.clone(), update_version: uv, platform: if cfg!(target_os = "macos") { "macos" } else { "windows" } })
}

/// «Salir» ya confirmado por la página. Si hay una actualización descargada, se instala.
#[tauri::command]
pub async fn shell_quit(webview: Webview, app: AppHandle) -> Result<(), PrintError> {
    require_any(&webview)?;
    let st = app.state::<AppState>();
    st.quitting.store(true, Ordering::SeqCst);
    let _ = app.run_on_main_thread(|| power::apply(false, false));
    let has_update = st.update.lock().unwrap().bytes.is_some();
    if has_update && update::install(&app).await.is_ok() {
        return Ok(());
    }
    app.exit(0);
    Ok(())
}
