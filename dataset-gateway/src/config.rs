use std::env;

#[derive(Clone)]
pub struct AppConfig {
    pub redis_url: String,
    pub redis_password: String,
    pub listen_addr: String,
    pub aws_region: String,
    pub rate_limit_window_ms: u64,
}

impl AppConfig {
    pub fn from_env() -> anyhow::Result<Self> {
        let redis_url = env::var("REDIS_URL").unwrap_or_else(|_| "redis://localhost:6379".into());
        let redis_password = env::var("REDIS_PASSWORD")
            .map_err(|_| anyhow::anyhow!("REDIS_PASSWORD is required"))?;
        let listen_addr = env::var("LISTEN_ADDR").unwrap_or_else(|_| "0.0.0.0:8080".into());
        let aws_region = env::var("AWS_REGION").unwrap_or_else(|_| "us-east-1".into());
        let rate_limit_window_ms: u64 = env::var("RATE_LIMIT_WINDOW_MS")
            .unwrap_or_else(|_| "1000".into())
            .parse()
            .unwrap_or(1000);

        Ok(Self {
            redis_url,
            redis_password,
            listen_addr,
            aws_region,
            rate_limit_window_ms,
        })
    }
}
