use std::{num::NonZero, sync::Arc};

use axum::{
    Router,
    routing::{get, post},
};
use clap::Parser;
use lru::LruCache;
use tokio::{
    net::{TcpListener, UnixListener},
    sync::RwLock,
};
use tracing_subscriber::{EnvFilter, FmtSubscriber};

use figure_8_bin::{
    AppState, Error,
    handlers::{health, live, session_create, session_delete, session_execute, session_get},
    session_reaper,
};

#[derive(Debug, Parser)]
struct Args {
    #[arg(short, long, env = "ADDRESS", default_value = "0.0.0.0")]
    address: String,

    #[arg(short, long, env = "PORT", default_value = "8080")]
    port: u16,

    #[arg(short, long, env = "SOCKET", conflicts_with_all = ["address", "port"])]
    socket: Option<String>,
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    let args = Args::parse();

    let subscriber = FmtSubscriber::builder()
        .with_env_filter(EnvFilter::from_default_env())
        .finish();

    tracing::subscriber::set_global_default(subscriber).unwrap();

    let sessions = Arc::new(RwLock::new(LruCache::new(NonZero::new(1000).unwrap())));
    let reaper = Arc::new(tokio::spawn(session_reaper(sessions.clone())));

    let app = Router::new()
        .route("/health", get(health))
        .route("/live", get(live))
        .route("/session", post(session_create))
        .route(
            "/session/{id}",
            get(session_get)
                .patch(session_execute)
                .delete(session_delete),
        )
        .with_state(AppState { sessions, reaper });

    if let Some(socket) = args.socket {
        let listener = UnixListener::bind(socket)?;
        Ok(axum::serve(listener, app).await?)
    } else {
        let listener = TcpListener::bind((args.address, args.port)).await?;
        Ok(axum::serve(listener, app).await?)
    }
}
