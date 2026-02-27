use std::sync::Arc;

use chromiumoxide::Page;
use futures::StreamExt;
use tokio::sync::RwLock;

use crate::sandbox::tool::ToolDef;
use crate::{ToolError, tool_async};

pub struct Browser {
    inner: RwLock<BrowserInner>,
}

enum BrowserInner {
    Pending(Box<chromiumoxide::BrowserConfig>),
    Initialized {
        _browser: Box<chromiumoxide::Browser>,
        page: Arc<Page>,
    },
}

impl Browser {
    pub fn new(config: chromiumoxide::BrowserConfig) -> Arc<Self> {
        Arc::new(Browser {
            inner: RwLock::new(BrowserInner::Pending(Box::new(config))),
        })
    }

    pub fn default_config() -> chromiumoxide::BrowserConfig {
        chromiumoxide::BrowserConfig::builder()
            .arg("--disable-gpu")
            .arg("--disable-dev-shm-usage")
            .build()
            .unwrap()
    }

    async fn ensure_browser(&self) -> Result<Arc<Page>, ToolError> {
        match &*self.inner.read().await {
            BrowserInner::Pending(_) => Err(ToolError::custom(
                "browser not launched -- call page.launch() first",
            )),
            BrowserInner::Initialized { page, .. } => Ok(page.clone()),
        }
    }

    async fn launch_browser(&self) -> Result<(), ToolError> {
        let config = match &*self.inner.read().await {
            BrowserInner::Pending(config) => config.clone(),
            BrowserInner::Initialized { .. } => return Ok(()),
        };

        let (browser, mut handler) = chromiumoxide::Browser::launch(*config).await?;

        tokio::spawn(async move { while handler.next().await.is_some() {} });

        let page = browser.new_page("about:blank").await?;
        page.wait_for_navigation().await?;

        *self.inner.write().await = BrowserInner::Initialized {
            _browser: Box::new(browser),
            page: Arc::new(page),
        };

        Ok(())
    }
}

pub fn launch(handle: &Arc<Browser>) -> ToolDef {
    let handle = Arc::clone(handle);
    tool_async!("launch", "Launch a headless Chrome browser", || -> () {
        let handle = handle.clone();
        async move {
            handle.launch_browser().await?;
            Ok::<_, ToolError>(())
        }
    })
    .with_namespace("page")
}

pub fn goto(handle: &Arc<Browser>) -> ToolDef {
    let handle = Arc::clone(handle);
    tool_async!("goto", "Navigate to a URL", |url: String| -> () {
        let handle = handle.clone();
        async move {
            let page = handle.ensure_browser().await?;
            page.goto(&url).await?;
            page.wait_for_navigation().await?;
            Ok::<_, ToolError>(())
        }
    })
    .with_namespace("page")
}

pub fn click(handle: &Arc<Browser>) -> ToolDef {
    let handle = Arc::clone(handle);
    tool_async!(
        "click",
        "Click an element by CSS selector",
        |selector: String| -> () {
            let handle = handle.clone();
            async move {
                let page = handle.ensure_browser().await?;
                page.find_element(&selector).await?.click().await?;
                Ok::<_, ToolError>(())
            }
        }
    )
    .with_namespace("page")
}

pub fn type_into(handle: &Arc<Browser>) -> ToolDef {
    let handle = Arc::clone(handle);
    tool_async!("type", "Type text into an element", |selector: String,
                                                      text: String|
     -> () {
        let handle = handle.clone();
        async move {
            let page = handle.ensure_browser().await?;
            page.find_element(&selector)
                .await?
                .click()
                .await?
                .type_str(&text)
                .await?;
            Ok::<_, ToolError>(())
        }
    })
    .with_namespace("page")
}

pub fn get_text(handle: &Arc<Browser>) -> ToolDef {
    let handle = Arc::clone(handle);
    tool_async!(
        "getText",
        "Get inner text of an element",
        |selector: String| -> String {
            let handle = handle.clone();
            async move {
                let page = handle.ensure_browser().await?;
                page.find_element(&selector)
                    .await?
                    .inner_text()
                    .await?
                    .ok_or_else(|| ToolError::custom("element has no text"))
            }
        }
    )
    .with_namespace("page")
}

pub fn get_html(handle: &Arc<Browser>) -> ToolDef {
    let handle = Arc::clone(handle);
    tool_async!("getHtml", "Get the full HTML of the page", || -> String {
        let handle = handle.clone();
        async move { Ok::<_, ToolError>(handle.ensure_browser().await?.content().await?) }
    })
    .with_namespace("page")
}

pub fn screenshot(handle: &Arc<Browser>) -> ToolDef {
    let handle = Arc::clone(handle);
    tool_async!("screenshot", "Capture a PNG screenshot as base64", || -> String {
        let handle = handle.clone();
        async move {
            use base64::Engine;
            let page = handle.ensure_browser().await?;
            let png = page.screenshot(
                chromiumoxide::page::ScreenshotParams::builder()
                    .format(chromiumoxide::cdp::browser_protocol::page::CaptureScreenshotFormat::Png)
                    .build(),
            ).await?;
            Ok::<_, ToolError>(base64::engine::general_purpose::STANDARD.encode(&png))
        }
    })
    .with_namespace("page")
}

pub fn eval(handle: &Arc<Browser>) -> ToolDef {
    let handle = Arc::clone(handle);
    tool_async!(
        "eval",
        "Evaluate JavaScript in the browser page",
        |code: String| -> String {
            let handle = handle.clone();
            async move {
                let page = handle.ensure_browser().await?;
                let val: serde_json::Value = page.evaluate_expression(&code).await?.into_value()?;
                Ok::<_, ToolError>(val.to_string())
            }
        }
    )
    .with_namespace("page")
}

pub fn wait_for(handle: &Arc<Browser>) -> ToolDef {
    let handle = Arc::clone(handle);
    tool_async!(
        "waitFor",
        "Wait for an element matching a CSS selector",
        |selector: String| -> () {
            let handle = handle.clone();
            async move {
                handle
                    .ensure_browser()
                    .await?
                    .find_element(&selector)
                    .await?;
                Ok::<_, ToolError>(())
            }
        }
    )
    .with_namespace("page")
}

pub fn get_title(handle: &Arc<Browser>) -> ToolDef {
    let handle = Arc::clone(handle);
    tool_async!(
        "getTitle",
        "Get the title of the current page",
        || -> String {
            let handle = handle.clone();
            async move {
                handle
                    .ensure_browser()
                    .await?
                    .get_title()
                    .await?
                    .ok_or_else(|| ToolError::custom("no title"))
            }
        }
    )
    .with_namespace("page")
}

/// Register all Chrome page tools on an InterfaceBuilder.
pub fn register_tools(
    builder: crate::InterfaceBuilder,
    handle: &Arc<Browser>,
) -> crate::InterfaceBuilder {
    builder
        .tool(launch(handle))
        .tool(goto(handle))
        .tool(click(handle))
        .tool(type_into(handle))
        .tool(get_text(handle))
        .tool(get_html(handle))
        .tool(screenshot(handle))
        .tool(eval(handle))
        .tool(wait_for(handle))
        .tool(get_title(handle))
}
