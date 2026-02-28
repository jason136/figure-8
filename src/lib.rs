use figure_8::{
    Interface, Sandbox,
    browser::{Browser, register_tools},
};

use crate::schemas::Capabilities;

pub mod handlers;
pub mod schemas;

#[derive(Default)]
pub struct CapabilityHandles {
    browser: Option<Browser>,
}

pub struct InstanceState {
    pub sandbox: Sandbox,
    handles: CapabilityHandles,
}

impl InstanceState {
    pub fn new(capabilities: &[Capabilities]) -> Result<Self, Error> {
        let (capability_handles, interface_builder) = capabilities.iter().try_fold(
            (
                CapabilityHandles::default(),
                Interface::builder("execution"),
            ),
            |(mut handles, builder), capability| {
                let builder = match capability {
                    Capabilities::Browser => {
                        if handles.browser.is_none() {
                            handles.browser = Some(Browser::new(Browser::default_config()?));
                        }
                        register_tools(builder, handles.browser.as_ref().unwrap())
                    }
                };

                Ok::<_, Error>((handles, builder))
            },
        )?;

        Ok(InstanceState {
            sandbox: Sandbox::new(interface_builder.build()),
            handles: capability_handles,
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
