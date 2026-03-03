use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, RwLock};

pub const VERSION: &str = "1.2.4";
pub const DEFAULT_SERVER_URL: &str = "https://api.minerva-archive.org";
pub const DEFAULT_UPLOAD_SERVER_URL: &str = "https://gate.minerva-archive.org";
pub const UPLOAD_CHUNK_SIZE: usize = 8 * 1024 * 1024;
pub const MAX_RETRIES: u32 = 3;
pub const RETRY_DELAY_SECS: u64 = 5;
pub const QUEUE_PREFETCH: usize = 2;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub server_url: String,
    pub upload_server_url: String,
    pub concurrency: usize,
    pub batch_size: usize,
    #[serde(skip)]
    pub temp_dir: PathBuf,
    #[serde(skip)]
    pub token_path: PathBuf,
    #[serde(skip)]
    pub settings_path: PathBuf,
}

impl Default for Config {
    fn default() -> Self {
        let base = app_data_dir();
        Self {
            server_url: DEFAULT_SERVER_URL.to_string(),
            upload_server_url: DEFAULT_UPLOAD_SERVER_URL.to_string(),
            concurrency: 2,
            batch_size: 10,
            temp_dir: base.join("tmp"),
            token_path: base.join("token"),
            settings_path: base.join("settings.json"),
        }
    }
}

impl Config {
    pub fn load() -> Self {
        let mut config = Config::default();
        if config.settings_path.exists() {
            if let Ok(data) = std::fs::read_to_string(&config.settings_path) {
                if let Ok(saved) = serde_json::from_str::<Config>(&data) {
                    config.server_url = saved.server_url;
                    config.upload_server_url = saved.upload_server_url;
                    config.concurrency = saved.concurrency.max(1).min(10);
                    config.batch_size = saved.batch_size.max(1).min(50);
                }
            }
        }
        config
    }

    pub fn save(&self) -> Result<(), std::io::Error> {
        std::fs::create_dir_all(self.settings_path.parent().unwrap())?;
        let data = serde_json::to_string_pretty(self)?;
        std::fs::write(&self.settings_path, data)
    }
}

pub struct WorkerStats {
    pub jobs_completed: u64,
    pub jobs_failed: u64,
    pub bytes_downloaded: u64,
    pub bytes_uploaded: u64,
}

impl Default for WorkerStats {
    fn default() -> Self {
        Self {
            jobs_completed: 0,
            jobs_failed: 0,
            bytes_downloaded: 0,
            bytes_uploaded: 0,
        }
    }
}

pub struct AppState {
    pub config: Config,
    pub token: RwLock<Option<String>>,
    pub running: AtomicBool,
    pub stats: Mutex<WorkerStats>,
}

impl AppState {
    pub fn new() -> Self {
        let config = Config::load();
        let token = load_token(&config.token_path);
        Self {
            config,
            token: RwLock::new(token),
            running: AtomicBool::new(false),
            stats: Mutex::new(WorkerStats::default()),
        }
    }

    pub fn get_token(&self) -> Option<String> {
        self.token.read().unwrap().clone()
    }

    pub fn set_token(&self, token: String) {
        *self.token.write().unwrap() = Some(token.clone());
        let _ = save_token(&self.config.token_path, &token);
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    pub fn set_running(&self, val: bool) {
        self.running.store(val, Ordering::Relaxed);
    }
}

fn app_data_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("minerva-dpn")
}

fn load_token(path: &PathBuf) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
}

fn save_token(path: &PathBuf, token: &str) -> Result<(), std::io::Error> {
    std::fs::create_dir_all(path.parent().unwrap())?;
    std::fs::write(path, token)
}
