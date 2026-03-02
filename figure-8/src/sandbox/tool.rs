use std::cell::RefCell;
use std::collections::VecDeque;

use tokio::sync::oneshot;

use crate::ToV8;
use crate::sandbox::marshal::TsType;

pub type SyncCallback = Box<
    dyn Fn(&mut v8::PinScope<'_, '_>, v8::FunctionCallbackArguments, v8::ReturnValue) + Send + Sync,
>;

pub type AsyncCallback = Box<
    dyn Fn(&mut v8::PinScope<'_, '_>, v8::FunctionCallbackArguments, v8::ReturnValue, &PendingQueue)
        + Send
        + Sync,
>;

pub enum ToolHandler {
    Sync(SyncCallback),
    Async(AsyncCallback),
}

pub trait DeferredValue: Send {
    fn to_v8<'s>(self: Box<Self>, scope: &mut v8::PinScope<'s, '_>) -> v8::Local<'s, v8::Value>;
}

impl<T: ToV8 + Send> DeferredValue for T {
    fn to_v8<'s>(self: Box<Self>, scope: &mut v8::PinScope<'s, '_>) -> v8::Local<'s, v8::Value> {
        (*self).to_v8(scope)
    }
}

pub type ToolResult = Result<Box<dyn DeferredValue>, String>;

pub struct PendingPromise {
    pub resolver: v8::Global<v8::PromiseResolver>,
    pub rx: oneshot::Receiver<ToolResult>,
}

pub type PendingQueue = RefCell<VecDeque<PendingPromise>>;

pub struct ToolDef {
    pub name: String,
    pub description: String,
    pub params: Vec<(String, TsType)>,
    pub ret: TsType,
    pub handler: ToolHandler,
}

#[macro_use]
pub mod macros {
    #[macro_export]
    macro_rules! tool_sync {
        ($name:literal, $desc:literal, || -> $ret:ty $body:block) => {
            $crate::tool_sync!($name, $desc, | | -> $ret $body)
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

            $crate::sandbox::tool::ToolDef {
                name: $name.into(),
                description: $desc.into(),
                params,
                ret,
                handler: $crate::sandbox::tool::ToolHandler::Sync(
                    Box::new(move |scope, args, mut rv| {
                        #[allow(unused)]
                        let mut __i: i32 = 0;
                        $(
                            let $param: $ty = <$ty as $crate::FromV8>::from_v8(scope, args.get(__i))
                                .unwrap_or_else(|e| panic!("{}: arg '{}': {}", $name, stringify!($param), e));
                            __i += 1;
                        )*
                        #[allow(clippy::redundant_closure_call)]
                        let result: Result<$ret, $crate::ToolError> = (|| $body)();
                        match result {
                            Ok(val) => rv.set($crate::ToV8::to_v8(&val, scope)),
                            Err(e) => {
                                let msg = $crate::v8::String::new(scope, &e.to_string()).unwrap();
                                scope.throw_exception($crate::v8::Exception::error(scope, msg));
                            }
                        }
                    }),
                ),
            }
        }};
    }

    #[macro_export]
    macro_rules! tool_async {
        ($name:literal, $desc:literal, || -> $ret:ty $body:block) => {
            $crate::tool_async!($name, $desc, | | -> $ret $body)
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

            $crate::sandbox::tool::ToolDef {
                name: $name.into(),
                description: $desc.into(),
                params,
                ret,
                handler: $crate::sandbox::tool::ToolHandler::Async(
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
                        pending.borrow_mut().push_back($crate::sandbox::tool::PendingPromise { resolver, rx });

                        let __fut = $body;
                        tokio::spawn(async move {
                            let result: $crate::sandbox::tool::ToolResult = match __fut.await {
                                Ok(val) => Ok(Box::new(val) as Box<dyn $crate::sandbox::tool::DeferredValue>),
                                Err(e) => Err(e.to_string()),
                            };
                            let _ = tx.send(result);
                        });
                    }),
                ),
            }
        }};
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    #[error("tool names and namespaces must not collide, found '{0}' and '{1}'")]
    NameCollision(String, String),

    #[error("{0}")]
    Serde(#[from] serde_json::Error),

    #[error("{0}")]
    Browser(#[from] chromiumoxide::error::CdpError),

    #[error("{0}")]
    Custom(String),

    #[error(transparent)]
    Other(#[from] Box<dyn std::error::Error + Send + Sync>),
}

impl ToolError {
    pub fn custom(msg: impl Into<String>) -> Self {
        ToolError::Custom(msg.into())
    }
}
