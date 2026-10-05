mod cache;
mod captcha;
mod config;
mod content;
mod db;
mod error;
mod geoip;
mod http;
mod postparts;
mod realtime;
mod secrets;
mod send;
mod state;
mod thumbnails;
mod tripcode;
mod types;
mod views;

use std::net::SocketAddr;
use std::sync::Arc;

use clap::Parser;

use crate::state::AppState;

#[derive(Parser)]
#[command(name = "ech-db-relay")]
struct Cli {
    #[arg(short, long)]
    config: String,
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let cli = Cli::parse();
    let config = config::load(&cli.config).map_err(std::io::Error::other)?;
    let bind = config.server.bind.clone();
    let admin_bind = config.server.admin_bind.clone();
    let state = Arc::new(AppState::build(config).await.map_err(std::io::Error::other)?);
    let (public, admin) = http::router(state);
    let public_listener = tokio::net::TcpListener::bind(&bind).await?;
    let admin_listener = tokio::net::TcpListener::bind(&admin_bind).await?;
    eprintln!("relay: public {bind}, admin {admin_bind}");
    let public_server = axum::serve(
        public_listener,
        public.into_make_service_with_connect_info::<SocketAddr>(),
    );
    let admin_server = axum::serve(admin_listener, admin.into_make_service());
    tokio::try_join!(public_server, admin_server)?;
    Ok(())
}
