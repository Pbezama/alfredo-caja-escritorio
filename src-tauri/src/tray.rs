//! Bandeja del sistema (T9.2 §3): ícono de Alfredo con punto naranjo si hay pedidos
//! pendientes, globo «Nuevo pedido · Tienes nuevas órdenes» y el menú del PRD §7.

use crate::state::AppState;
use crate::window;
use tauri::image::Image;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Wry};
use tauri_plugin_autostart::ManagerExt as _;
use tauri_plugin_notification::NotificationExt;

pub const TRAY_ID: &str = "alfredo";

pub struct TrayItems {
    pub pedido: MenuItem<Wry>,
    pub autostart: CheckMenuItem<Wry>,
    pub update: MenuItem<Wry>,
}

fn icon(pending: bool) -> Option<Image<'static>> {
    let bytes: &'static [u8] = if pending { include_bytes!("../icons/tray-32-pedido.png") } else { include_bytes!("../icons/tray-32.png") };
    Image::from_bytes(bytes).ok()
}

/// Texto del ítem y del tooltip según los pendientes.
pub fn pending_label(count: u32) -> String {
    match count {
        0 => "Sin pedidos nuevos".into(),
        1 => "Nuevo pedido (1)".into(),
        n => format!("Nuevos pedidos ({n})"),
    }
}

pub fn create(app: &AppHandle) -> tauri::Result<TrayIcon> {
    let abrir = MenuItem::with_id(app, "open", "Abrir Alfredo", true, None::<&str>)?;
    let pedido = MenuItem::with_id(app, "pedido", pending_label(0), false, None::<&str>)?;
    let prueba = MenuItem::with_id(app, "print-test", "Imprimir prueba", true, None::<&str>)?;
    let soporte = MenuItem::with_id(app, "support", "Iniciar soporte remoto", true, None::<&str>)?;
    let diag = MenuItem::with_id(app, "diagnostics", "Copiar diagnóstico", true, None::<&str>)?;
    let update = MenuItem::with_id(app, "update", "Buscar actualización", true, None::<&str>)?;
    let auto_on = app.autolaunch().is_enabled().unwrap_or(false);
    let autostart = CheckMenuItem::with_id(app, "autostart", if cfg!(target_os = "macos") { "Abrir al iniciar sesión" } else { "Iniciar con Windows" }, true, auto_on, None::<&str>)?;
    let cambiar = MenuItem::with_id(app, "change-store", "Cambiar de local…", true, None::<&str>)?;
    let salir = MenuItem::with_id(app, "quit", "Salir", true, None::<&str>)?;
    let sep = || PredefinedMenuItem::separator(app);
    let menu = Menu::with_items(app, &[&abrir, &pedido, &sep()?, &prueba, &soporte, &diag, &update, &sep()?, &autostart, &cambiar, &sep()?, &salir])?;
    app.manage(TrayItems { pedido: pedido.clone(), autostart: autostart.clone(), update: update.clone() });

    let mut b = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("Alfredo Caja")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, ev| on_menu(app, ev.id().as_ref()))
        .on_tray_icon_event(|tray, ev| match ev {
            TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } | TrayIconEvent::DoubleClick { button: MouseButton::Left, .. } => {
                window::bring_to_front(tray.app_handle());
            }
            _ => {}
        });
    if let Some(i) = icon(false) {
        b = b.icon(i);
    }
    b.build(app)
}

pub fn on_menu(app: &AppHandle, id: &str) {
    match id {
        "open" | "pedido" => window::bring_to_front(app),
        "print-test" => {
            window::bring_to_front(app);
            window::emit_to_web(app, "tray-action", serde_json::json!({"action": "print-test"}));
        }
        "support" => {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                let r = crate::bridge::start_support(&app).await;
                notify(&app, "Soporte remoto", &r.message.unwrap_or_default());
            });
        }
        "diagnostics" => {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                let _ = crate::bridge::copy_diagnostics(&app, None).await;
                notify(&app, "Alfredo Caja", "Diagnóstico copiado. Pégalo en el chat con soporte.");
            });
        }
        "update" => {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                match crate::update::check(&app, true).await {
                    Ok(c) if c.status == "ok" => notify(&app, "Alfredo Caja", "Estás en la última versión."),
                    Ok(c) => notify(&app, "Actualización disponible", &format!("Alfredo Caja {} · ¡Haz clic aquí para instalarla!", c.version.unwrap_or_default())),
                    Err(e) => notify(&app, "Alfredo Caja", &format!("No se pudo buscar la actualización: {e}")),
                }
            });
        }
        "autostart" => {
            let on = !app.autolaunch().is_enabled().unwrap_or(false);
            crate::bridge::apply_autostart(app, on);
        }
        "change-store" => {
            window::bring_to_front(app);
            window::navigate_local(app, "setup.html?cambiar=1");
        }
        "quit" => {
            // La confirmación la muestra la página («¿Salir? Dejarás de recibir avisos de
            // pedidos en este equipo») y llama a shell_quit.
            window::bring_to_front(app);
            window::emit_to_web(app, "tray-action", serde_json::json!({"action": "quit"}));
        }
        _ => {}
    }
}

/// Punto naranjo, tooltip e ítem del menú según los pendientes.
pub fn set_pending(app: &AppHandle, count: u32) {
    if let Some(t) = app.tray_by_id(TRAY_ID) {
        let _ = t.set_icon(icon(count > 0));
        let _ = t.set_tooltip(Some(if count > 0 { format!("Alfredo Caja · {}", pending_label(count)) } else { "Alfredo Caja".to_string() }));
    }
    if let Some(items) = app.try_state::<TrayItems>() {
        let _ = items.pedido.set_text(pending_label(count));
        let _ = items.pedido.set_enabled(count > 0);
    }
}

pub fn notify(app: &AppHandle, title: &str, body: &str) {
    let _ = app.notification().builder().title(title).body(body).show();
}

/// La primera vez que la X oculta la ventana: «Alfredo sigue recibiendo pedidos en la bandeja».
pub fn first_hide_hint(app: &AppHandle) {
    let st = app.state::<AppState>();
    if !st.cfg().tray_hint_shown {
        let _ = st.update_cfg(|c| c.tray_hint_shown = true);
        notify(app, "Alfredo Caja", "Alfredo sigue recibiendo pedidos en la bandeja. Para cerrarlo, usa «Salir» en el ícono.");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn etiqueta_de_pendientes() {
        assert_eq!(pending_label(0), "Sin pedidos nuevos");
        assert_eq!(pending_label(1), "Nuevo pedido (1)");
        assert_eq!(pending_label(3), "Nuevos pedidos (3)");
    }
}
