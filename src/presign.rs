use aws_config::BehaviorVersion;
use aws_sdk_s3::presigning::PresigningConfig;
use std::time::Duration;

pub struct Presigner {
    client: aws_sdk_s3::Client,
    expiry_secs: u64,
}

impl Presigner {
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
