use std::collections::HashMap;

use crate::{
    FnDef, FnDefError, IntoV8, JsApi, TsType, TsTyped, fn_def_async, fn_def_sync,
    sandbox::{interface::Interface, marshall::ObjExt},
};

#[derive(Clone)]
pub struct Fetch {
    client: reqwest::Client,
}

impl Interface for Fetch {
    fn extend_api(&self, js_api: &mut JsApi) -> Result<(), FnDefError> {
        js_api.extend_fn_defs(vec![raw_fetch(self.clone()), utf8_decode(), utf8_encode()])?;

        js_api.push_polyfill(include_str!("polyfills/fetch.js"));
        js_api.push_dts("", include_str!("polyfills/fetch.d.ts"));

        Ok(())
    }
}

impl Fetch {
    pub fn new(client: reqwest::Client) -> Self {
        Fetch { client }
    }
}

struct FetchResponse {
    status: u16,
    status_text: String,
    ok: bool,
    headers: HashMap<String, String>,
    url: String,
    body: Vec<u8>,
    redirected: bool,
}

impl TsTyped for FetchResponse {
    fn ts_type() -> TsType {
        TsType::Unknown
    }
}

impl IntoV8 for FetchResponse {
    fn into_v8<'s>(self, scope: &mut v8::PinScope<'s, '_>) -> v8::Local<'s, v8::Value> {
        let obj = v8::Object::new(scope);

        obj.set_obj(scope, "status", self.status as u32);
        obj.set_obj(scope, "statusText", self.status_text);
        obj.set_obj(scope, "ok", self.ok);
        obj.set_obj(scope, "url", self.url);
        obj.set_obj(scope, "redirected", self.redirected);
        obj.set_obj(scope, "headers", self.headers);
        obj.set_obj(scope, "body", self.body.clone());

        obj.into()
    }
}

fn raw_fetch(handle: Fetch) -> FnDef {
    fn_def_async!(
        "__rawFetch",
        "Internal: perform an HTTP request",
        |url: String,
         method: String,
         headers_json: String,
         body: Option<Vec<u8>>|
         -> FetchResponse {
            let client = handle.client.clone();
            async move {
                let method = reqwest::Method::from_bytes(method.as_bytes())
                    .map_err(|e| FnDefError::InvalidMethod(e.to_string()))?;

                let parsed_url =
                    reqwest::Url::parse(&url).map_err(|e| FnDefError::InvalidUrl(e.to_string()))?;

                let mut req = client.request(method, parsed_url.clone());

                if let Ok(headers) = serde_json::from_str::<HashMap<String, String>>(&headers_json)
                {
                    for (k, v) in headers {
                        req = req.header(k, v);
                    }
                }

                if let Some(b) = body {
                    req = req.body(b);
                }

                let resp = req.send().await?;
                let status = resp.status().as_u16();
                let status_text = resp.status().canonical_reason().unwrap_or("").to_string();
                let redirected = *resp.url() != parsed_url;
                let final_url = resp.url().to_string();

                let headers = resp
                    .headers()
                    .iter()
                    .filter_map(|(k, v)| v.to_str().ok().map(|v| (k.to_string(), v.to_string())))
                    .collect::<HashMap<_, _>>();

                let body = resp.bytes().await?.to_vec();

                Ok::<_, FnDefError>(FetchResponse {
                    status,
                    status_text,
                    ok: (200..300).contains(&status),
                    headers,
                    redirected,
                    url: final_url,
                    body,
                })
            }
        }
    )
}

fn utf8_decode() -> FnDef {
    fn_def_sync!(
        "__utf8Decode",
        "Decode UTF-8 bytes to a string",
        |bytes: Vec<u8>| -> String { Ok(String::from_utf8_lossy(&bytes).into_owned()) }
    )
}

fn utf8_encode() -> FnDef {
    fn_def_sync!(
        "__utf8Encode",
        "Encode a string to UTF-8 bytes",
        |text: String| -> Vec<u8> { Ok(text.into_bytes()) }
    )
}
