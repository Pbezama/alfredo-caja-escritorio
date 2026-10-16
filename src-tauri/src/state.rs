//! Estado compartido del escritorio.

use crate::config::{self, AppConfig};
use crate::printing::PrintLocks;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Mutex;

pub struct AppState {
    pub data_dir: PathBuf,
    pub log_dir: PathBuf,
    pub config: Mutex<AppConfig>,
    pub device_id: String,
    pub http: reqwest::Client,
    pub locks: PrintLocks,
    /// Pedidos pendientes que informó la web (notify.incomingOrders).
    pub incoming: AtomicU32,
    /// Generación de la navegación actual y la última que avisó `page_ready`.
    pub nav_gen: AtomicU64,
    pub ready_gen: AtomicU64,
    /// Intentos seguidos sin red (calendario de offline.html).
    pub offline_attempt: AtomicU32,
    pub clock_skew_ms: Mutex<Option<i64>>,
    pub quitting: AtomicBool,
    pub kiosk: AtomicBool,
    pub keep_awake: Mutex<Option<(bool, bool)>>,
    pub update: Mutex<crate::update::UpdateState>,
}

impl AppState {
    pub fn new(data_dir: PathBuf, log_dir: PathBuf, device_id: String) -> Self {
        let cfg = config::load(&data_dir);
        let kiosk = cfg.kiosk;
        let http = reqwest::Client::builder()
            .user_agent(format!("AlfredoCaja/{} ({})", env!("CARGO_PKG_VERSION"), std::env::consts::OS))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("cliente http");
        Self {
            data_dir,
            log_dir,
            config: Mutex::new(cfg),
            device_id,
            http,
            locks: PrintLocks::default(),
            incoming: AtomicU32::new(0),
            nav_gen: AtomicU64::new(0),
            ready_gen: AtomicU64::new(0),
            offline_attempt: AtomicU32::new(0),
            clock_skew_ms: Mutex::new(None),
            quitting: AtomicBool::new(false),
            kiosk: AtomicBool::new(kiosk),
            keep_awake: Mutex::new(None),
            update: Mutex::new(Default::default()),
        }
    }

    pub fn cfg(&self) -> AppConfig {
        self.config.lock().unwrap().clone()
    }

    pub fn update_cfg<F: FnOnce(&mut AppConfig)>(&self, f: F) -> std::io::Result<()> {
        let mut g = self.config.lock().unwrap();
        f(&mut g);
        config::save(&self.data_dir, &g)
    }

    pub fn incoming(&self) -> u32 {
        self.incoming.load(Ordering::SeqCst)
    }
}
