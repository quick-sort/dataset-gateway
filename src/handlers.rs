use actix_web::{web, HttpRequest, HttpResponse};
use serde::Deserialize;
use std::collections::HashMap;

use crate::config::StorageRoute;
use crate::redis::{ApiKeyConfig, RedisStore};
use crate::storage;

pub struct AppState {
    pub admin_token: String,
    pub redis: RedisStore,
    pub s3_clients: HashMap<String, storage::S3Storage>,
    pub local: storage::LocalStorage,
    pub storage_routes: HashMap<String, StorageRoute>,
}

fn extract_bearer_token(req: &HttpRequest) -> Option<String> {
    req.headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| {
            if v.eq_ignore_ascii_case("bearer ") {
                Some(v[7..].trim().to_string())
            } else {
                v.strip_prefix("Bearer ").map(|s| s.trim().to_string())
            }
        })
        .filter(|s| !s.is_empty())
}

// GET /get/{path}
pub async fn get_file(
    req: HttpRequest,
    path: web::Path<String>,
    state: web::Data<AppState>,
) -> HttpResponse {
    let token = match extract_bearer_token(&req) {
        Some(t) => t,
        None => {
            return unauthorized();
        }
    };

    let key_config = match state.redis.get_api_key(&token).await {
        Ok(Some(c)) => c,
        Ok(None) => {
            return unauthorized();
        }
        Err(e) => {
            log::error!("Redis error: {}", e);
            return HttpResponse::InternalServerError().json("Internal error");
        }
    };

    let path_str = path.into_inner();

    // Check access: find the longest prefix the key is allowed to access
    let matched_prefix = key_config
        .prefixes
        .iter()
        .filter(|p| path_str.starts_with(p.as_str()))
        .max_by_key(|p| p.len())
        .cloned();

    let match_prefix = match matched_prefix {
        Some(p) => p,
        None => {
            return HttpResponse::Forbidden().json("Access denied: path not allowed");
        }
    };

    if let Err(e) = state.redis.increment_usage(&token).await {
        log::error!("Usage increment error: {}", e);
    }

    match state.redis.check_rate_limit(&token, key_config.rate_limit).await {
        Ok(true) => {}
        Ok(false) => {
            return HttpResponse::TooManyRequests().json("Rate limit exceeded");
        }
        Err(e) => {
            log::error!("Rate limit error: {}", e);
        }
    }

    // Resolve storage backend from gateway-level routes
    let route = match state
        .storage_routes
        .iter()
        .filter(|(prefix, _)| path_str.starts_with(prefix.as_str()))
        .max_by_key(|(prefix, _)| prefix.len())
    {
        Some((_, r)) => r,
        None => {
            return HttpResponse::InternalServerError()
                .json("No storage route configured for this path");
        }
    };

    let remaining = &path_str[match_prefix.len()..];
    let gz_key = format!("{}{}.gz", route.key_prefix, remaining);
    let plain_key = format!("{}{}", route.key_prefix, remaining);

    match route.storage_type.as_str() {
        "local" => {
            let result = if route.default_gzip {
                match state.local.read(&route.target, &gz_key).await {
                    Ok(data) => Ok(data),
                    Err(_) => state.local.read(&route.target, &plain_key).await,
                }
            } else {
                state.local.read(&route.target, &plain_key).await
            };
            match result {
                Ok(data) => {
                    let ct = storage::guess_content_type(&path_str);
                    if gz_key.ends_with(".gz") || plain_key.ends_with(".gz") {
                        HttpResponse::Ok()
                            .insert_header(("Content-Type", ct))
                            .insert_header(("Content-Encoding", "gzip"))
                            .body(data)
                    } else {
                        HttpResponse::Ok()
                            .insert_header(("Content-Type", ct))
                            .body(data)
                    }
                }
                Err(e) => {
                    log::error!("Local read error: {}", e);
                    if e.contains("not found") || e.contains("No such file") {
                        HttpResponse::NotFound().json("File not found")
                    } else {
                        HttpResponse::InternalServerError().json("Failed to read file")
                    }
                }
            }
        }
        _ => {
            let region = route.region.as_deref().unwrap_or("us-east-1");
            let s3 = match state.s3_clients.get(region) {
                Some(c) => c,
                None => {
                    log::error!("No S3 client for region: {}", region);
                    return HttpResponse::InternalServerError()
                        .json("Storage backend not configured");
                }
            };
            let key = if route.default_gzip { &gz_key } else { &plain_key };
            match s3.presign(&route.target, key).await {
                Ok(url) => HttpResponse::Found()
                    .insert_header(("Location", url))
                    .finish(),
                Err(e) => {
                    log::error!("Presign error: {}", e);
                    HttpResponse::InternalServerError().json("Failed to generate presigned URL")
                }
            }
        }
    }
}

