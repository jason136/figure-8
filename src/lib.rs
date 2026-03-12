use std::{
    path::PathBuf,
    sync::{Arc, OnceLock},
};

use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use figure_8::{
    JsApi, Sandbox,
    builtins::{browser::Browser, fetch::Fetch, fs::Fs, mcp::Mcp},
    sandbox::interface::Interface,
};
use futures::future::try_join_all;
use lru::LruCache;
use serde_json::json;
use tokio::{sync::RwLock, task::JoinHandle};

use crate::schemas::Capabilities;

pub mod handlers;
pub mod schemas;

#[derive(Clone)]
pub struct AppState {
    pub sessions: Arc<RwLock<LruCache<String, Arc<InstanceState>>>>,
    pub reaper: Arc<JoinHandle<()>>,
}

#[derive(Default)]
pub struct CapabilityHandles {
    _fs: Option<Fs>,
    _fetch: Option<Fetch>,
    _browser: Option<Browser>,
    _mcp: Vec<Mcp>,
}

pub struct InstanceState {
    pub sandbox: Sandbox,
    pub dts: String,
    _handles: CapabilityHandles,
}

impl InstanceState {
    pub async fn new(capabilities: &Capabilities) -> Result<Self, Error> {
        let mut js_api = JsApi::default();

        let _handles = CapabilityHandles {
            _fs: capabilities
                .fs
                .as_ref()
                .map(|_capability| Fs::from_local_path(PathBuf::from("/tmp/f8-fs"))),
            _fetch: {
                static REQWEST_CLIENT: OnceLock<reqwest::Client> = OnceLock::new();

                let client = REQWEST_CLIENT.get_or_init(reqwest::Client::new);

                let fetch = Fetch::new(client.clone());
                fetch.extend_api(&mut js_api)?;

                Some(fetch)
            },
            _browser: capabilities
                .browser
                .as_ref()
                .map(|_capability| {
                    let browser = Browser::new(Browser::default_config()?);
                    browser.extend_api(&mut js_api)?;

                    Ok::<_, Error>(browser)
                })
                .transpose()?,
            _mcp: {
                let mcps = try_join_all(
                    capabilities
                        .mcp
                        .iter()
                        .map(|capability| async move { Mcp::new(capability.server.as_str()).await })
                        .collect::<Vec<_>>(),
                )
                .await?;

                for mcp in &mcps {
                    mcp.extend_api(&mut js_api)?;
                }

                mcps
            },
        };

        Ok(InstanceState {
            dts: js_api.generate_dts(),
            sandbox: Sandbox::new(js_api)?,
            _handles,
        })
    }
}

pub async fn session_reaper(sessions: Arc<RwLock<LruCache<String, Arc<InstanceState>>>>) {
    loop {
        {
            let _sessions = sessions.read().await;
        }

        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("axum error: {0}")]
    Axum(#[from] axum::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("tungstenite error: {0}")]
    Tungstenite(#[from] tokio_tungstenite::tungstenite::Error),

    #[error("session not found")]
    SessionNotFound,

    #[error("sandbox error: {0}")]
    Sandbox(#[from] figure_8::SandboxError),

    #[error("tool error: {0}")]
    Tool(#[from] figure_8::FnDefError),
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "error": self.to_string(),
            })),
        )
            .into_response()
    }
}
