use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::quote;
use syn::{Expr, ItemFn, LitStr, Meta, parse2};

#[proc_macro_attribute]
pub fn tool(attr: TokenStream, item: TokenStream) -> TokenStream {
    match tool_impl(attr.into(), item.into()) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

fn tool_impl(
    attr: proc_macro2::TokenStream,
    item: proc_macro2::TokenStream,
) -> Result<proc_macro2::TokenStream, syn::Error> {
    let func: ItemFn = parse2(item)?;
    let description = parse_description(attr)?;

    let fn_name = &func.sig.ident;
    let fn_name_str = fn_name.to_string();
    let tool_def_name = syn::Ident::new(&format!("{}_tool_def", fn_name), fn_name.span());
    let is_async = func.sig.asyncness.is_some();
    let fn_name_lit = LitStr::new(&fn_name_str, fn_name.span());
    let desc_lit = LitStr::new(&description, Span::call_site());

    let params = extract_params(&func)?;
    let ret_type = extract_return_type(&func.sig.output);

    let param_idents: Vec<syn::Ident> = params
        .iter()
        .map(|(name, _)| syn::Ident::new(name, Span::call_site()))
        .collect();

    let extraction_stmts: Vec<proc_macro2::TokenStream> = params
        .iter()
        .enumerate()
        .map(|(i, (name, ty))| {
            let ident = syn::Ident::new(name, Span::call_site());
            let idx = i as i32;
            let name_lit = LitStr::new(name, Span::call_site());
            quote! {
                let #ident = <#ty as figure_8::FromV8>::from_v8(
                    scope, args.get(#idx),
                ).unwrap_or_else(|e| panic!("{}: arg '{}': {}", #fn_name_lit, #name_lit, e));
            }
        })
        .collect();

    let ts_param_parts: Vec<proc_macro2::TokenStream> = params
        .iter()
        .map(|(name, ty)| {
            let name_lit = LitStr::new(name, Span::call_site());
            quote! {
                {
                    let ts = <#ty as figure_8::TsTyped>::ts_type();
                    match &ts {
                        figure_8::TsType::Optional(inner) => format!("{}?: {}", #name_lit, inner),
                        _ => format!("{}: {}", #name_lit, ts),
                    }
                }
            }
        })
        .collect();

    let output = if is_async {
        let ts_decl_expr = quote! {
            let params_str: String = [#(#ts_param_parts),*].join(", ");
            let ret_ts = <#ret_type as figure_8::TsTyped>::ts_type();
            format!("declare function {}({}): Promise<{}>;", #fn_name_lit, params_str, ret_ts)
        };

        quote! {
            #func

            pub fn #tool_def_name() -> figure_8::ToolDef {
                let ts_declaration = { #ts_decl_expr };
                figure_8::ToolDef::new_async(
                    #fn_name_lit, #desc_lit, ts_declaration,
                    Box::new(|scope, args, mut rv, pending| {
                        #(#extraction_stmts)*

                        let resolver = figure_8::v8::PromiseResolver::new(scope).unwrap();
                        rv.set(resolver.get_promise(scope).into());
                        let resolver = figure_8::v8::Global::new(scope, resolver);

                        let (tx, rx) = tokio::sync::oneshot::channel();
                        pending.borrow_mut().push_back(figure_8::PendingPromise { resolver, rx });

                        tokio::spawn(async move {
                            let result = match #fn_name(#(#param_idents),*).await {
                                Ok(val) => Ok(Box::new(val) as Box<dyn figure_8::DeferredValue>),
                                Err(e) => Err(e.to_string()),
                            };
                            let _ = tx.send(result);
                        });
                    }),
                )
            }
        }
    } else {
        let ts_decl_expr = quote! {
            let params_str: String = [#(#ts_param_parts),*].join(", ");
            let ret_ts = <#ret_type as figure_8::TsTyped>::ts_type();
            format!("declare function {}({}): {};", #fn_name_lit, params_str, ret_ts)
        };

        quote! {
            #func

            pub fn #tool_def_name() -> figure_8::ToolDef {
                let ts_declaration = { #ts_decl_expr };
                figure_8::ToolDef::new_sync(
                    #fn_name_lit, #desc_lit, ts_declaration,
                    Box::new(|scope, args, mut rv| {
                        #(#extraction_stmts)*
                        match #fn_name(#(#param_idents),*) {
                            Ok(val) => rv.set(figure_8::IntoV8::into_v8(val, scope)),
                            Err(e) => {
                                let msg = figure_8::v8::String::new(scope, &e.to_string()).unwrap();
                                scope.throw_exception(figure_8::v8::Exception::error(scope, msg));
                            }
                        }
                    }),
                )
            }
        }
    };

    Ok(output)
}

fn parse_description(attr: proc_macro2::TokenStream) -> Result<String, syn::Error> {
    if attr.is_empty() {
        return Ok(String::new());
    }
    let meta: Meta = parse2(attr)?;
    if let Meta::NameValue(nv) = &meta
        && nv.path.is_ident("description")
        && let Expr::Lit(expr_lit) = &nv.value
        && let syn::Lit::Str(s) = &expr_lit.lit
    {
        return Ok(s.value());
    }
    Ok(String::new())
}

fn extract_params(func: &ItemFn) -> Result<Vec<(String, Box<syn::Type>)>, syn::Error> {
    func.sig
        .inputs
        .iter()
        .filter_map(|arg| {
            if let syn::FnArg::Typed(pat_type) = arg {
                let name = if let syn::Pat::Ident(ident) = &*pat_type.pat {
                    ident.ident.to_string()
                } else {
                    return Some(Err(syn::Error::new_spanned(
                        &pat_type.pat,
                        "expected a simple identifier pattern",
                    )));
                };
                Some(Ok((name, pat_type.ty.clone())))
            } else {
                None
            }
        })
        .collect()
}

fn extract_return_type(output: &syn::ReturnType) -> proc_macro2::TokenStream {
    match output {
        syn::ReturnType::Default => quote! { () },
        syn::ReturnType::Type(_, ty) => {
            if let syn::Type::Path(type_path) = &**ty
                && let Some(segment) = type_path.path.segments.last()
                && segment.ident == "Result"
                && let syn::PathArguments::AngleBracketed(args) = &segment.arguments
                && let Some(syn::GenericArgument::Type(inner)) = args.args.first()
            {
                return quote! { #inner };
            }
            quote! { #ty }
        }
    }
}