// GET /usage
pub async fn get_usage(
    req: HttpRequest,
    state: web::Data<AppState>,
) -> HttpResponse {
    let token = match extract_bearer_token(&req) {
        Some(t) => t,
        None => return unauthorized(),
    };

    let key_config = match state.redis.get_api_key(&token).await {
        Ok(Some(c)) => c,
        Ok(None) => return unauthorized(),
        Err(e) => {
            log::error!("Redis error: {}", e);
            return HttpResponse::InternalServerError().json("Internal error");
        }
    };

    let usage = state.redis.get_all_usage(&token).await.unwrap_or_default();

    HttpResponse::Ok().json(serde_json::json!({
        "key": token,
        "prefixes": key_config.prefixes,
        "rate_limit": key_config.rate_limit,
        "usage": usage,
    }))
}

// GET /admin/keys
pub async fn list_keys(
    req: HttpRequest,
    state: web::Data<AppState>,
) -> HttpResponse {
    if !is_admin(&req, &state.admin_token) {
        return unauthorized();
    }

    match state.redis.list_api_keys().await {
        Ok(keys) => HttpResponse::Ok().json(keys),
        Err(e) => {
            log::error!("Redis error: {}", e);
            HttpResponse::InternalServerError().json("Internal error")
        }
    }
}

#[derive(Deserialize)]
pub struct CreateKeyRequest {
    pub key: String,
    pub prefixes: Vec<String>,
    #[serde(default = "default_rate_limit")]
    pub rate_limit: u64,
}

fn default_rate_limit() -> u64 {
    100
}

// POST /admin/keys
pub async fn create_key(
    req: HttpRequest,
    body: web::Json<CreateKeyRequest>,
    state: web::Data<AppState>,
) -> HttpResponse {
    if !is_admin(&req, &state.admin_token) {
        return unauthorized();
    }

    let config = ApiKeyConfig {
        prefixes: body.prefixes.clone(),
        rate_limit: body.rate_limit,
    };

    match state.redis.create_api_key(&body.key, &config).await {
        Ok(()) => HttpResponse::Created().json(serde_json::json!({
            "key": body.key,
            "prefixes": body.prefixes,
            "rate_limit": body.rate_limit,
        })),
        Err(e) => {
            log::error!("Redis error: {}", e);
            HttpResponse::InternalServerError().json("Internal error")
        }
    }
}

// GET /admin/keys/{key}
pub async fn get_key(
    req: HttpRequest,
    key: web::Path<String>,
    state: web::Data<AppState>,
) -> HttpResponse {
    if !is_admin(&req, &state.admin_token) {
        return unauthorized();
    }

    let key_str = key.into_inner();
    match state.redis.get_api_key(&key_str).await {
        Ok(Some(config)) => {
            let today = chrono::Local::now().format("%Y-%m-%d").to_string();
            let usage = state.redis.get_usage(&key_str, &today).await.unwrap_or(0);
            HttpResponse::Ok().json(serde_json::json!({
                "key": key_str,
                "prefixes": config.prefixes,
                "rate_limit": config.rate_limit,
                "usage_today": usage,
            }))
        }
        Ok(None) => HttpResponse::NotFound().json("Key not found"),
        Err(e) => {
            log::error!("Redis error: {}", e);
            HttpResponse::InternalServerError().json("Internal error")
        }
    }
}

#[derive(Deserialize)]
pub struct UpdateKeyRequest {
    pub prefixes: Option<Vec<String>>,
    pub rate_limit: Option<u64>,
}

