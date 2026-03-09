use std::collections::HashSet;
use std::path::Path;

use oxc_allocator::Allocator;
use oxc_ast::ast::{BindingPattern, Declaration, ModuleExportName, Statement};
use oxc_codegen::Codegen;
use oxc_parser::Parser;
use oxc_semantic::SemanticBuilder;
use oxc_span::SourceType;
use oxc_transformer::{TransformOptions, Transformer};

pub fn prepare_repl_source(code: String) -> String {
    let allocator = Allocator::default();
    let source_type = SourceType::ts().with_module(true);
    let mut ret = Parser::new(&allocator, &code, source_type).parse();

    let js = if ret.panicked {
        return code;
    } else {
        let sem = SemanticBuilder::new().build(&ret.program);
        let scoping = sem.semantic.into_scoping();

        let options = TransformOptions::default();
        let _ = Transformer::new(&allocator, Path::new("<repl>.ts"), &options)
            .build_with_scoping(scoping, &mut ret.program);

        Codegen::new().build(&ret.program).code
    };

    let decl_names = {
        let mut declared: Vec<String> = Vec::new();
        let mut exported: HashSet<String> = HashSet::new();

        for stmt in &ret.program.body {
            match stmt {
                Statement::VariableDeclaration(var) => {
                    for d in &var.declarations {
                        collect_bindings(&d.id, &mut declared);
                    }
                }
                Statement::FunctionDeclaration(f) => {
                    if let Some(id) = &f.id {
                        declared.push(id.name.to_string());
                    }
                }
                Statement::ClassDeclaration(c) => {
                    if let Some(id) = &c.id {
                        declared.push(id.name.to_string());
                    }
                }
                Statement::ExportNamedDeclaration(exp) => {
                    if let Some(decl) = &exp.declaration {
                        match decl {
                            Declaration::VariableDeclaration(var) => {
                                for d in &var.declarations {
                                    let mut names = Vec::new();
                                    collect_bindings(&d.id, &mut names);
                                    exported.extend(names);
                                }
                            }
                            Declaration::FunctionDeclaration(f) => {
                                if let Some(id) = &f.id {
                                    exported.insert(id.name.to_string());
                                }
                            }
                            Declaration::ClassDeclaration(c) => {
                                if let Some(id) = &c.id {
                                    exported.insert(id.name.to_string());
                                }
                            }
                            _ => {}
                        }
                    }
                    if exp.source.is_none() {
                        for spec in &exp.specifiers {
                            let name = match &spec.local {
                                ModuleExportName::IdentifierName(id) => id.name.to_string(),
                                ModuleExportName::IdentifierReference(id) => id.name.to_string(),
                                ModuleExportName::StringLiteral(s) => s.value.to_string(),
                            };
                            exported.insert(name);
                        }
                    }
                }
                Statement::ExportDefaultDeclaration(_) => {
                    exported.insert("default".to_string());
                }
                _ => {}
            }
        }

        declared.retain(|name| !exported.contains(name));
        declared
    };

    if decl_names.is_empty() {
        js.to_string()
    } else {
        format!("{js}\nexport {{ {} }};", decl_names.join(", "))
    }
}

fn collect_bindings(pat: &BindingPattern<'_>, out: &mut Vec<String>) {
    match pat {
        BindingPattern::BindingIdentifier(id) => out.push(id.name.to_string()),
        BindingPattern::ObjectPattern(obj) => {
            for prop in &obj.properties {
                collect_bindings(&prop.value, out);
            }
            if let Some(rest) = &obj.rest {
                collect_bindings(&rest.argument, out);
            }
        }
        BindingPattern::ArrayPattern(arr) => {
            for elem in arr.elements.iter().flatten() {
                collect_bindings(elem, out);
            }
            if let Some(rest) = &arr.rest {
                collect_bindings(&rest.argument, out);
            }
        }
        BindingPattern::AssignmentPattern(assign) => collect_bindings(&assign.left, out),
    }
}

pub fn globalize_namespace(scope: &mut v8::PinScope<'_, '_>, module: v8::Local<v8::Module>) {
    let namespace = module.get_module_namespace();
    let ns_obj: v8::Local<v8::Object> = namespace.try_into().unwrap();
    let global = scope.get_current_context().global(scope);

    let Some(keys) = ns_obj.get_own_property_names(scope, Default::default()) else {
        return;
    };

    for i in 0..keys.length() {
        if let Some(key) = keys.get_index(scope, i)
            && let Some(val) = ns_obj.get(scope, key)
        {
            global.set(scope, key, val);
        }
    }
}
