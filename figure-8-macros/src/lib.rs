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
    let tool_def_name = syn::Ident::new(&format!("{}_tool_def", fn_name), fn_name.span());
    let is_async = func.sig.asyncness.is_some();
    let fn_name_lit = LitStr::new(&fn_name.to_string(), fn_name.span());
    let desc_lit = LitStr::new(&description, Span::call_site());

    let params = extract_params(&func)?;
    let ret_type = extract_return_type(&func.sig.output);

    let param_idents: Vec<syn::Ident> = params
        .iter()
        .map(|(name, _)| syn::Ident::new(name, Span::call_site()))
        .collect();
    let param_types: Vec<&Box<syn::Type>> = params.iter().map(|(_, ty)| ty).collect();

    let output = if is_async {
        quote! {
            #func

            pub fn #tool_def_name() -> figure_8::ToolDef {
                figure_8::tool_async!(#fn_name_lit, #desc_lit,
                    |#(#param_idents : #param_types),*| -> #ret_type {
                        #fn_name(#(#param_idents),*)
                    }
                )
            }
        }
    } else {
        quote! {
            #func

            pub fn #tool_def_name() -> figure_8::ToolDef {
                figure_8::tool_sync!(#fn_name_lit, #desc_lit,
                    |#(#param_idents : #param_types),*| -> #ret_type {
                        #fn_name(#(#param_idents),*)
                    }
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
