use std::sync::Arc;

use chromiumoxide::Page;
use futures::StreamExt;
use tokio::sync::RwLock;

use crate::sandbox::tool::ToolDef;
use crate::{ToolError, tool_async};

#[derive(Clone)]
pub struct Browser {
    inner: Arc<RwLock<BrowserInner>>,
}

enum BrowserInner {
    Pending(Box<chromiumoxide::BrowserConfig>),
    Initialized {
        config: Box<chromiumoxide::BrowserConfig>,
        browser: Box<chromiumoxide::Browser>,
        page: Arc<Page>,
    },
}

impl Browser {
    pub fn new(config: chromiumoxide::BrowserConfig) -> Self {
        Browser {
            inner: Arc::new(RwLock::new(BrowserInner::Pending(Box::new(config)))),
        }
    }

    pub fn default_config() -> Result<chromiumoxide::BrowserConfig, ToolError> {
        chromiumoxide::BrowserConfig::builder()
            .arg("--disable-gpu")
            .arg("--disable-dev-shm-usage")
            .build()
            .map_err(|e| ToolError::custom(e.to_string()))
    }

    async fn ensure_page(&self) -> Result<Arc<Page>, ToolError> {
        match &*self.inner.read().await {
            BrowserInner::Pending(config) => {
                let (browser, mut handler) =
                    chromiumoxide::Browser::launch(*config.clone()).await?;

                tokio::spawn(async move { while handler.next().await.is_some() {} });

                let page = browser.new_page("about:blank").await?;
                page.wait_for_navigation().await?;
                let arc_page = Arc::new(page);

                *self.inner.write().await = BrowserInner::Initialized {
                    config: config.clone(),
                    browser: Box::new(browser),
                    page: arc_page.clone(),
                };

                Ok(arc_page)
            }
            BrowserInner::Initialized { page, .. } => Ok(page.clone()),
        }
    }

    async fn shutdown_browser(&self) -> Result<(), ToolError> {
        if let BrowserInner::Initialized {
            browser, config, ..
        } = &mut *self.inner.write().await
        {
            browser.close().await?;

            *self.inner.write().await = BrowserInner::Pending(config.clone());
        }

        Ok(())
    }
}

pub fn shutdown(handle: Browser) -> ToolDef {
    tool_async!("shutdown", "Shutdown the browser", || -> () {
        let handle = handle.clone();
        async move {
            handle.shutdown_browser().await?;
            Ok::<_, ToolError>(())
        }
    })
    .with_namespace("page")
}

pub fn goto(handle: Browser) -> ToolDef {
    tool_async!("goto", "Navigate to a URL", |url: String| -> () {
        let handle = handle.clone();
        async move {
            let page = handle.ensure_page().await?;
            page.goto(&url).await?;
            page.wait_for_navigation().await?;
            Ok::<_, ToolError>(())
        }
    })
    .with_namespace("page")
}

pub fn click(handle: Browser) -> ToolDef {
    tool_async!(
        "click",
        "Click an element by CSS selector",
        |selector: String| -> () {
            let handle = handle.clone();
            async move {
                let page = handle.ensure_page().await?;
                page.find_element(&selector).await?.click().await?;
                Ok::<_, ToolError>(())
            }
        }
    )
    .with_namespace("page")
}

pub fn type_into(handle: Browser) -> ToolDef {
    tool_async!("type", "Type text into an element", |selector: String,
                                                      text: String|
     -> () {
        let handle = handle.clone();
        async move {
            let page = handle.ensure_page().await?;
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

pub fn get_text(handle: Browser) -> ToolDef {
    tool_async!(
        "getText",
        "Get inner text of an element",
        |selector: String| -> String {
            let handle = handle.clone();
            async move {
                let page = handle.ensure_page().await?;
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

pub fn get_html(handle: Browser) -> ToolDef {
    tool_async!("getHtml", "Get the full HTML of the page", || -> String {
        let handle = handle.clone();
        async move { Ok::<_, ToolError>(handle.ensure_page().await?.content().await?) }
    })
    .with_namespace("page")
}

pub fn screenshot(handle: Browser) -> ToolDef {
    tool_async!("screenshot", "Capture a PNG screenshot as base64", || -> String {
        let handle = handle.clone();
        async move {
            use base64::Engine;
            let page = handle.ensure_page().await?;
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

pub fn eval(handle: Browser) -> ToolDef {
    tool_async!(
        "eval",
        "Evaluate JavaScript in the browser page",
        |code: String| -> String {
            let handle = handle.clone();
            async move {
                let page = handle.ensure_page().await?;
                let val: serde_json::Value = page.evaluate_expression(&code).await?.into_value()?;
                Ok::<_, ToolError>(val.to_string())
            }
        }
    )
    .with_namespace("page")
}

pub fn wait_for(handle: Browser) -> ToolDef {
    tool_async!(
        "waitFor",
        "Wait for an element matching a CSS selector",
        |selector: String| -> () {
            let handle = handle.clone();
            async move {
                handle.ensure_page().await?.find_element(&selector).await?;
                Ok::<_, ToolError>(())
            }
        }
    )
    .with_namespace("page")
}

pub fn get_title(handle: Browser) -> ToolDef {
    tool_async!(
        "getTitle",
        "Get the title of the current page",
        || -> String {
            let handle = handle.clone();
            async move {
                handle
                    .ensure_page()
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
    handle: &Browser,
) -> crate::InterfaceBuilder {
    builder
        .tool(goto(handle.clone()))
        .tool(click(handle.clone()))
        .tool(type_into(handle.clone()))
        .tool(get_text(handle.clone()))
        .tool(get_html(handle.clone()))
        .tool(screenshot(handle.clone()))
        .tool(eval(handle.clone()))
        .tool(wait_for(handle.clone()))
        .tool(get_title(handle.clone()))
}
