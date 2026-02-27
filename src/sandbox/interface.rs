use std::collections::HashMap;

use crate::sandbox::tool::{PendingQueue, ToolDef, ToolHandler};

pub(crate) struct InjectedToolData {
    tool: ToolDef,
    pending: *const PendingQueue,
}

pub struct Interface {
    pub name: String,
    pub tools: Vec<ToolDef>,
}

impl Interface {
    pub fn builder(name: impl Into<String>) -> InterfaceBuilder {
        InterfaceBuilder {
            name: name.into(),
            tools: Vec::new(),
        }
    }

    pub fn generate_dts(&self) -> String {
        let (globals, namespaces) = self.tools.iter().fold(
            (Vec::new(), HashMap::<&str, Vec<&str>>::new()),
            |(mut globals, mut namespaces), tool| {
                if let Some(ns) = tool.namespace.as_ref() {
                    namespaces.entry(ns).or_default().push(&tool.ts_declaration);
                } else {
                    globals.push(tool.ts_declaration.as_str());
                }

                (globals, namespaces)
            },
        );

        let mut dts = globals.join("\n");
        if !globals.is_empty() {
            dts.push('\n');
        }

        namespaces.iter().fold(dts, |mut dts, (ns, decls)| {
            dts.push_str(&format!("declare const {ns}: {{\n"));
            for decl in decls {
                let method = decl.strip_prefix("declare function ").unwrap_or(decl);
                dts.push_str(&format!("  {method}\n"));
            }
            dts.push_str("};\n");
            dts
        })
    }

    pub(crate) fn inject(
        self,
        scope: &mut v8::PinScope<'_, '_>,
        global: v8::Local<v8::Object>,
        pending: *const PendingQueue,
    ) -> Vec<*mut InjectedToolData> {
        let mut ns_objects: HashMap<String, v8::Local<v8::Object>> = HashMap::new();
        self.tools
            .into_iter()
            .map(|tool| {
                let key = v8::String::new(scope, &tool.name).unwrap();

                let target = tool
                    .namespace
                    .as_ref()
                    .map(|ns| {
                        ns_objects.entry(ns.clone()).or_insert_with(|| {
                            let obj = v8::Object::new(scope);
                            let key = v8::String::new(scope, ns).unwrap();
                            global.set(scope, key.into(), obj.into());

                            obj
                        });

                        *ns_objects.get(ns.as_str()).unwrap()
                    })
                    .unwrap_or(global);

                let data = Box::into_raw(Box::new(InjectedToolData { tool, pending }));

                let external = v8::External::new(scope, data as *mut std::ffi::c_void);
                let func = v8::Function::builder(tool_callback)
                    .data(external.into())
                    .build(scope)
                    .unwrap();

                target.set(scope, key.into(), func.into());

                data
            })
            .collect()
    }
}

pub(crate) unsafe fn free_injected_data(ptrs: Vec<*mut InjectedToolData>) {
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
            cb(scope, args, rv, unsafe { &*data.pending });
        }
    }
}

pub struct InterfaceBuilder {
    name: String,
    tools: Vec<ToolDef>,
}

impl InterfaceBuilder {
    pub fn tool(mut self, tool: ToolDef) -> Self {
        self.tools.push(tool);
        self
    }

    pub fn build(self) -> Interface {
        Interface {
            name: self.name,
            tools: self.tools,
        }
    }
}
