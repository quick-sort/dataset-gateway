use lambda_runtime::{service_fn, Error, LambdaEvent};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug, Serialize, Deserialize)]
struct ApiGatewayResponse {
    #[serde(rename = "statusCode")]
    status_code: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    body: Option<String>,
    #[serde(rename = "isBase64Encoded")]
    is_base64_encoded: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    headers: Option<HashMap<String, String>>,
}

#[derive(Debug, Deserialize)]
struct ApiGatewayEvent {
    #[serde(rename = "pathParameters")]
    path_parameters: Option<Value>,
    headers: HashMap<String, String>,
}

#[derive(Debug, Deserialize, Clone)]
struct BucketConfig {
    bucket: String,
    #[serde(default)]
    allowed_prefixes: Vec<String>,
}

type ApiKeyPermissions = HashMap<String, BucketConfig>;

async fn lambda_handler(event: LambdaEvent<ApiGatewayEvent>) -> Result<ApiGatewayResponse, Error> {
    let event = event.payload;
    let api_key_permissions_json = std::env::var("API_KEY_PERMISSIONS")
        .unwrap_or_else(|_| r#"{"api_key_abc123":{"bucket":"example-bucket","allowed_prefixes":["userA/","public/"]}}"#.to_string());

    let api_key_permissions: ApiKeyPermissions = serde_json::from_str(&api_key_permissions_json)
        .unwrap_or_else(|_| {
            let mut m = HashMap::new();
            m.insert("api_key_abc123".to_string(), BucketConfig {
                bucket: "example-bucket".to_string(),
                allowed_prefixes: vec!["userA/".to_string(), "public/".to_string()],
            });
            m
        });

    let api_key = event.headers.get("x-api-key").cloned().unwrap_or_default();

    if !api_key_permissions.contains_key(&api_key) {
        return Ok(ApiGatewayResponse {
            status_code: 403,
            body: Some("Invalid API Key".to_string()),
            is_base64_encoded: false,
            headers: None,
        });
    }

    let config = &api_key_permissions[&api_key];
    let path = event.path_parameters
        .as_ref()
        .and_then(|p| p.get("path"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if !config.allowed_prefixes.iter().any(|prefix| path.starts_with(prefix)) {
        return Ok(ApiGatewayResponse {
            status_code: 403,
            body: Some("Access Denied: Path not allowed".to_string()),
            is_base64_encoded: false,
            headers: None,
        });
    }

    let s3_key = format!("{}.gz", path);
    let presigned_url = generate_presigned_url(&config.bucket, &s3_key).await?;

    let mut headers = HashMap::new();
    headers.insert("Location".to_string(), presigned_url);

    Ok(ApiGatewayResponse {
        status_code: 302,
        body: None,
        is_base64_encoded: false,
        headers: Some(headers),
    })
}

async fn generate_presigned_url(bucket: &str, key: &str) -> Result<String, Error> {
    use aws_config::BehaviorVersion;
    use aws_sdk_s3::presigning::PresigningConfig;
    use std::time::Duration;

    let config = aws_config::defaults(BehaviorVersion::latest())
        .load()
        .await;
    let client = aws_sdk_s3::Client::new(&config);

    let presigning_config = PresigningConfig::builder()
        .expires_in(Duration::from_secs(900))
        .build()
        .map_err(|e| Error::from(format!("Presigning config error: {}", e)))?;

    let presigned = client
        .get_object()
        .bucket(bucket)
        .key(key)
        .presigned(presigning_config)
        .await
        .map_err(|e| Error::from(format!("Presign error: {}", e)))?;

    Ok(presigned.uri().to_string())
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    lambda_runtime::run(service_fn(lambda_handler)).await?;
    Ok(())
}
