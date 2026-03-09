use figure_8::{
    Interface, Sandbox,
    builtins::{browser::Browser, mcp::Mcp},
};
use futures::future::try_join_all;

use crate::schemas::Capabilities;

pub mod handlers;
pub mod schemas;

#[derive(Default)]
pub struct CapabilityHandles {
    _browser: Option<Browser>,
    _mcp: Vec<Mcp>,
}

pub struct InstanceState {
    pub sandbox: Sandbox,
    _handles: CapabilityHandles,
}

impl InstanceState {
    pub async fn new(capabilities: &Capabilities) -> Result<Self, Error> {
        let mut interface = Interface::default();

        let _handles = CapabilityHandles {
            _browser: capabilities
                .browser
                .as_ref()
                .map(|_capability| {
                    let browser = Browser::new(Browser::default_config()?);
                    interface.extend(browser.default_tools())?;
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
                    interface.extend(mcp.default_tools()?)?;
                }

                mcps
            },
        };

        Ok(InstanceState {
            sandbox: Sandbox::new(interface)?,
            _handles,
        })
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

    #[error("sandbox error: {0}")]
    Sandbox(#[from] figure_8::SandboxError),

    #[error("tool error: {0}")]
    Tool(#[from] figure_8::ToolError),
}
