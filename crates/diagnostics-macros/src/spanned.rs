use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::{format_ident, quote};
use syn::{
    Attribute, Data, DeriveInput, Fields, Generics, Ident, Index, Member, Path, Type, parenthesized,
};

use super::{Cardinality, duplicate, members_equal, resolve_runtime_path, type_shape};

pub fn expand_spanned(input: &DeriveInput) -> syn::Result<TokenStream2> {
    let name = &input.ident;
    let mut cases = Vec::new();
    let runtime_override;
    let enum_type;
    match &input.data {
        Data::Struct(data) => {
            let args = parse_spanned_args(&input.attrs, true)?;
            runtime_override = args.runtime.clone();
            let (fields_style, fields) = field_specs(&data.fields)?;
            cases.push(CaseSpec {
                variant: None,
                fields_style,
                fields,
                args,
            });
            enum_type = false;
        }
        Data::Enum(data) => {
            let container = parse_spanned_args(&input.attrs, true)?;
            if container.transparent || container.forward.is_some() {
                return Err(syn::Error::new_spanned(
                    input,
                    "enum spanned metadata belongs on each variant; only `crate` is type-level",
                ));
            }
            runtime_override = container.runtime;
            for variant in &data.variants {
                let args = parse_spanned_args(&variant.attrs, false)?;
                let (fields_style, fields) = field_specs(&variant.fields)?;
                cases.push(CaseSpec {
                    variant: Some(variant.ident.clone()),
                    fields_style,
                    fields,
                    args,
                });
            }
            enum_type = true;
        }
        Data::Union(_) => {
            return Err(syn::Error::new_spanned(input, "Spanned cannot be derived for unions"));
        }
    }
    for case in &cases {
        validate_case(case)?;
    }

    let runtime = resolve_runtime_path(runtime_override)?;
    let mut generics = input.generics.clone();
    add_required_bounds(&mut generics, &cases, &runtime)?;
    let (impl_generics, type_generics, where_clause) = generics.split_for_impl();

    let span_body = method_body(&cases, enum_type, &runtime, span_body);

    Ok(quote! {
        impl #impl_generics #runtime::Spanned for #name #type_generics #where_clause {
            fn span(&self) -> #runtime::SourceSpan {
                #span_body
            }
        }
    })
}

#[derive(Default)]
struct SpannedArgs {
    runtime: Option<Path>,
    transparent: bool,
    forward: Option<Member>,
}

#[derive(Default)]
struct FieldRoles {
    span: bool,
}

struct FieldSpec {
    member: Member,
    binding: Ident,
    ty: Type,
    shape: Cardinality,
    element_ty: Type,
    boxed: bool,
    roles: FieldRoles,
}

struct CaseSpec {
    variant: Option<Ident>,
    fields_style: FieldStyle,
    fields: Vec<FieldSpec>,
    args: SpannedArgs,
}

#[derive(Clone, Copy)]
enum FieldStyle {
    Named,
    Unnamed,
    Unit,
}

fn parse_spanned_args(attributes: &[Attribute], allow_runtime: bool) -> syn::Result<SpannedArgs> {
    let mut args = SpannedArgs::default();
    for attribute in attributes.iter().filter(|attribute| attribute.path().is_ident("spanned")) {
        attribute.parse_nested_meta(|meta| {
            if meta.path.is_ident("crate") {
                if !allow_runtime {
                    return Err(meta.error("`crate` is allowed only on the derived type"));
                }
                let value: Path = meta.value()?.parse()?;
                return duplicate(&mut args.runtime, value, meta.path, "crate");
            }
            if meta.path.is_ident("transparent") {
                if args.transparent {
                    return Err(meta.error("duplicate spanned `transparent` argument"));
                }
                args.transparent = true;
                return Ok(());
            }
            if meta.path.is_ident("forward") {
                if args.forward.is_some() {
                    return Err(meta.error("duplicate spanned `forward` argument"));
                }
                let content;
                parenthesized!(content in meta.input);
                args.forward = Some(content.parse()?);
                return Ok(());
            }
            Err(meta.error("unsupported diagnostic argument"))
        })?;
    }
    Ok(args)
}

fn parse_field_roles(attributes: &[Attribute]) -> syn::Result<FieldRoles> {
    let mut roles = FieldRoles::default();
    for attribute in attributes {
        let path = attribute.path();
        if path.is_ident("span") {
            if roles.span {
                return Err(syn::Error::new_spanned(attribute, "duplicate `span` role"));
            }
            roles.span = true;
        }
    }
    Ok(roles)
}

