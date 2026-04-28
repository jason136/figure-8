use flume::Sender;
use tokio::sync::oneshot;

use crate::IntoV8;
use crate::sandbox::marshall::TsType;

pub type SyncCallback = Box<
    dyn Fn(&mut v8::PinScope<'_, '_>, v8::FunctionCallbackArguments, v8::ReturnValue) + Send + Sync,
>;

pub type AsyncCallback = Box<
    dyn Fn(
            &mut v8::PinScope<'_, '_>,
            v8::FunctionCallbackArguments,
            v8::ReturnValue,
            &Sender<PendingPromise>,
        ) + Send
        + Sync,
>;

pub enum FnHandler {
    Sync(SyncCallback),
    Async(AsyncCallback),
}

pub trait DeferredValue: Send {
    fn into_v8<'s>(self: Box<Self>, scope: &mut v8::PinScope<'s, '_>) -> v8::Local<'s, v8::Value>;
}

impl<T: IntoV8 + Send> DeferredValue for T {
    fn into_v8<'s>(self: Box<Self>, scope: &mut v8::PinScope<'s, '_>) -> v8::Local<'s, v8::Value> {
        (*self).into_v8(scope)
    }
}

pub type ToolResult = Result<Box<dyn DeferredValue>, String>;

pub struct PendingPromise {
    pub resolver: v8::Global<v8::PromiseResolver>,
    pub rx: oneshot::Receiver<ToolResult>,
}

pub struct FnDef {
    pub name: String,
    pub description: String,
    pub params: Vec<(String, TsType)>,
    pub ret: TsType,
    pub handler: FnHandler,
}

#[macro_use]
pub mod macros {
    #[macro_export]
    macro_rules! fn_def_sync {
        ($name:literal, $desc:literal, || -> $ret:ty $body:block) => {
            $crate::fn_def_sync!($name, $desc, | | -> $ret $body)
        };
        ($name:literal, $desc:literal,
        |$($param:ident : $ty:ty),* $(,)?| -> $ret:ty $body:block
        ) => {{
            #[allow(unused_mut)]
            let mut params: Vec<(String, $crate::TsType)> = Vec::new();
            $({
                params.push((stringify!($param).to_string(), <$ty as $crate::TsTyped>::ts_type()));
            })*
            let ret = <$ret as $crate::TsTyped>::ts_type();

            $crate::sandbox::fn_def::FnDef {
                name: $name.into(),
                description: $desc.into(),
                params,
                ret,
                handler: $crate::sandbox::fn_def::FnHandler::Sync(
                    Box::new(move |scope, args, mut rv| {
                        let __span = tracing::span!(tracing::Level::INFO, "js_fn", name = $name, result = tracing::field::Empty);
                        let __guard = __span.enter();
                        #[allow(unused)]
                        let mut __i: i32 = 0;
                        $(
                            let $param: $ty = <$ty as $crate::FromV8>::from_v8(scope, args.get(__i))
                                .unwrap_or_else(|e| panic!("{}: arg '{}': {}", $name, stringify!($param), e));
                            __i += 1;
                        )*
                        #[allow(clippy::redundant_closure_call)]
                        let result: Result<$ret, $crate::FnDefError> = (|| $body)();
                        match result {
                            Ok(val) => {
                                __span.record("result", "ok");
                                rv.set($crate::IntoV8::into_v8(val, scope));
                            }
                            Err(e) => {
                                __span.record("result", tracing::field::display(&e));
                                let msg = $crate::v8::String::new(scope, &e.to_string()).unwrap();
                                scope.throw_exception($crate::v8::Exception::error(scope, msg));
                            }
                        }
                        drop(__guard);
                    }),
                ),
            }
        }};
    }