// PUT /admin/keys/{key}
pub async fn update_key(
    req: HttpRequest,
    key: web::Path<String>,
    body: web::Json<UpdateKeyRequest>,
    state: web::Data<AppState>,
) -> HttpResponse {
    if !is_admin(&req, &state.admin_token) {
        return unauthorized();
    }

    let key_str = key.into_inner();
    let existing = match state.redis.get_api_key(&key_str).await {
        Ok(Some(c)) => c,
        Ok(None) => return HttpResponse::NotFound().json("Key not found"),
        Err(e) => {
            log::error!("Redis error: {}", e);
            return HttpResponse::InternalServerError().json("Internal error");
        }
    };

    let body = body.into_inner();
    let updated = ApiKeyConfig {
        prefixes: body.prefixes.unwrap_or(existing.prefixes),
        rate_limit: body.rate_limit.unwrap_or(existing.rate_limit),
    };

    match state.redis.create_api_key(&key_str, &updated).await {
        Ok(()) => HttpResponse::Ok().json(serde_json::json!({
            "key": key_str,
            "prefixes": updated.prefixes,
            "rate_limit": updated.rate_limit,
        })),
        Err(e) => {
            log::error!("Redis error: {}", e);
            HttpResponse::InternalServerError().json("Internal error")
        }
    }
}

// DELETE /admin/keys/{key}
pub async fn delete_key(
    req: HttpRequest,
    key: web::Path<String>,
    state: web::Data<AppState>,
) -> HttpResponse {
    if !is_admin(&req, &state.admin_token) {
        return unauthorized();
    }

    let key_str = key.into_inner();
    match state.redis.delete_api_key(&key_str).await {
        Ok(true) => HttpResponse::Ok().json(serde_json::json!({
            "deleted": key_str,
        })),
        Ok(false) => HttpResponse::NotFound().json("Key not found"),
        Err(e) => {
            log::error!("Redis error: {}", e);
            HttpResponse::InternalServerError().json("Internal error")
        }
    }
}

fn is_admin(req: &HttpRequest, admin_token: &str) -> bool {
    extract_bearer_token(req).map_or(false, |t| t == admin_token)
}

fn unauthorized() -> HttpResponse {
    HttpResponse::Unauthorized()
        .insert_header(("WWW-Authenticate", "Bearer"))
        .json("Invalid or missing Bearer token")
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::test::TestRequest;

    #[test]
    fn extract_bearer_standard() {
        let req = TestRequest::default()
            .insert_header(("authorization", "Bearer my-token"))
            .to_http_request();
        assert_eq!(extract_bearer_token(&req), Some("my-token".into()));
    }

    #[test]
    fn extract_bearer_with_spaces() {
        let req = TestRequest::default()
            .insert_header(("authorization", "Bearer   my-token  "))
            .to_http_request();
        assert_eq!(extract_bearer_token(&req), Some("my-token".into()));
    }

    #[test]
    fn extract_bearer_missing_header() {
        let req = TestRequest::default().to_http_request();
        assert_eq!(extract_bearer_token(&req), None);
    }

    #[test]
    fn extract_bearer_empty() {
        let req = TestRequest::default()
            .insert_header(("authorization", ""))
            .to_http_request();
        assert_eq!(extract_bearer_token(&req), None);
    }

    #[test]
    fn extract_bearer_wrong_scheme() {
        let req = TestRequest::default()
            .insert_header(("authorization", "Basic dXNlcjpwYXNz"))
            .to_http_request();
        assert_eq!(extract_bearer_token(&req), None);
    }

    #[test]
    fn is_admin_correct_token() {
        let req = TestRequest::default()
            .insert_header(("authorization", "Bearer admin-secret"))
            .to_http_request();
        assert!(is_admin(&req, "admin-secret"));
    }

    #[test]
    fn is_admin_wrong_token() {
        let req = TestRequest::default()
            .insert_header(("authorization", "Bearer wrong"))
            .to_http_request();
        assert!(!is_admin(&req, "admin-secret"));
    }

    #[test]
    fn is_admin_no_token() {
        let req = TestRequest::default().to_http_request();
        assert!(!is_admin(&req, "admin-secret"));
    }

    #[test]
    fn create_key_request_with_prefixes() {
        let json = r#"{
            "key": "k",
            "prefixes": ["data/", "local/"],
            "rate_limit": 50
        }"#;
        let req: CreateKeyRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.prefixes.len(), 2);
        assert_eq!(req.prefixes[0], "data/");
        assert_eq!(req.prefixes[1], "local/");
        assert_eq!(req.rate_limit, 50);
    }

    #[test]
    fn create_key_request_defaults() {
        let json = r#"{
            "key": "k",
            "prefixes": ["data/"]
        }"#;
        let req: CreateKeyRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.rate_limit, 100);
    }

    #[test]
    fn unauthorized_response() {
        let resp = unauthorized();
        assert_eq!(resp.status(), 401);
    }
}
