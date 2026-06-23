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

fn register_mdns(port: u16) -> anyhow::Result<mdns_sd::ServiceDaemon> {
    use mdns_sd::{ServiceDaemon, ServiceInfo};
    let daemon = ServiceDaemon::new()?;
    let hostname = format!(
        "{}.local.",
        std::env::var("HOSTNAME")
            .unwrap_or_else(|_| "playora-server".into())
            .replace('.', "-")
    );
    let ips: Vec<std::net::IpAddr> = if_addrs::get_if_addrs()
        .map(|ifs| {
            ifs.into_iter()
                .filter(|i| !i.is_loopback())
                .map(|i| i.ip())
                .collect()
        })
        .unwrap_or_default();
    let ip_strs: Vec<String> = ips.iter().map(|i| i.to_string()).collect();
    let ip_refs: Vec<&str> = ip_strs.iter().map(|s| s.as_str()).collect();
    let info = ServiceInfo::new(
        "_playora._tcp.local.",
        "playora-server",
        &hostname,
        &ip_refs[..],
        port,
        &[("version", env!("CARGO_PKG_VERSION"))][..],
    )?;
    daemon.register(info)?;
    tracing::info!("mDNS registered _playora._tcp.local. on port {port}");
    Ok(daemon)
}

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

    // Register over mDNS so agents on the LAN can discover us without
    // hard-coding the IP. Holds the daemon alive for the process lifetime.
    let _mdns = match register_mdns(args.bind.port()) {
        Ok(d) => Some(d),
        Err(e) => {
            tracing::warn!("mDNS register failed: {e} — agents will need PLAYORA_SERVER_URL");
            None
        }
    };

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
        .route("/api/v1/devices/:id/issues", get(routes::device_issues))
        .route(
            "/api/v1/devices/:id/rom-audit",
            get(routes::device_rom_audit),
        )
        .route(
            "/api/v1/devices/:id/doctor-report",
            get(routes::device_doctor_report),
        )
        .route("/api/v1/activities/recent", get(routes::activities_recent))
        .route("/api/v1/activities/:id", get(routes::activity_get))
        .route(
            "/api/v1/devices/:id/delete-rom",
            post(routes::delete_rom_request),
        )
        .route(
            "/api/v1/devices/:id/delete-pending",
            get(routes::delete_pending),
        )
        .route("/api/v1/devices/:id/delete-ack", post(routes::delete_ack))
        .route(
            "/api/v1/devices/:id/cloud-auth-token",
            get(routes::cloud_auth_fetch).post(routes::cloud_auth_submit),
        )
        .route(
            "/dashboard/cloud-setup/:id",
            get(dashboard::cloud_setup_page).post(dashboard::cloud_setup_submit),
        )
        .route(
            "/api/v1/devices/:id/cloud-catalog",
            get(routes::cloud_catalog_list).post(routes::cloud_catalog_post),
        )
        .route(
            "/api/v1/devices/:id/cloud-download",
            post(routes::cloud_download_request),
        )
        .route(
            "/api/v1/devices/:id/cloud-download-pending",
            get(routes::cloud_download_pending),
        )
        .route(
            "/api/v1/devices/:id/cloud-download-ack",
            post(routes::cloud_download_ack),
        )
        .route(
            "/dashboard/cloud-roms/:id",
            get(dashboard::cloud_roms_page).post(dashboard::cloud_download_form),
        )
        .route(
            "/api/v1/devices/:id/update-request",
            post(routes::update_request),
        )
        .route(
            "/api/v1/devices/:id/update-pending",
            get(routes::update_pending),
        )
        .route("/api/v1/devices/:id/update-ack", post(routes::update_ack))
        .route(
            "/dashboard/device/:id/update",
            post(dashboard::update_request_form),
        )
        .route(
            "/dashboard/device/:id/delete-rom",
            post(dashboard::delete_rom_form),
        )
        .route(
            "/api/v1/restore/progress",
            get(routes::restore_progress_latest),
        )
        .route("/dashboard", get(dashboard::page))
        .route("/dashboard/devices", get(dashboard::devices_list_page))
        .route("/dashboard/games", get(dashboard::games_list_page))
        .route(
            "/dashboard/games/:system",
            get(dashboard::games_by_system_page),
        )
        .route(
            "/dashboard/device/:id/games",
            get(dashboard::device_games_page),
        )
        .route("/dashboard/activity", get(dashboard::activity_page))
        .route(
            "/dashboard/activity/:id",
            get(dashboard::activity_detail_page),
        )
        .route("/dashboard/device/:id", get(dashboard::device_page))
        .route(
            "/dashboard/device/:id/issues",
            get(dashboard::device_issues_page),
        )
        .route(
            "/dashboard/device/:id/rom-audit",
            get(dashboard::device_rom_audit_page),
        )
        .route(
            "/dashboard/device/:id/doctor",
            get(dashboard::device_doctor_page),
        )
        .route(
            "/dashboard/device/:id/sessions",
            get(dashboard::device_sessions_page),
        )
        .with_state(state)
        .layer(TraceLayer::new_for_http());

    tracing::info!("listening on http://{}", args.bind);
    let listener = tokio::net::TcpListener::bind(args.bind).await?;
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .await?;
    Ok(())
}
