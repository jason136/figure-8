use std::collections::HashMap;

use crate::{
    ToolError, TsType,
    sandbox::tool::{PendingQueue, ToolDef, ToolHandler},
};

pub(crate) struct InjectedToolData {
    tool: ToolDef,
    pending: *const PendingQueue,
}

#[derive(Default)]
struct Node {
    tools: HashMap<String, ToolDef>,
    children: HashMap<String, Node>,
}

#[derive(Default)]
pub struct Interface {
    root: Node,
}

impl Interface {
    pub fn from_tools(tools: impl IntoIterator<Item = ToolDef>) -> Result<Self, ToolError> {
        let mut interface = Self::default();
        for tool in tools {
            interface.push(tool)?;
        }

        Ok(interface)
    }

    pub fn extend(&mut self, other: impl IntoIterator<Item = ToolDef>) -> Result<(), ToolError> {
        for tool in other {
            self.push(tool)?;
        }

        Ok(())
    }

    pub fn push(&mut self, tool: ToolDef) -> Result<(), ToolError> {
        let parts = tool.name.split('.').collect::<Vec<_>>();
        let func_name = *parts.last().unwrap();

        let node = parts[..parts.len() - 1]
            .iter()
            .fold(&mut self.root, |node, part| {
                node.children.entry(part.to_string()).or_default()
            });

        if node.tools.contains_key(func_name) {
            return Err(ToolError::NameCollision(
                tool.name.clone(),
                func_name.to_string(),
            ));
        }
        node.tools.insert(func_name.to_string(), tool);

        Ok(())
    }

    pub fn generate_dts(&self) -> String {
        fn render(node: &Node, indent: usize, is_root: bool) -> String {
            let pad = "  ".repeat(indent);
            let mut out = String::new();

            for (leaf_name, tool) in &node.tools {
                let params = tool
                    .params
                    .iter()
                    .map(|(name, ty)| match ty {
                        TsType::Optional(inner) => format!("{name}?: {inner}"),
                        _ => format!("{name}: {ty}"),
                    })
                    .collect::<Vec<_>>()
                    .join(", ");

                if is_root {
                    out.push_str(&format!(
                        "{pad}declare function {leaf_name}({params}): {};\n",
                        tool.ret
                    ));
                } else {
                    out.push_str(&format!("{pad}{leaf_name}({params}): {};\n", tool.ret));
                }
            }

            for (name, child) in &node.children {
                if is_root {
                    out.push_str(&format!("{pad}declare const {name}: {{\n"));
                } else {
                    out.push_str(&format!("{pad}{name}: {{\n"));
                }
                out.push_str(&render(child, indent + 1, false));
                out.push_str(&format!("{pad}}};\n"));
            }

            out
        }

        render(&self.root, 0, true)
    }

    pub(crate) fn inject(
        self,
        scope: &mut v8::PinScope<'_, '_>,
        global: v8::Local<v8::Object>,
        pending: *const PendingQueue,
    ) -> Vec<*mut InjectedToolData> {
        let mut ptrs = Vec::new();
        let mut stack = vec![(self.root, global)];

        while let Some((node, target)) = stack.pop() {
            for (func_name, tool) in node.tools {
                let key = v8::String::new(scope, &func_name).unwrap();

                let data = Box::into_raw(Box::new(InjectedToolData { tool, pending }));
                let external = v8::External::new(scope, data as *mut std::ffi::c_void);
                let func = v8::Function::builder(tool_callback)
                    .data(external.into())
                    .build(scope)
                    .unwrap();

                target.set(scope, key.into(), func.into());
                ptrs.push(data);
            }

            for (name, child) in node.children {
                let obj = v8::Object::new(scope);
                let key = v8::String::new(scope, &name).unwrap();
                target.set(scope, key.into(), obj.into());
                stack.push((child, obj));
            }
        }

        ptrs
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
