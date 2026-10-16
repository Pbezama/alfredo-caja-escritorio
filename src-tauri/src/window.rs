//! Ventana principal: navegación con lista blanca, páginas locales, traer al frente,
//! cerrar a la bandeja, kiosco y vigilancia de carga (sin red → offline.html).

use crate::origins::{self, NavDecision};
use crate::state::AppState;
use crate::{clock, net};
use std::sync::atomic::Ordering;
use std::time::Duration;
use tauri::webview::PageLoadEvent;
use tauri::{AppHandle, Manager, UserAttentionType, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use tauri_plugin_opener::OpenerExt;
use url::Url;

pub const MAIN: &str = "main";

/// Flags de WebView2 (Windows): alarma sin gesto y sin congelar timers en segundo plano.
/// Al fijar `additional_browser_args` se pierden los que pone Tauri: se repiten.
pub const WEBVIEW2_ARGS: &str = "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection --autoplay-policy=no-user-gesture-required --disable-background-timer-throttling --disable-renderer-backgrounding --disable-backgrounding-occluded-windows";

/// Script de inicialización: arma window.alfredoDesktop (sólo en orígenes permitidos).
pub fn init_script(boot: &serde_json::Value) -> String {
    format!(
        "window.__ALFREDO_DESKTOP_BOOT_RAW__ = {};\n{}",
        serde_json::to_string(boot).unwrap_or_else(|_| "null".into()),
        include_str!("../../src/bridge-init.js")
    )
}

pub fn main_window(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window(MAIN)
}

pub fn create(app: &AppHandle, boot: serde_json::Value) -> tauri::Result<WebviewWindow> {
    let handle = app.clone();
    let handle2 = app.clone();
    let builder = WebviewWindowBuilder::new(app, MAIN, WebviewUrl::App("index.html".into()))
        .title("Alfredo Caja")
        .inner_size(1280.0, 800.0)
        .min_inner_size(900.0, 600.0)
        .visible(true)
        .initialization_script(init_script(&boot))
        .background_throttling(tauri::utils::config::BackgroundThrottlingPolicy::Disabled)
        .on_navigation(move |url| {
            match origins::decide_navigation(url) {
                NavDecision::Allow => {
                    if origins::is_allowed_url(url) {
                        let st = handle.state::<AppState>();
                        let _ = st.update_cfg(|c| crate::config::remember_url(c, url.as_str()));
                    }
                    true
                }
                NavDecision::OpenExternal => {
                    let _ = handle.opener().open_url(url.as_str(), None::<&str>);
                    false
                }
                NavDecision::Block => false,
            }
        })
        .on_page_load(move |webview, payload| {
            if payload.event() == PageLoadEvent::Started && origins::is_allowed_url(payload.url()) {
                let st = handle2.state::<AppState>();
                let gen = st.nav_gen.fetch_add(1, Ordering::SeqCst) + 1;
                let app = handle2.clone();
                let _ = webview;
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(net::LOAD_TIMEOUT).await;
                    let st = app.state::<AppState>();
                    if st.nav_gen.load(Ordering::SeqCst) == gen && st.ready_gen.load(Ordering::SeqCst) < gen {
                        log::warn!("la web del local no cargó en {:?}", net::LOAD_TIMEOUT);
                        show_offline(&app, "timeout");
                    }
                });
            }
        })
        .additional_browser_args(WEBVIEW2_ARGS);
    let win = builder.build()?;
    let w2 = win.clone();
    let app2 = app.clone();
    win.on_window_event(move |e| {
        if let tauri::WindowEvent::CloseRequested { api, .. } = e {
            let st = app2.state::<AppState>();
            if !st.quitting.load(Ordering::SeqCst) {
                api.prevent_close();
                let _ = w2.hide();
                crate::tray::first_hide_hint(&app2);
            }
        }
    });
    Ok(win)
}

