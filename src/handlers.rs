use actix_web::{web, HttpRequest, HttpResponse};
use serde::Deserialize;

use crate::presign::Presigner;
use crate::redis::{ApiKeyConfig, RedisStore};

pub struct AppState {
    pub admin_token: String,
    pub redis: RedisStore,
    pub presigner: Presigner,
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

    if !key_config
        .prefixes
        .iter()
        .any(|prefix| path_str.starts_with(prefix))
    {
        return HttpResponse::Forbidden().json("Access denied: path not allowed");
    }

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

    let s3_key = format!("{}.gz", path_str);
    match state.presigner.presign(&key_config.bucket, &s3_key).await {
        Ok(url) => HttpResponse::Found()
            .insert_header(("Location", url))
            .finish(),
        Err(e) => {
            log::error!("Presign error: {}", e);
            HttpResponse::InternalServerError().json("Failed to generate presigned URL")
        }
    }
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
    pub bucket: String,
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
        bucket: body.bucket.clone(),
        prefixes: body.prefixes.clone(),
        rate_limit: body.rate_limit,
    };

    match state.redis.create_api_key(&body.key, &config).await {
        Ok(()) => HttpResponse::Created().json(serde_json::json!({
            "key": body.key,
            "bucket": body.bucket,
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
                "bucket": config.bucket,
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
    pub bucket: Option<String>,
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
        bucket: body.bucket.unwrap_or_else(|| existing.bucket),
        prefixes: body.prefixes.unwrap_or_else(|| existing.prefixes),
        rate_limit: body.rate_limit.unwrap_or(existing.rate_limit),
    };

    match state.redis.create_api_key(&key_str, &updated).await {
        Ok(()) => HttpResponse::Ok().json(serde_json::json!({
            "key": key_str,
            "bucket": updated.bucket,
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
    fn create_key_request_defaults() {
        let json = r#"{"key":"k","bucket":"b","prefixes":["p/"]}"#;
        let req: CreateKeyRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.rate_limit, 100); // default
    }

    #[test]
    fn create_key_request_custom_rate_limit() {
        let json = r#"{"key":"k","bucket":"b","prefixes":["p/"],"rate_limit":50}"#;
        let req: CreateKeyRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.rate_limit, 50);
    }

    #[test]
    fn unauthorized_response() {
        let resp = unauthorized();
        assert_eq!(resp.status(), 401);
    }
}
