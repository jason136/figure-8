use axum::{Router, routing::get};
use clap::Parser;
use tokio::net::{TcpListener, UnixListener};
use tracing_subscriber::{EnvFilter, FmtSubscriber};

use figure_8_bin::{
    Error,
    handlers::AppState,
    handlers::{health, stream},
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

    let app = Router::new()
        .route("/health", get(health))
        .route("/stream", get(stream))
        .with_state(AppState {});

    if let Some(socket) = args.socket {
        let listener = UnixListener::bind(socket)?;
        Ok(axum::serve(listener, app).await?)
    } else {
        let listener = TcpListener::bind((args.address, args.port)).await?;
        Ok(axum::serve(listener, app).await?)
    }
}