fn field_specs(fields: &Fields) -> syn::Result<(FieldStyle, Vec<FieldSpec>)> {
    let (style, fields) = match fields {
        Fields::Named(fields) => (FieldStyle::Named, fields.named.iter().collect::<Vec<_>>()),
        Fields::Unnamed(fields) => (FieldStyle::Unnamed, fields.unnamed.iter().collect::<Vec<_>>()),
        Fields::Unit => (FieldStyle::Unit, Vec::new()),
    };
    let mut specs = Vec::new();
    for (position, field) in fields.into_iter().enumerate() {
        let member = field
            .ident
            .clone()
            .map(Member::Named)
            .unwrap_or_else(|| Member::Unnamed(Index::from(position)));
        let binding = format_ident!("__miden_spanned_field_{position}");
        let shape = type_shape(&field.ty);
        let roles = parse_field_roles(&field.attrs)?;
        if roles.span && matches!(shape.cardinality, Cardinality::Many | Cardinality::OptionalMany)
        {
            return Err(syn::Error::new_spanned(field, "a span field cannot be a collection"));
        }
        specs.push(FieldSpec {
            member,
            binding,
            ty: field.ty.clone(),
            shape: shape.cardinality,
            element_ty: shape.element.clone(),
            boxed: shape.boxed,
            roles,
        });
    }
    Ok((style, specs))
}

