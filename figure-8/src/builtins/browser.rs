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

    pub fn default_tools(&self) -> Vec<ToolDef> {
        vec![
            goto(self.clone()),
            click(self.clone()),
            type_into(self.clone()),
            get_text(self.clone()),
            get_html(self.clone()),
            screenshot(self.clone()),
            eval(self.clone()),
        ]
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
    tool_async!("page.shutdown", "Shutdown the browser", || -> () {
        let handle = handle.clone();
        async move {
            handle.shutdown_browser().await?;
            Ok::<_, ToolError>(())
        }
    })
}

pub fn goto(handle: Browser) -> ToolDef {
    tool_async!("page.goto", "Navigate to a URL", |url: String| -> () {
        let handle = handle.clone();
        async move {
            let page = handle.ensure_page().await?;
            page.goto(&url).await?;
            page.wait_for_navigation().await?;
            Ok::<_, ToolError>(())
        }
    })
}

pub fn click(handle: Browser) -> ToolDef {
    tool_async!(
        "page.click",
        "Click an element by CSS selector",
        |selector: String| -> () {
            let handle = handle.clone();
            async move {
                let page = &handle.ensure_page().await?;
                page.find_element(&selector).await?.click().await?;
                Ok::<_, ToolError>(())
            }
        }
    )
}

pub fn type_into(handle: Browser) -> ToolDef {
    tool_async!(
        "page.type",
        "Type text into an element",
        |selector: String, text: String| -> () {
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
        }
    )
}

pub fn get_text(handle: Browser) -> ToolDef {
    tool_async!(
        "page.getText",
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
}

pub fn get_html(handle: Browser) -> ToolDef {
    tool_async!(
        "page.getHtml",
        "Get the full HTML of the page",
        || -> String {
            let handle = handle.clone();
            async move { Ok::<_, ToolError>(handle.ensure_page().await?.content().await?) }
        }
    )
}

pub fn screenshot(handle: Browser) -> ToolDef {
    tool_async!(
        "page.screenshot",
        "Capture a PNG screenshot as base64",
        || -> String {
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
        }
    )
}

pub fn eval(handle: Browser) -> ToolDef {
    tool_async!(
        "page.eval",
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
}

pub fn wait_for(handle: Browser) -> ToolDef {
    tool_async!(
        "page.waitFor",
        "Wait for an element matching a CSS selector",
        |selector: String| -> () {
            let handle = handle.clone();
            async move {
                handle.ensure_page().await?.find_element(&selector).await?;
                Ok::<_, ToolError>(())
            }
        }
    )
}

pub fn get_title(handle: Browser) -> ToolDef {
    tool_async!(
        "page.getTitle",
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
}
