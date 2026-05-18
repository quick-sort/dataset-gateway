use redis::aio::ConnectionManager;
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeyConfig {
    pub prefixes: Vec<String>,
    pub rate_limit: u64,
}

pub struct RedisStore {
    conn: ConnectionManager,
    rate_limit_window_secs: u64,
}

impl RedisStore {
    pub async fn new(redis_url: &str, rate_limit_window_secs: u64) -> anyhow::Result<Self> {
        let client = redis::Client::open(redis_url)?;
        let conn = ConnectionManager::new(client).await?;
        Ok(Self {
            conn,
            rate_limit_window_secs,
        })
    }

    pub async fn get_api_key(&self, key: &str) -> anyhow::Result<Option<ApiKeyConfig>> {
        let mut conn = self.conn.clone();
        let data: Option<String> = conn.get(format!("apikey:{}", key)).await?;
        match data {
            Some(json) => Ok(Some(serde_json::from_str(&json)?)),
            None => Ok(None),
        }
    }

    pub async fn create_api_key(&self, key: &str, config: &ApiKeyConfig) -> anyhow::Result<()> {
        let mut conn = self.conn.clone();
        let json = serde_json::to_string(config)?;
        let _: () = conn.set(format!("apikey:{}", key), json).await?;
        Ok(())
    }

    pub async fn delete_api_key(&self, key: &str) -> anyhow::Result<bool> {
        let mut conn = self.conn.clone();
        let deleted: bool = conn.del(format!("apikey:{}", key)).await?;
        Ok(deleted)
    }

    pub async fn list_api_keys(&self) -> anyhow::Result<Vec<String>> {
        let mut conn = self.conn.clone();
        let keys: Vec<String> = conn.keys("apikey:*").await?;
        Ok(keys.into_iter().map(|k| k.strip_prefix("apikey:").unwrap_or(&k).to_string()).collect())
    }

    pub async fn increment_usage(&self, key: &str) -> anyhow::Result<i64> {
        let mut conn = self.conn.clone();
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        let usage_key = format!("usage:{}:{}", key, today);
        let count: i64 = conn.incr(&usage_key, 1).await?;
        let _: () = conn.expire(&usage_key, 7 * 24 * 3600).await?;
        Ok(count)
    }

    pub async fn get_usage(&self, key: &str, date: &str) -> anyhow::Result<i64> {
        let mut conn = self.conn.clone();
        let usage_key = format!("usage:{}:{}", key, date);
        let count: i64 = conn.get(&usage_key).await.unwrap_or(0);
        Ok(count)
    }

    pub async fn get_all_usage(&self, key: &str) -> anyhow::Result<std::collections::HashMap<String, i64>> {
        let mut usage = std::collections::HashMap::new();
        let today = chrono::Local::now().date_naive();
        for i in 0..7 {
            let date = (today - chrono::Duration::days(i)).format("%Y-%m-%d").to_string();
            let count = self.get_usage(key, &date).await.unwrap_or(0);
            if count > 0 {
                usage.insert(date, count);
            }
        }
        Ok(usage)
    }

