//! Alfredo Caja de escritorio (F9 del plan de paridad con Go: «GoWin propio»).
//!
//! Envuelve la web staff del local (`https://{slug}.app.asistentealfredo.cl`) con un puente
//! nativo `window.alfredoDesktop`: impresión directa, bandeja, traer al frente, alarma sin
//! gesto, anti-suspensión, auto-update ligado a la política de F0, telemetría y soporte remoto.
//! Windows es el objetivo de F9; lo de macOS queda como base para F10 (mismo proyecto,
//! ramas `#[cfg(target_os = "macos")]`).

pub mod bridge;
pub mod clock;
pub mod config;
pub mod device;
pub mod net;
pub mod origins;
pub mod power;
pub mod printing;
pub mod session;
pub mod state;
pub mod support;
pub mod system;
pub mod tray;
pub mod update;
pub mod window;

use state::AppState;
use std::sync::atomic::Ordering;
use std::time::{Duration, SystemTime};
use tauri::Manager;
use tauri_plugin_autostart::{MacosLauncher, ManagerExt as _};

/// Sentry de los panics de Rust (RF9.14). Sin DSN al compilar ni en el entorno, no hace nada.
fn init_sentry() -> Option<sentry::ClientInitGuard> {
    let dsn = std::env::var("ALFREDO_DESKTOP_SENTRY_DSN").ok().or_else(|| option_env!("ALFREDO_DESKTOP_SENTRY_DSN").map(String::from))?;
    if dsn.trim().is_empty() {
        return None;
    }
    Some(sentry::init((
        dsn,
        sentry::ClientOptions {
            release: Some(format!("alfredo-escritorio@{}", env!("CARGO_PKG_VERSION")).into()),
            environment: Some("production".into()),
            ..Default::default()
        },
    )))
}

fn handle_args(app: &tauri::AppHandle, args: &[String]) {
    if args.iter().any(|a| a == "--soporte") {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            window::bring_to_front(&app);
            let r = bridge::start_support(&app).await;
            tray::notify(&app, "Soporte remoto", &r.message.unwrap_or_default());
        });
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let _sentry = init_sentry();
    let log_plugin = tauri_plugin_log::Builder::new()
        .level(log::LevelFilter::Info)
        .max_file_size(2_000_000)
        .rotation_strategy(tauri_plugin_log::RotationStrategy::KeepSome(7))
        .clear_targets()
        .target(tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::Stdout))
        .target(tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::LogDir { file_name: Some("alfredo-caja".into()) }))
        .build();

    tauri::Builder::default()
        // Instancia única (RF9.1): la segunda ejecución trae la ventana y sale.
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            window::bring_to_front(app);
            handle_args(app, &args);
        }))
        .plugin(log_plugin)
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, Some(vec!["--autostart"])))
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .with_state_flags(tauri_plugin_window_state::StateFlags::SIZE | tauri_plugin_window_state::StateFlags::POSITION | tauri_plugin_window_state::StateFlags::MAXIMIZED)
                .build(),
        )
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            bridge::ping,
            bridge::page_ready,
            bridge::desktop_info,
            bridge::printers_list,
            bridge::printers_save,
            bridge::printers_remove,
            bridge::printers_discover,
            bridge::printers_print,
            bridge::printers_status,
            bridge::printers_create_queue,
            bridge::notify_incoming,
            bridge::notify_clear,
            bridge::window_bring_to_front,
            bridge::window_set_kiosk,
            bridge::power_keep_awake,
            bridge::update_check,
            bridge::update_install,
            bridge::support_start_remote,
            bridge::support_copy_diagnostics,
            bridge::session_remember,
            bridge::session_recall,
            bridge::settings_set_autostart,
            bridge::setup_validate,
            bridge::setup_save,
            bridge::shell_retry,
            bridge::shell_open_clock_settings,
            bridge::shell_state,
            bridge::shell_quit,
        ])
        .setup(|app| {
            let handle = app.handle().clone();
            let data_dir = app.path().app_data_dir()?;
            let log_dir = app.path().app_log_dir().unwrap_or_else(|_| data_dir.join("logs"));
            std::fs::create_dir_all(&data_dir)?;
            let device_id = device::machine_device_id().unwrap_or_else(|| {
                // Sin id de máquina: uno propio, persistido en device.json.
                let p = data_dir.join("device.json");
                std::fs::read_to_string(&p).ok().filter(|s| !s.is_empty()).unwrap_or_else(|| {
                    let id = device::derive_device_id(device::prefix(), &format!("{:?}{}", SystemTime::now(), std::process::id()));
                    let _ = std::fs::write(&p, &id);
                    id
                })
            });
            sentry::configure_scope(|s| {
                s.set_tag("device_id", &device_id);
                s.set_tag("os", std::env::consts::OS);
            });
            let st = AppState::new(data_dir, log_dir, device_id);
            let autostart = st.cfg().autostart;
            app.manage(st);

            // Inicio automático activado por defecto (desactivable desde la bandeja o «Este equipo»).
            let enabled = handle.autolaunch().is_enabled().unwrap_or(false);
            if autostart && !enabled {
                let _ = handle.autolaunch().enable();
            }

            // La sesión de la persona del turno, si no venció, va inline en el init script.
            let boot = match session::recall() {
                Some(r) => serde_json::json!({ "personToken": r.token, "personUntil": r.until }),
                None => serde_json::json!({}),
            };
            let win = window::create(&handle, boot)?;
            if app.state::<AppState>().kiosk.load(Ordering::SeqCst) {
                window::set_kiosk(&handle, true);
            }
            // Arrancó solo con el sistema: igual se muestra (es la caja del local).
            let _ = win.show();
            tray::create(&handle)?;
            tauri::async_runtime::spawn(window::go_to_store(handle.clone()));
            update::spawn_loop(handle.clone());

            // Vuelta de una suspensión: el reloj saltó más de 30 s entre dos marcas de 5 s.
            let h2 = handle.clone();
            std::thread::spawn(move || {
                let mut prev = SystemTime::now();
                loop {
                    std::thread::sleep(Duration::from_secs(5));
                    let now = SystemTime::now();
                    if power::slept_between(prev, now) {
                        log::info!("el equipo volvió de una suspensión");
                        window::emit_to_web(&h2, "resume-from-sleep", serde_json::json!({}));
                        // Re-aplica la anti-suspensión (algunos drivers la sueltan al dormir).
                        if let Some((on, display)) = *h2.state::<AppState>().keep_awake.lock().unwrap() {
                            let _ = h2.run_on_main_thread(move || power::apply(on, display));
                        }
                    }
                    prev = now;
                }
            });

            let args: Vec<String> = std::env::args().collect();
            handle_args(&handle, &args);
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error al iniciar Alfredo Caja")
        .run(|app, event| {
            if let tauri::RunEvent::ExitRequested { api, code, .. } = event {
                // Cerrar la última ventana no termina la app (sigue en la bandeja); sólo «Salir».
                if code.is_none() && !app.state::<AppState>().quitting.load(Ordering::SeqCst) {
                    api.prevent_exit();
                }
            }
        });
}
