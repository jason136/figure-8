use std::sync::Arc;

use crate::sandbox::tool::{Pending, ToolDef, ToolHandler};

pub struct InjectedToolData {
    tool: Arc<ToolDef>,
    pending: *const Pending,
}

pub struct Interface {
    pub name: String,
    tools: Vec<Arc<ToolDef>>,
}

impl Interface {
    pub fn builder(name: impl Into<String>) -> InterfaceBuilder {
        InterfaceBuilder {
            name: name.into(),
            tools: Vec::new(),
        }
    }

    pub fn generate_dts(&self) -> String {
        self.tools
            .iter()
            .map(|t| t.ts_declaration.as_str())
            .collect::<Vec<_>>()
            .join("\n")
            + "\n"
    }

    pub fn inject(
        &self,
        scope: &mut v8::PinScope<'_, '_>,
        global: v8::Local<v8::Object>,
        pending: *const Pending,
    ) -> Vec<*mut InjectedToolData> {
        self.tools
            .iter()
            .map(|tool| {
                let data = Box::into_raw(Box::new(InjectedToolData {
                    tool: Arc::clone(tool),
                    pending,
                }));
                let external = v8::External::new(scope, data as *mut std::ffi::c_void);
                let func = v8::Function::builder(tool_callback)
                    .data(external.into())
                    .build(scope)
                    .expect("failed to build V8 function");
                let key = v8::String::new(scope, &tool.name).unwrap();
                global.set(scope, key.into(), func.into());
                data
            })
            .collect()
    }
}

pub unsafe fn free_injected_data(ptrs: Vec<*mut InjectedToolData>) {
    for ptr in ptrs {
        drop(unsafe { Box::from_raw(ptr) });
    }
}

fn tool_callback(
    scope: &mut v8::PinScope<'_, '_>,
    args: v8::FunctionCallbackArguments,
    rv: v8::ReturnValue,
) {
    let external: v8::Local<v8::External> = args.data().try_into().unwrap();
    let data = unsafe { &*(external.value() as *const InjectedToolData) };

    match &data.tool.handler {
        ToolHandler::Sync(cb) => cb(scope, args, rv),
        ToolHandler::Async(cb) => {
            let pending = unsafe { &*data.pending };
            cb(scope, args, rv, pending);
        }
    }
}

pub struct InterfaceBuilder {
    name: String,
    tools: Vec<Arc<ToolDef>>,
}

impl InterfaceBuilder {
    pub fn tool(mut self, tool: ToolDef) -> Self {
        self.tools.push(Arc::new(tool));
        self
    }

    pub fn build(self) -> Interface {
        Interface {
            name: self.name,
            tools: self.tools,
        }
    }
}