    pub async fn check_rate_limit(&self, key: &str, max_requests: u64) -> anyhow::Result<bool> {
        let mut conn = self.conn.clone();
        let rl_key = format!("ratelimit:{}", key);
        let now = chrono::Utc::now().timestamp_millis() as f64;
        let window_start = now - (self.rate_limit_window_secs as f64 * 1000.0);
        let request_id = uuid::Uuid::new_v4().to_string();

        let _: () = redis::cmd("ZREMRANGEBYSCORE")
            .arg(&rl_key)
            .arg("-inf")
            .arg(window_start)
            .query_async(&mut conn)
            .await?;
        let _: () = redis::cmd("ZADD")
            .arg(&rl_key)
            .arg(now)
            .arg(&request_id)
            .query_async(&mut conn)
            .await?;
        let count: i64 = redis::cmd("ZCARD")
            .arg(&rl_key)
            .query_async(&mut conn)
            .await?;
        let _: () = redis::cmd("EXPIRE")
            .arg(&rl_key)
            .arg(self.rate_limit_window_secs as i64 + 10)
            .query_async(&mut conn)
            .await?;

        Ok(count <= max_requests as i64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn get_store() -> Option<RedisStore> {
        let url = std::env::var("REDIS_URL")
            .unwrap_or_else(|_| "redis://localhost:6379".into());
        RedisStore::new(&url, 60).await.ok()
    }

    #[tokio::test]
    async fn api_key_crud() {
        let store = match get_store().await {
            Some(s) => s,
            None => { eprintln!("skipping: Redis not available"); return; }
        };

        let key = format!("test-key-{}", uuid::Uuid::new_v4());
        let config = ApiKeyConfig {
            prefixes: vec!["data/".into(), "local/".into()],
            rate_limit: 50,
        };

        // Create
        store.create_api_key(&key, &config).await.unwrap();

        // Read
        let loaded = store.get_api_key(&key).await.unwrap().unwrap();
        assert_eq!(loaded.prefixes, vec!["data/", "local/"]);
        assert_eq!(loaded.rate_limit, 50);

        // List
        let keys = store.list_api_keys().await.unwrap();
        assert!(keys.iter().any(|k| k == &key));

        // Delete
        assert!(store.delete_api_key(&key).await.unwrap());
        assert!(store.get_api_key(&key).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn usage_counter_increments_per_day() {
        let store = match get_store().await {
            Some(s) => s,
            None => { eprintln!("skipping: Redis not available"); return; }
        };

        let key = format!("test-usage-{}", uuid::Uuid::new_v4());

        let count1 = store.increment_usage(&key).await.unwrap();
        let count2 = store.increment_usage(&key).await.unwrap();
        assert_eq!(count1, 1);
        assert_eq!(count2, 2);

        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        let loaded = store.get_usage(&key, &today).await.unwrap();
        assert_eq!(loaded, 2);

        // Cleanup
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        let usage_key = format!("usage:{}:{}", key, today);
        let mut conn = store.conn.clone();
        let _: () = redis::cmd("DEL").arg(&usage_key).query_async(&mut conn).await.unwrap();
    }

    #[tokio::test]
    async fn rate_limit_enforcement() {
        let store = match get_store().await {
            Some(s) => s,
            None => { eprintln!("skipping: Redis not available"); return; }
        };

        let key = format!("test-rl-{}", uuid::Uuid::new_v4());
        let max = 3u64;

        // First 3 should pass
        for _ in 0..3 {
            assert!(store.check_rate_limit(&key, max).await.unwrap());
        }
        // 4th should be rejected
        assert!(!store.check_rate_limit(&key, max).await.unwrap());

        // Cleanup
        let rl_key = format!("ratelimit:{}", key);
        let mut conn = store.conn.clone();
        let _: () = redis::cmd("DEL").arg(&rl_key).query_async(&mut conn).await.unwrap();
    }

    #[tokio::test]
    async fn get_nonexistent_key_returns_none() {
        let store = match get_store().await {
            Some(s) => s,
            None => { eprintln!("skipping: Redis not available"); return; }
        };

        let result = store.get_api_key("nonexistent-key-xyz").await.unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn api_key_config_serialization() {
        let config = ApiKeyConfig {
            prefixes: vec!["data/".into(), "local/".into()],
            rate_limit: 100,
        };
        let json = serde_json::to_string(&config).unwrap();
        let parsed: ApiKeyConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.prefixes, vec!["data/", "local/"]);
        assert_eq!(parsed.rate_limit, 100);
    }

    #[test]
    fn api_key_config_deserialization() {
        let json = r#"{"prefixes":["s3data/","localdata/"],"rate_limit":200}"#;
        let parsed: ApiKeyConfig = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.prefixes.len(), 2);
        assert_eq!(parsed.prefixes[0], "s3data/");
        assert_eq!(parsed.prefixes[1], "localdata/");
        assert_eq!(parsed.rate_limit, 200);
    }
}
