//! playora-server — Axum + SQLite, mini-backend for Playora MVP.

mod catalog;
mod dashboard;
mod db;
mod routes;

use anyhow::Result;
use axum::{
    routing::{get, post, put},
    Router,
};
use clap::Parser;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::Mutex;
use tower_http::trace::TraceLayer;

#[derive(Parser, Debug)]
struct Args {
    #[arg(long, default_value = "./server.db")]
    db: String,
    #[arg(long, default_value = "0.0.0.0:8080")]
    bind: SocketAddr,
}

pub type State = Arc<Mutex<rusqlite::Connection>>;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            std::env::var("PLAYORA_LOG")
                .unwrap_or_else(|_| "playora_server=info,tower_http=info".into()),
        )
        .init();
    let args = Args::parse();
    let conn = db::open(&args.db)?;
    let state: State = Arc::new(Mutex::new(conn));

    let app = Router::new()
        .route("/health", get(routes::health))
        .route("/api/v1/devices/register", post(routes::register))
        .route("/api/v1/devices/heartbeat", post(routes::heartbeat))
        .route("/api/v1/devices/capabilities", post(routes::capabilities))
        .route("/api/v1/hardware/snapshot", post(routes::hardware_snapshot))
        .route(
            "/api/v1/hardware/test-result",
            post(routes::hardware_test_result),
        )
        .route("/api/v1/resources/sample", post(routes::resource_sample))
        .route("/api/v1/events/batch", post(routes::events_batch))
        .route("/api/v1/events", get(routes::events_list))
        .route("/api/v1/devices", get(routes::devices_list))
        .route("/api/v1/devices/:id", get(routes::device_detail))
        .route("/api/v1/devices/:id/manifest", get(routes::manifest))
        .route("/api/v1/devices/:id/features", put(routes::set_features))
        .route("/api/v1/games", get(routes::games_list))
        .route("/api/v1/rankings/playtime", get(routes::ranking_playtime))
        .route("/api/v1/rankings/systems", get(routes::ranking_systems))
        .route("/api/v1/catalog", get(routes::catalog_list))
        .route("/api/v1/catalog/:id", get(routes::catalog_detail))
        .route(
            "/api/v1/catalog/:id/download",
            get(routes::catalog_download),
        )
        .route("/api/v1/downloads/report", post(routes::downloads_report))
        .route("/api/v1/sources", get(routes::sources_list))
        .route("/api/v1/systems", get(routes::systems_list))
        .route("/api/v1/saves/upload", post(routes::saves_upload))
        .route(
            "/api/v1/analytics/overview",
            get(routes::analytics_overview),
        )
        .route("/api/v1/activities/recent", get(routes::activities_recent))
        .route("/api/v1/activities/:id", get(routes::activity_get))
        .route(
            "/api/v1/restore/progress",
            get(routes::restore_progress_latest),
        )
        .route("/dashboard", get(dashboard::page))
        .route("/dashboard/devices", get(dashboard::devices_list_page))
        .route("/dashboard/games", get(dashboard::games_list_page))
        .route("/dashboard/activity", get(dashboard::activity_page))
        .route(
            "/dashboard/activity/:id",
            get(dashboard::activity_detail_page),
        )
        .route("/dashboard/device/:id", get(dashboard::device_page))
        .with_state(state)
        .layer(TraceLayer::new_for_http());

    tracing::info!("listening on http://{}", args.bind);
    let listener = tokio::net::TcpListener::bind(args.bind).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
