mod config;
mod handlers;
mod presign;
mod redis;

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

    let presigner = presign::Presigner::new(&config.aws_region, config.presign_expiry_secs).await;
    log::info!("S3 presigner ready (region: {})", config.aws_region);

    let state = web::Data::new(AppState {
        admin_token: config.admin_token.clone(),
        redis: redis_store,
        presigner,
    });

    let listen_addr = config.listen_addr.clone();
    HttpServer::new(move || {
        App::new()
            .app_data(state.clone())
            .route("/get/{path:.*}", web::get().to(handlers::get_file))
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