pub fn navigate_local(app: &AppHandle, page: &str) {
    if let Some(w) = main_window(app) {
        // En Windows las páginas locales son http://tauri.localhost; en macOS tauri://localhost.
        let base = if cfg!(windows) { "http://tauri.localhost/" } else { "tauri://localhost/" };
        if let Ok(u) = Url::parse(&format!("{base}{page}")) {
            let _ = w.navigate(u);
        }
    }
}

pub fn show_offline(app: &AppHandle, reason: &str) {
    let st = app.state::<AppState>();
    let attempt = st.offline_attempt.fetch_add(1, Ordering::SeqCst);
    let wait = net::retry_delay_s(attempt);
    navigate_local(app, &format!("offline.html?s={wait}&motivo={reason}"));
}

/// Arranque y reintento: reloj → alcance de la web → navegar a la última URL del local.
pub async fn go_to_store(app: AppHandle) {
    let st = app.state::<AppState>();
    let cfg = st.cfg();
    let Some(origin) = cfg.origin.clone() else {
        navigate_local(&app, "setup.html");
        return;
    };
    if let Some(skew) = clock::measure(&st.http).await {
        *st.clock_skew_ms.lock().unwrap() = Some(skew);
        if clock::is_skewed(skew) {
            navigate_local(&app, &format!("reloj.html?min={}", skew.abs() / 60_000));
            return;
        }
    }
    let https = reqwest::Client::builder().timeout(Duration::from_secs(10)).build().unwrap_or_else(|_| st.http.clone());
    match net::origin_reachable(&https, &origin).await {
        Ok(()) => {
            st.offline_attempt.store(0, Ordering::SeqCst);
            let target = cfg.last_url.clone().filter(|u| u.starts_with(&origin)).unwrap_or_else(|| format!("{origin}/"));
            if let (Some(w), Ok(u)) = (main_window(&app), Url::parse(&target)) {
                let _ = w.navigate(u);
            }
        }
        Err(reason) if reason == "certificate_date" => navigate_local(&app, "reloj.html?min=0"),
        Err(_) => show_offline(&app, "offline"),
    }
}

/// Traer al frente (RF9.5): restaurar, mostrar y enfocar; si el sistema no deja robar el
/// foco, parpadeo en la barra de tareas (o rebote del Dock) y «siempre visible» 3 s.
pub fn bring_to_front(app: &AppHandle) {
    let Some(w) = main_window(app) else { return };
    let _ = w.unminimize();
    let _ = w.show();
    let _ = w.set_focus();
    let focused = w.is_focused().unwrap_or(false);
    if !focused {
        let _ = w.request_user_attention(Some(UserAttentionType::Critical));
        let _ = w.set_always_on_top(true);
        let w2 = w.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(Duration::from_secs(3)).await;
            let _ = w2.set_always_on_top(false);
        });
    }
}

/// Kiosco: pantalla completa sin bordes; la web bloquea el menú contextual y F12.
pub fn set_kiosk(app: &AppHandle, on: bool) {
    let st = app.state::<AppState>();
    st.kiosk.store(on, Ordering::SeqCst);
    let _ = st.update_cfg(|c| c.kiosk = on);
    if let Some(w) = main_window(app) {
        let _ = w.set_decorations(!on);
        let _ = w.set_fullscreen(on);
        let _ = w.eval(&format!("window.__alfredoKiosk = {on};"));
    }
}

/// Evento hacia la web (sin permisos de eventos: eval de una función del init script).
pub fn emit_to_web(app: &AppHandle, event: &str, payload: serde_json::Value) {
    if let Some(w) = main_window(app) {
        let js = format!(
            "window.__alfredoDesktopEmit && window.__alfredoDesktopEmit({}, {});",
            serde_json::to_string(event).unwrap_or_default(),
            serde_json::to_string(&payload).unwrap_or_else(|_| "null".into())
        );
        let _ = w.eval(&js);
    }
}