fn add_required_bounds(
    generics: &mut Generics,
    cases: &[CaseSpec],
    runtime: &TokenStream2,
) -> syn::Result<()> {
    let mut predicates = Vec::<syn::WherePredicate>::new();
    for case in cases {
        for field in &case.fields {
            let ty = &field.element_ty;
            if field.roles.span
                || case
                    .args
                    .forward
                    .as_ref()
                    .is_some_and(|member| members_equal(member, &field.member))
                || (case.args.transparent && case.fields.len() == 1)
            {
                predicates.push(syn::parse2(quote!(#ty: #runtime::Spanned))?);
            }
        }
    }
    generics.make_where_clause().predicates.extend(predicates);
    Ok(())
}

fn validate_case(case: &CaseSpec) -> syn::Result<()> {
    if case.args.transparent && case.args.forward.is_some() {
        return Err(syn::Error::new(
            Span::call_site(),
            "`transparent` and `forward(field)` are mutually exclusive",
        ));
    }

    if case.args.transparent || case.args.forward.is_some() {
        let target = forwarding_field(case)?;
        if !matches!(target.shape, Cardinality::One) {
            return Err(syn::Error::new_spanned(
                &target.ty,
                "a forwarding target must be singular and non-optional",
            ));
        }
        if case.fields.iter().any(|field| field.roles.span) {
            return Err(syn::Error::new(
                Span::call_site(),
                "a forwarding case cannot also declare a `span` field",
            ));
        }
        return Ok(());
    }

    let span_fields = case.fields.iter().filter(|field| field.roles.span).collect::<Vec<_>>();
    if span_fields.is_empty() {
        return Err(syn::Error::new(
            Span::call_site(),
            "at least one field must be annotated with #[span]",
        ));
    }
    if span_fields.len() > 1 {
        return Err(syn::Error::new_spanned(
            &span_fields[1].ty,
            "at most one field can be annotated with #[span]",
        ));
    }
    let span_field = span_fields[0];
    if !matches!(span_field.shape, Cardinality::One) {
        return Err(syn::Error::new_spanned(
            &span_field.ty,
            "a span field must be singular and non-optional",
        ));
    }
    Ok(())
}

fn forwarding_field(case: &CaseSpec) -> syn::Result<&FieldSpec> {
    if case.args.transparent {
        if case.fields.len() != 1 {
            return Err(syn::Error::new(
                Span::call_site(),
                "`transparent` requires exactly one field",
            ));
        }
        return Ok(&case.fields[0]);
    }
    let member = case.args.forward.as_ref().expect("forwarding was checked");
    case.fields
        .iter()
        .find(|field| members_equal(&field.member, member))
        .ok_or_else(|| syn::Error::new_spanned(member, "forwarding field does not exist"))
}

fn case_forward_target(case: &CaseSpec, _runtime: &TokenStream2) -> Option<TokenStream2> {
    if !(case.args.transparent || case.args.forward.is_some()) {
        return None;
    }
    let field = forwarding_field(case).expect("forwarding was validated");
    let binding = &field.binding;
    if field.boxed {
        Some(quote!(&**#binding))
    } else {
        Some(quote!(#binding))
    }
}

fn method_body(
    cases: &[CaseSpec],
    enum_type: bool,
    runtime: &TokenStream2,
    body: impl Fn(&CaseSpec, &TokenStream2) -> TokenStream2,
) -> TokenStream2 {
    if enum_type {
        let arms = cases.iter().map(|case| wrap_case(case, true, body(case, runtime)));
        quote!(match self { #(#arms),* })
    } else {
        wrap_case(&cases[0], false, body(&cases[0], runtime))
    }
}

fn case_pattern(case: &CaseSpec, enum_case: bool) -> TokenStream2 {
    let prefix = case
        .variant
        .as_ref()
        .map_or_else(|| quote!(Self), |variant| quote!(Self::#variant));
    match case.fields_style {
        FieldStyle::Named => {
            let fields = case.fields.iter().map(|field| {
                let Member::Named(member) = &field.member else {
                    unreachable!()
                };
                let binding = &field.binding;
                quote!(#member: #binding)
            });
            if enum_case {
                quote!(#prefix { #(#fields),* })
            } else {
                quote!(let #prefix { #(#fields),* } = self;)
            }
        }
        FieldStyle::Unnamed => {
            let fields = case.fields.iter().map(|field| &field.binding);
            if enum_case {
                quote!(#prefix(#(#fields),*))
            } else {
                quote!(let #prefix(#(#fields),*) = self;)
            }
        }
        FieldStyle::Unit => {
            if enum_case {
                quote!(#prefix)
            } else {
                quote!(let #prefix = self;)
            }
        }
    }
}

fn aliases(case: &CaseSpec) -> TokenStream2 {
    let aliases = case.fields.iter().map(|field| {
        let binding = &field.binding;
        let alias = match &field.member {
            Member::Named(ident) => ident.clone(),
            Member::Unnamed(index) => format_ident!("_{}", index.index),
        };
        quote!(#[allow(unused_variables)] let #alias = #binding;)
    });
    quote!(#(#aliases)*)
}

fn wrap_case(case: &CaseSpec, enum_case: bool, body: TokenStream2) -> TokenStream2 {
    let pattern = case_pattern(case, enum_case);
    let aliases = aliases(case);
    if enum_case {
        quote!(#pattern => { #aliases #body })
    } else {
        quote!({ #pattern #aliases #body })
    }
}

fn span_body(case: &CaseSpec, runtime: &TokenStream2) -> TokenStream2 {
    if let Some(target) = case_forward_target(case, runtime) {
        quote!(#runtime::Spanned::span(#target))
    } else {
        let field = case
            .fields
            .iter()
            .find(|field| field.roles.span)
            .expect("span field was validated");
        let alias = match &field.member {
            Member::Named(ident) => ident.clone(),
            Member::Unnamed(index) => format_ident!("_{}", index.index),
        };
        quote!(#runtime::Spanned::span(#alias))
    }
}

#[cfg(test)]
mod tests {
    use quote::quote;
    use syn::DeriveInput;

    #[test]
    fn generated_spanned_impl_test() {
        let input: DeriveInput = syn::parse2(quote! {
            #[derive(Spanned)]
            #[spanned(
                crate = runtime_api,
            )]
            struct Example<T> {
                #[span]
                value: T,
            }
        })
        .unwrap();

        let expanded = super::expand_spanned(&input).unwrap().to_string();
        assert!(expanded.contains("runtime_api :: Spanned for Example < T >"));
        assert!(expanded.contains("T : runtime_api :: Spanned"));
        assert!(expanded.contains("runtime_api :: Spanned :: span (value)"));
    }

    #[test]
    fn complete_forwarding_delegates_every_protocol_method() {
        let input: DeriveInput = syn::parse2(quote! {
            #[derive(Debug, Spanned)]
            #[spanned(crate = runtime_api, forward(inner))]
            struct Wrapper<T> {
                context: u8,
                inner: T,
            }
        })
        .unwrap();

        let expanded = super::expand_spanned(&input).unwrap().to_string();
        assert!(expanded.contains("T : runtime_api :: Spanned"));
        assert!(expanded.contains("runtime_api :: Spanned :: span (__miden_spanned_field_1)"));
    }
}