    #[macro_export]
    macro_rules! fn_def_async {
        ($name:literal, $desc:literal, || -> $ret:ty $body:block) => {
            $crate::fn_def_async!($name, $desc, | | -> $ret $body)
        };
        ($name:literal, $desc:literal,
        |$($param:ident : $ty:ty),* $(,)?| -> $ret:ty $body:block
        ) => {{
            #[allow(unused_mut)]
            let mut params: Vec<(String, $crate::TsType)> = Vec::new();
            $({
                params.push((stringify!($param).to_string(), <$ty as $crate::TsTyped>::ts_type()));
            })*
            let ret = $crate::TsType::Promise(Box::new(<$ret as $crate::TsTyped>::ts_type()));

            $crate::sandbox::fn_def::FnDef {
                name: $name.into(),
                description: $desc.into(),
                params,
                ret,
                handler: $crate::sandbox::fn_def::FnHandler::Async(
                    Box::new(move |scope, args, mut rv, pending| {
                        #[allow(unused)]
                        let _ = &args;
                        let mut __i: i32 = 0;
                        $(
                            let $param: $ty = <$ty as $crate::FromV8>::from_v8(scope, args.get(__i))
                                .unwrap_or_else(|e| panic!("{}: arg '{}': {}", $name, stringify!($param), e));
                            __i += 1;
                        )*

                        let resolver = $crate::v8::PromiseResolver::new(scope).unwrap();
                        rv.set(resolver.get_promise(scope).into());
                        let resolver = $crate::v8::Global::new(scope, resolver);

                        let (tx, rx) = tokio::sync::oneshot::channel();
                        pending.send($crate::sandbox::fn_def::PendingPromise { resolver, rx }).unwrap();

                        let __span = tracing::span!(tracing::Level::INFO, "js_fn", name = $name, result = tracing::field::Empty);
                        let __span_clone = __span.clone();
                        let __fut = $body;
                        tokio::spawn(tracing::Instrument::instrument(
                            async move {
                                let result: $crate::sandbox::fn_def::ToolResult = match __fut.await {
                                    Ok(val) => {
                                        __span_clone.record("result", "ok");
                                        Ok(Box::new(val) as Box<dyn $crate::sandbox::fn_def::DeferredValue>)
                                    }
                                    Err(e) => {
                                        __span_clone.record("result", tracing::field::display(&e));
                                        Err(e.to_string())
                                    }
                                };
                                let _ = tx.send(result);
                            },
                            __span,
                        ));
                    }),
                ),
            }
        }};
    }
}

#[derive(Debug, thiserror::Error)]
pub enum FnDefError {
    #[error("tool names and namespaces must not collide, found '{0}' and '{1}'")]
    NameCollision(String, String),

    #[error("{0}")]
    Serde(#[from] serde_json::Error),

    #[error("{0}")]
    Browser(#[from] chromiumoxide::error::CdpError),

    #[error("mcp intialization error: {0}")]
    McpInitialization(Box<rmcp::service::ClientInitializeError>),

    #[error("mcp error: {0}")]
    McpData(#[from] rmcp::ErrorData),

    #[error("mcp service error: {0}")]
    McpService(#[from] rmcp::service::ServiceError),

    #[error("mcp tool parameters expected to be json object, got {0}")]
    McpParamType(TsType),

    #[error("{0}")]
    Reqwest(#[from] reqwest::Error),

    #[error("invalid HTTP method: {0}")]
    InvalidMethod(String),

    #[error("invalid URL: {0}")]
    InvalidUrl(String),

    #[error("unsafe path: expected relative path with no '..', got '{0}'")]
    UnsafePath(String),

    #[error("{0}")]
    Io(#[from] std::io::Error),

    #[error("{0}")]
    Custom(String),

    #[error(transparent)]
    Other(#[from] Box<dyn std::error::Error + Send + Sync>),
}

impl FnDefError {
    pub fn custom(msg: impl Into<String>) -> Self {
        FnDefError::Custom(msg.into())
    }
}

impl From<rmcp::service::ClientInitializeError> for FnDefError {
    fn from(error: rmcp::service::ClientInitializeError) -> Self {
        FnDefError::McpInitialization(Box::new(error))
    }
}
