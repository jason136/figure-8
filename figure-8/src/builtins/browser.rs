use std::sync::Arc;

use chromiumoxide::Page;
use futures::StreamExt;
use tokio::sync::RwLock;

use crate::sandbox::interface::Interface;
use crate::sandbox::fn_def::FnDef;
use crate::{JsApi, FnDefError, fn_def_async};

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

impl Interface for Browser {
    fn extend_api(&self, js_api: &mut JsApi) -> Result<(), FnDefError> {
        js_api.extend_fn_defs(vec![
            goto(self.clone()),
            click(self.clone()),
            type_into(self.clone()),
            get_text(self.clone()),
            get_html(self.clone()),
        ])?;

        Ok(())
    }
}

impl Browser {
    pub fn new(config: chromiumoxide::BrowserConfig) -> Self {
        Browser {
            inner: Arc::new(RwLock::new(BrowserInner::Pending(Box::new(config)))),
        }
    }

    pub fn default_config() -> Result<chromiumoxide::BrowserConfig, FnDefError> {
        chromiumoxide::BrowserConfig::builder()
            .arg("--disable-gpu")
            .arg("--disable-dev-shm-usage")
            .build()
            .map_err(|e| FnDefError::custom(e.to_string()))
    }

    async fn ensure_page(&self) -> Result<Arc<Page>, FnDefError> {
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

    async fn shutdown_browser(&self) -> Result<(), FnDefError> {
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

pub fn shutdown(handle: Browser) -> FnDef {
    fn_def_async!("page.shutdown", "Shutdown the browser", || -> () {
        let handle = handle.clone();
        async move {
            handle.shutdown_browser().await?;
            Ok::<_, FnDefError>(())
        }
    })
}

pub fn goto(handle: Browser) -> FnDef {
    fn_def_async!("page.goto", "Navigate to a URL", |url: String| -> () {
        let handle = handle.clone();
        async move {
            let page = handle.ensure_page().await?;
            page.goto(&url).await?;
            page.wait_for_navigation().await?;
            Ok::<_, FnDefError>(())
        }
    })
}

pub fn click(handle: Browser) -> FnDef {
    fn_def_async!(
        "page.click",
        "Click an element by CSS selector",
        |selector: String| -> () {
            let handle = handle.clone();
            async move {
                let page = &handle.ensure_page().await?;
                page.find_element(&selector).await?.click().await?;
                Ok::<_, FnDefError>(())
            }
        }
    )
}

pub fn type_into(handle: Browser) -> FnDef {
    fn_def_async!(
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
                Ok::<_, FnDefError>(())
            }
        }
    )
}

pub fn get_text(handle: Browser) -> FnDef {
    fn_def_async!(
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
                    .ok_or_else(|| FnDefError::custom("element has no text"))
            }
        }
    )
}

pub fn get_html(handle: Browser) -> FnDef {
    fn_def_async!(
        "page.getHtml",
        "Get the full HTML of the page",
        || -> String {
            let handle = handle.clone();
            async move { Ok::<_, FnDefError>(handle.ensure_page().await?.content().await?) }
        }
    )
}

pub fn screenshot(handle: Browser) -> FnDef {
    fn_def_async!(
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
                Ok::<_, FnDefError>(base64::engine::general_purpose::STANDARD.encode(&png))
            }
        }
    )
}

pub fn eval(handle: Browser) -> FnDef {
    fn_def_async!(
        "page.eval",
        "Evaluate JavaScript in the browser page",
        |code: String| -> String {
            let handle = handle.clone();
            async move {
                let page = handle.ensure_page().await?;
                let val: serde_json::Value = page.evaluate_expression(&code).await?.into_value()?;
                Ok::<_, FnDefError>(val.to_string())
            }
        }
    )
}

pub fn wait_for(handle: Browser) -> FnDef {
    fn_def_async!(
        "page.waitFor",
        "Wait for an element matching a CSS selector",
        |selector: String| -> () {
            let handle = handle.clone();
            async move {
                handle.ensure_page().await?.find_element(&selector).await?;
                Ok::<_, FnDefError>(())
            }
        }
    )
}

pub fn get_title(handle: Browser) -> FnDef {
    fn_def_async!(
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
                    .ok_or_else(|| FnDefError::custom("no title"))
            }
        }
    )
}
