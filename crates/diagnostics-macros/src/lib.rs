mod diagnostic;
mod spanned;

use proc_macro::TokenStream;
use proc_macro_crate::{FoundCrate, crate_name};
use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::quote;
use syn::{
    DeriveInput, GenericArgument, Ident, Member, Path, PathArguments, Type, parse_macro_input,
};

#[proc_macro_derive(
    Diagnostic,
    attributes(
        diagnostic,
        diagnostic_source,
        help,
        label,
        labels,
        note,
        related,
        source,
        suggestion,
        suggestions
    )
)]
pub fn derive_diagnostic(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    diagnostic::expand_diagnostic(&input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

#[proc_macro_derive(Spanned, attributes(span, spanned))]
pub fn derive_spanned(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    spanned::expand_spanned(&input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

pub(crate) fn duplicate<T>(
    slot: &mut Option<T>,
    value: T,
    span: impl quote::ToTokens,
    name: &str,
) -> syn::Result<()> {
    if slot.is_some() {
        return Err(syn::Error::new_spanned(
            span,
            format!("duplicate diagnostic `{name}` argument"),
        ));
    }
    *slot = Some(value);
    Ok(())
}

pub(crate) fn resolve_runtime_path(explicit: Option<Path>) -> syn::Result<TokenStream2> {
    if let Some(path) = explicit {
        return Ok(quote!(#path));
    }
    match crate_name("miden-diagnostics") {
        Ok(FoundCrate::Itself) => {
            // When compiling examples, `crate::` refers to the example crate, not miden-diagnostics,
            // but crate_name returns Itself because the example shares its Cargo.toml with the
            // miden-diagnostics crate. We check these env vars to distinguish when we can actually
            // use `crate` to refer to `miden-diagnostics` and when we can't
            let is_example_or_integration_test = std::env::var("CARGO_PKG_NAME")
                .is_ok_and(|name| name == "miden-diagnostics")
                && std::env::var("CARGO_CRATE_NAME").is_ok_and(|name| name != "miden-diagnostics");
            if is_example_or_integration_test {
                Ok(quote!(::miden_diagnostics))
            } else {
                Ok(quote!(crate))
            }
        }
        Ok(FoundCrate::Name(name)) => {
            let name = Ident::new(&name, Span::call_site());
            Ok(quote!(::#name))
        }
        Err(error) => Err(syn::Error::new(
            Span::call_site(),
            format!("cannot locate `miden-diagnostics`: {error}"),
        )),
    }
}

pub(crate) fn type_argument<'a>(ty: &'a Type, wrapper: &str) -> Option<&'a Type> {
    let Type::Path(path) = ty else {
        return None;
    };
    let segment = path.path.segments.last()?;
    if segment.ident != wrapper {
        return None;
    }
    let PathArguments::AngleBracketed(arguments) = &segment.arguments else {
        return None;
    };
    if arguments.args.len() != 1 {
        return None;
    }
    match arguments.args.first()? {
        GenericArgument::Type(ty) => Some(ty),
        _ => None,
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) enum Cardinality {
    One,
    Optional,
    Many,
    OptionalMany,
}

pub(crate) struct TypeShape<'a> {
    pub cardinality: Cardinality,
    pub element: &'a Type,
    pub boxed: bool,
}

pub(crate) fn type_shape(ty: &Type) -> TypeShape<'_> {
    if let Some(inner) = type_argument(ty, "Option") {
        if let Some(element) = type_argument(inner, "Vec") {
            return TypeShape {
                cardinality: Cardinality::OptionalMany,
                element,
                boxed: false,
            };
        }
        let boxed_element = type_argument(inner, "Box");
        return TypeShape {
            cardinality: Cardinality::Optional,
            element: boxed_element.unwrap_or(inner),
            boxed: boxed_element.is_some(),
        };
    }
    if let Some(element) = type_argument(ty, "Vec") {
        return TypeShape {
            cardinality: Cardinality::Many,
            element,
            boxed: false,
        };
    }
    TypeShape {
        cardinality: Cardinality::One,
        element: type_argument(ty, "Box").unwrap_or(ty),
        boxed: type_argument(ty, "Box").is_some(),
    }
}

pub(crate) fn members_equal(left: &Member, right: &Member) -> bool {
    match (left, right) {
        (Member::Named(left), Member::Named(right)) => left == right,
        (Member::Unnamed(left), Member::Unnamed(right)) => left.index == right.index,
        _ => false,
    }
}
