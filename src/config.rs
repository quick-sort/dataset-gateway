use std::env;
use std::fs;
use std::path::Path;

use std::collections::HashMap;

#[derive(Debug, Clone, serde::Deserialize)]
pub struct StorageRoute {
    pub storage_type: String,
    pub target: String,
    #[serde(default)]
    pub key_prefix: String,
    pub region: Option<String>,
    #[serde(default)]
    pub default_gzip: bool,
}

#[derive(Clone)]
pub struct AppConfig {
    pub admin_token: String,
    pub redis_url: String,
    pub listen_addr: String,
    pub rate_limit_window_secs: u64,
    pub presign_expiry_secs: u64,
    pub storage_routes: HashMap<String, StorageRoute>,
}

#[derive(serde::Deserialize)]
struct YamlConfig {
    admin_token: Option<String>,
    redis_url: Option<String>,
    redis_password: Option<String>,
    listen_addr: Option<String>,
    rate_limit_window_secs: Option<u64>,
    presign_expiry_secs: Option<u64>,
    storage_routes: Option<HashMap<String, StorageRoute>>,
}

impl AppConfig {
    pub fn load() -> anyhow::Result<Self> {
        let yaml = Self::load_yaml();

        let admin_token = env::var("ADMIN_TOKEN")
            .ok()
            .or_else(|| yaml.as_ref().and_then(|y| y.admin_token.clone()))
            .ok_or_else(|| anyhow::anyhow!("ADMIN_TOKEN required (env or config.yaml)"))?;

        let redis_password = env::var("REDIS_PASSWORD")
            .ok()
            .or_else(|| yaml.as_ref().and_then(|y| y.redis_password.clone()))
            .unwrap_or_default();

        let redis_url = env::var("REDIS_URL")
            .ok()
            .or_else(|| yaml.as_ref().and_then(|y| y.redis_url.clone()))
            .unwrap_or_else(|| "redis://localhost:6379".into());

        let redis_url = if redis_password.is_empty() {
            redis_url
        } else if redis_url.contains('@') {
            redis_url
        } else {
            redis_url.replace("redis://", &format!("redis://:{}@", redis_password))
        };

        let listen_addr = env::var("LISTEN_ADDR")
            .ok()
            .or_else(|| yaml.as_ref().and_then(|y| y.listen_addr.clone()))
            .unwrap_or_else(|| "0.0.0.0:8080".into());

        let rate_limit_window_secs = env::var("RATE_LIMIT_WINDOW_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .or_else(|| yaml.as_ref().and_then(|y| y.rate_limit_window_secs))
            .unwrap_or(60);

        let presign_expiry_secs = env::var("PRESIGN_EXPIRY_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .or_else(|| yaml.as_ref().and_then(|y| y.presign_expiry_secs))
            .unwrap_or(900);

        let storage_routes = yaml
            .as_ref()
            .and_then(|y| y.storage_routes.clone())
            .unwrap_or_default();

        Ok(Self {
            admin_token,
            redis_url,
            listen_addr,
            rate_limit_window_secs,
            presign_expiry_secs,
            storage_routes,
        })
    }

    fn load_yaml() -> Option<YamlConfig> {
        let path = env::var("CONFIG_FILE")
            .unwrap_or_else(|_| "config.yaml".into());

        if !Path::new(&path).exists() {
            return None;
        }

        fs::read_to_string(&path)
            .ok()
            .and_then(|content| serde_yaml::from_str(&content).ok())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Serialize config tests — they mutate process-wide env vars
    static CONFIG_LOCK: Mutex<()> = Mutex::new(());

    fn unique_config_path() -> String {
        format!("/tmp/dg-test-config-{}.yaml", uuid::Uuid::new_v4())
    }

    #[test]
    fn load_from_env_only() {
        let _lock = CONFIG_LOCK.lock().unwrap();
        let cfg_path = unique_config_path();

        std::env::set_var("CONFIG_FILE", &cfg_path);
        std::env::set_var("ADMIN_TOKEN", "test-token");
        std::env::set_var("REDIS_URL", "redis://localhost:6379");
        std::env::remove_var("REDIS_PASSWORD");
        std::env::remove_var("LISTEN_ADDR");
        std::env::remove_var("RATE_LIMIT_WINDOW_SECS");
        std::env::remove_var("PRESIGN_EXPIRY_SECS");

        let config = AppConfig::load().unwrap();
        assert_eq!(config.admin_token, "test-token");
        assert_eq!(config.redis_url, "redis://localhost:6379");
        assert_eq!(config.listen_addr, "0.0.0.0:8080");
        assert_eq!(config.rate_limit_window_secs, 60);
        assert_eq!(config.presign_expiry_secs, 900);

        std::env::remove_var("CONFIG_FILE");
    }

    #[test]
    fn missing_admin_token_returns_error() {
        let _lock = CONFIG_LOCK.lock().unwrap();
        let cfg_path = unique_config_path();

        std::env::set_var("CONFIG_FILE", &cfg_path);
        std::env::remove_var("ADMIN_TOKEN");

        let result = AppConfig::load();
        assert!(result.is_err());
        assert!(result.err().unwrap().to_string().contains("ADMIN_TOKEN"));

        std::env::remove_var("CONFIG_FILE");
    }

    #[test]
    fn yaml_fallback_when_env_missing() {
        let _lock = CONFIG_LOCK.lock().unwrap();

        let dir = std::env::temp_dir().join(format!("dg-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let yaml_path = dir.join("config.yaml");
        std::fs::write(&yaml_path, "admin_token: yaml-token\nlisten_addr: 127.0.0.1:9000\n").unwrap();

        std::env::set_var("CONFIG_FILE", yaml_path.to_str().unwrap());
        std::env::set_var("ADMIN_TOKEN", "yaml-token");
        std::env::remove_var("REDIS_URL");
        std::env::remove_var("REDIS_PASSWORD");
        std::env::remove_var("LISTEN_ADDR");

        let config = AppConfig::load().unwrap();
        assert_eq!(config.admin_token, "yaml-token");
        assert_eq!(config.listen_addr, "127.0.0.1:9000"); // yaml value

        std::fs::remove_dir_all(&dir).unwrap();
        std::env::remove_var("CONFIG_FILE");
    }

    #[test]
    fn env_overrides_yaml() {
        let _lock = CONFIG_LOCK.lock().unwrap();

        let dir = std::env::temp_dir().join(format!("dg-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let yaml_path = dir.join("config.yaml");
        std::fs::write(&yaml_path, "admin_token: yaml-token\nlisten_addr: 127.0.0.1:9000\n").unwrap();

        std::env::set_var("CONFIG_FILE", yaml_path.to_str().unwrap());
        std::env::set_var("ADMIN_TOKEN", "env-token");
        std::env::set_var("LISTEN_ADDR", "0.0.0.0:3000");
        std::env::remove_var("REDIS_URL");
        std::env::remove_var("REDIS_PASSWORD");

        let config = AppConfig::load().unwrap();
        assert_eq!(config.admin_token, "env-token"); // env wins over yaml
        assert_eq!(config.listen_addr, "0.0.0.0:3000"); // env wins over yaml

        std::fs::remove_dir_all(&dir).unwrap();
        std::env::remove_var("CONFIG_FILE");
    }

    #[test]
    fn redis_url_with_password_injected() {
        let _lock = CONFIG_LOCK.lock().unwrap();
        let cfg_path = unique_config_path();

        std::env::set_var("CONFIG_FILE", &cfg_path);
        std::env::set_var("ADMIN_TOKEN", "t");
        std::env::set_var("REDIS_PASSWORD", "secret");
        std::env::set_var("REDIS_URL", "redis://localhost:6379");

        let config = AppConfig::load().unwrap();
        assert_eq!(config.redis_url, "redis://:secret@localhost:6379");

        std::env::remove_var("ADMIN_TOKEN");
        std::env::remove_var("REDIS_PASSWORD");
        std::env::remove_var("REDIS_URL");
        std::env::remove_var("CONFIG_FILE");
    }

    #[test]
    fn redis_url_with_password_already_embedded() {
        let _lock = CONFIG_LOCK.lock().unwrap();
        let cfg_path = unique_config_path();

        std::env::set_var("CONFIG_FILE", &cfg_path);
        std::env::set_var("ADMIN_TOKEN", "t");
        std::env::set_var("REDIS_PASSWORD", "secret");
        std::env::set_var("REDIS_URL", "redis://:secret@redis.host:6379");

        let config = AppConfig::load().unwrap();
        assert_eq!(config.redis_url, "redis://:secret@redis.host:6379");

        std::env::remove_var("ADMIN_TOKEN");
        std::env::remove_var("REDIS_PASSWORD");
        std::env::remove_var("REDIS_URL");
        std::env::remove_var("CONFIG_FILE");
    }
}
