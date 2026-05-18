use aws_config::BehaviorVersion;
use aws_sdk_s3::presigning::PresigningConfig;
use std::path::Path;
use std::time::Duration;
use tokio::fs;

pub struct S3Storage {
    client: aws_sdk_s3::Client,
    expiry_secs: u64,
}

impl S3Storage {
    pub async fn new(aws_region: &str, expiry_secs: u64) -> Self {
        let config = aws_config::defaults(BehaviorVersion::latest())
            .region(aws_config::Region::new(aws_region.to_string()))
            .load()
            .await;
        let client = aws_sdk_s3::Client::new(&config);
        Self {
            client,
            expiry_secs,
        }
    }

    pub async fn presign(&self, bucket: &str, key: &str) -> Result<String, String> {
        let presigning_config = PresigningConfig::builder()
            .expires_in(Duration::from_secs(self.expiry_secs))
            .build()
            .map_err(|e| format!("Presigning config error: {}", e))?;

        let presigned = self
            .client
            .get_object()
            .bucket(bucket)
            .key(key)
            .presigned(presigning_config)
            .await
            .map_err(|e| format!("Presign error: {}", e))?;

        Ok(presigned.uri().to_string())
    }
}

pub struct LocalStorage;

impl LocalStorage {
    pub async fn read(&self, base_dir: &str, key: &str) -> Result<Vec<u8>, String> {
        let file_path = Path::new(base_dir).join(key);

        let canonical_base = Path::new(base_dir)
            .canonicalize()
            .map_err(|e| format!("Invalid base dir: {}", e))?;

        let canonical_file = file_path
            .canonicalize()
            .map_err(|e| format!("File not found: {}", e))?;

        if !canonical_file.starts_with(&canonical_base) {
            return Err("Path traversal denied".into());
        }

        fs::read(&canonical_file)
            .await
            .map_err(|e| format!("Read error: {}", e))
    }
}

pub fn guess_content_type(path: &str) -> &'static str {
    let path = path.to_lowercase();
    if path.ends_with(".csv") {
        "text/csv"
    } else if path.ends_with(".json") || path.ends_with(".jsonl") {
        "application/json"
    } else if path.ends_with(".parquet") {
        "application/octet-stream"
    } else if path.ends_with(".txt") {
        "text/plain"
    } else if path.ends_with(".html") || path.ends_with(".htm") {
        "text/html"
    } else if path.ends_with(".xml") {
        "application/xml"
    } else {
        "application/octet-stream"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn local_storage_reads_file() {
        let dir = std::env::temp_dir().join(format!("dg-local-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join("data")).unwrap();
        std::fs::write(dir.join("data/test.csv.gz"), b"hello").unwrap();

        let storage = LocalStorage;
        let data = storage.read(
            std::fs::canonicalize(&dir).unwrap().to_str().unwrap(),
            "data/test.csv.gz",
        ).await.unwrap();
        assert_eq!(data, b"hello");

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn local_storage_not_found() {
        let storage = LocalStorage;
        let result = storage.read("/nonexistent", "missing.gz").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn local_storage_blocks_path_traversal() {
        let dir = std::env::temp_dir().join(format!("dg-local-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("safe.gz"), b"safe").unwrap();

        let storage = LocalStorage;
        let result = storage.read(
            std::fs::canonicalize(&dir).unwrap().to_str().unwrap(),
            "../etc/passwd",
        ).await;
        assert!(result.is_err());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn guess_content_type_csv() {
        assert_eq!(guess_content_type("data/file.csv"), "text/csv");
    }

    #[test]
    fn guess_content_type_json() {
        assert_eq!(guess_content_type("data/file.json"), "application/json");
    }

    #[test]
    fn guess_content_type_unknown() {
        assert_eq!(guess_content_type("data/file.xyz"), "application/octet-stream");
    }
}
