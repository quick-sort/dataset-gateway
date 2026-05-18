mod config;
mod handlers;
mod redis;
mod storage;

use std::collections::HashMap;

use actix_web::{web, App, HttpServer};
use handlers::AppState;

#[actix_web::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();

    let config = config::AppConfig::load()?;
    log::info!("Starting dataset-gateway on {}", config.listen_addr);

    let redis_store = redis::RedisStore::new(
        &config.redis_url,
        config.rate_limit_window_secs,
    )
    .await?;
    log::info!("Connected to Redis");

    let mut s3_clients: HashMap<String, storage::S3Storage> = HashMap::new();
    for (_, route) in &config.storage_routes {
        if route.storage_type == "s3" {
            let region = route.region.as_deref().unwrap_or("us-east-1");
            if !s3_clients.contains_key(region) {
                let client = storage::S3Storage::new(region, config.presign_expiry_secs).await;
                log::info!("S3 client ready (region: {})", region);
                s3_clients.insert(region.to_string(), client);
            }
        }
    }

    let state = web::Data::new(AppState {
        admin_token: config.admin_token.clone(),
        redis: redis_store,
        s3_clients,
        local: storage::LocalStorage,
        storage_routes: config.storage_routes,
    });

    let listen_addr = config.listen_addr.clone();
    HttpServer::new(move || {
        App::new()
            .app_data(state.clone())
            .route("/get/{path:.*}", web::get().to(handlers::get_file))
            .route("/usage", web::get().to(handlers::get_usage))
            .route("/admin/keys", web::get().to(handlers::list_keys))
            .route("/admin/keys", web::post().to(handlers::create_key))
            .route("/admin/keys/{key}", web::get().to(handlers::get_key))
            .route("/admin/keys/{key}", web::put().to(handlers::update_key))
            .route("/admin/keys/{key}", web::delete().to(handlers::delete_key))
    })
    .bind(&listen_addr)?
    .run()
    .await?;

    Ok(())
}
