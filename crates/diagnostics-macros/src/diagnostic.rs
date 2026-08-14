use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::{format_ident, quote};
use syn::{
    Attribute, Data, DeriveInput, Expr, ExprArray, Fields, Generics, Ident, Index, LitStr, Member,
    Path, Type, parenthesized,
};

use super::{Cardinality, duplicate, members_equal, resolve_runtime_path, type_shape};

pub fn expand_diagnostic(input: &DeriveInput) -> syn::Result<TokenStream2> {
    let name = &input.ident;
    let mut cases = Vec::new();
    let runtime_override;
    let enum_type;
    match &input.data {
        Data::Struct(data) => {
            let args = parse_diagnostic_args(&input.attrs, true)?;
            runtime_override = args.runtime.clone();
            let (fields_style, fields) = field_specs(&data.fields)?;
            cases.push(CaseSpec {
                variant: None,
                fields_style,
                fields,
                args,
                descriptor_ident: None,
            });
            enum_type = false;
        }
        Data::Enum(data) => {
            let container = parse_diagnostic_args(&input.attrs, true)?;
            if container.descriptor.is_some()
                || container.code.is_some()
                || container.summary.is_some()
                || container.severity.is_some()
                || container.explanation.is_some()
                || container.documentation_url.is_some()
                || !container.tags.is_empty()
                || container.message.is_some()
                || container.help.is_some()
                || container.transparent
                || container.forward.is_some()
            {
                return Err(syn::Error::new_spanned(
                    input,
                    "enum diagnostic metadata belongs on each variant; only `crate` is type-level",
                ));
            }
            runtime_override = container.runtime;
            for variant in &data.variants {
                let args = parse_diagnostic_args(&variant.attrs, false)?;
                let (fields_style, fields) = field_specs(&variant.fields)?;
                cases.push(CaseSpec {
                    variant: Some(variant.ident.clone()),
                    fields_style,
                    fields,
                    args,
                    descriptor_ident: None,
                });
            }
            enum_type = true;
        }
        Data::Union(_) => {
            return Err(syn::Error::new_spanned(input, "Diagnostic cannot be derived for unions"));
        }
    }
    for (index, case) in cases.iter_mut().enumerate() {
        if case.args.code.is_some() {
            case.descriptor_ident = Some(format_ident!("__MIDEN_DIAGNOSTIC_DESCRIPTOR_{index}"));
        }
        validate_case(case)?;
    }

    let runtime = resolve_runtime_path(runtime_override)?;
    let descriptors = cases.iter().map(|case| descriptor_static(case, &runtime));
    let mut generics = input.generics.clone();
    add_required_bounds(&mut generics, name, &cases, &runtime)?;
    let (impl_generics, type_generics, where_clause) = generics.split_for_impl();

    let message = method_body(&cases, enum_type, &runtime, message_body);
    let descriptor = method_body(&cases, enum_type, &runtime, descriptor_body);
    let code = method_body(&cases, enum_type, &runtime, code_body);
    let severity = method_body(&cases, enum_type, &runtime, severity_body);
    let tags = method_body(&cases, enum_type, &runtime, tags_body);
    let visit = method_body(&cases, enum_type, &runtime, visit_body);
    let cause = method_body(&cases, enum_type, &runtime, cause_body);
    let diagnostic_source = method_body(&cases, enum_type, &runtime, diagnostic_source_body);

    Ok(quote! {
        const _: () = {
            #(#descriptors)*

            impl #impl_generics #runtime::Diagnostic for #name #type_generics #where_clause {
                fn message(
                    &self,
                    __miden_diagnostics_out: &mut dyn core::fmt::Write,
                ) -> core::fmt::Result {
                    #message
                }

                fn descriptor(&self) -> Option<&'static #runtime::DiagnosticDescriptor> {
                    #descriptor
                }

                fn code(&self) -> Option<#runtime::DiagnosticCodeRef<'_>> {
                    #code
                }

                fn severity(&self) -> #runtime::Severity {
                    #severity
                }

                fn tags(&self) -> &[#runtime::DiagnosticTag] {
                    #tags
                }

                fn visit(&self, __miden_diagnostics_visitor: &mut dyn #runtime::VisitDiagnostic) {
                    #visit
                }

                fn cause(&self) -> Option<&(dyn core::error::Error + 'static)> {
                    #cause
                }

                fn diagnostic_source(&self) -> Option<&dyn #runtime::Diagnostic> {
                    #diagnostic_source
                }
            }
        };
    })
}

#[derive(Default)]
struct DiagnosticArgs {
    runtime: Option<Path>,
    descriptor: Option<Path>,
    code: Option<LitStr>,
    summary: Option<LitStr>,
    severity: Option<Ident>,
    explanation: Option<Expr>,
    documentation_url: Option<LitStr>,
    tags: Vec<Ident>,
    message: Option<LitStr>,
    help: Option<LitStr>,
    transparent: bool,
    forward: Option<Member>,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum LabelKind {
    Primary,
    Context,
}

struct LabelAttr {
    kind: LabelKind,
    message: Option<LitStr>,
}

struct SuggestionAttr {
    message: LitStr,
    replacement: LitStr,
    applicability: Ident,
}

#[derive(Default)]
struct FieldRoles {
    label: Option<LabelAttr>,
    labels: bool,
    note: bool,
    help: bool,
    suggestion: Option<SuggestionAttr>,
    suggestions: bool,
    related: bool,
    source: bool,
    diagnostic_source: bool,
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
    args: DiagnosticArgs,
    descriptor_ident: Option<Ident>,
}

#[derive(Clone, Copy)]
enum FieldStyle {
    Named,
    Unnamed,
    Unit,
}

fn parse_diagnostic_args(
    attributes: &[Attribute],
    allow_runtime: bool,
) -> syn::Result<DiagnosticArgs> {
    let mut args = DiagnosticArgs::default();
    for attribute in attributes.iter().filter(|attribute| attribute.path().is_ident("diagnostic")) {
        attribute.parse_nested_meta(|meta| {
            if meta.path.is_ident("crate") {
                if !allow_runtime {
                    return Err(meta.error("`crate` is allowed only on the derived type"));
                }
                let value: Path = meta.value()?.parse()?;
                return duplicate(&mut args.runtime, value, meta.path, "crate");
            }
            if meta.path.is_ident("descriptor") {
                let value: Path = meta.value()?.parse()?;
                return duplicate(&mut args.descriptor, value, meta.path, "descriptor");
            }
            if meta.path.is_ident("code") {
                let value: LitStr = meta.value()?.parse()?;
                return duplicate(&mut args.code, value, meta.path, "code");
            }
            if meta.path.is_ident("summary") {
                let value: LitStr = meta.value()?.parse()?;
                return duplicate(&mut args.summary, value, meta.path, "summary");
            }
            if meta.path.is_ident("severity") {
                let value: Ident = meta.value()?.parse()?;
                return duplicate(&mut args.severity, value, meta.path, "severity");
            }
            if meta.path.is_ident("explanation") {
                let value: Expr = meta.value()?.parse()?;
                return duplicate(&mut args.explanation, value, meta.path, "explanation");
            }
            if meta.path.is_ident("documentation_url") {
                let value: LitStr = meta.value()?.parse()?;
                return duplicate(
                    &mut args.documentation_url,
                    value,
                    meta.path,
                    "documentation_url",
                );
            }
            if meta.path.is_ident("tags") {
                if !args.tags.is_empty() {
                    return Err(meta.error("duplicate diagnostic `tags` argument"));
                }
                let values: ExprArray = meta.value()?.parse()?;
                for value in values.elems {
                    let Expr::Path(value) = value else {
                        return Err(syn::Error::new_spanned(
                            value,
                            "diagnostic tags must be identifiers",
                        ));
                    };
                    let Some(ident) = value.path.get_ident() else {
                        return Err(syn::Error::new_spanned(
                            value,
                            "diagnostic tags must be unqualified identifiers",
                        ));
                    };
                    validate_tag(ident)?;
                    args.tags.push(ident.clone());
                }
                return Ok(());
            }
            if meta.path.is_ident("message") {
                let value: LitStr = meta.value()?.parse()?;
                return duplicate(&mut args.message, value, meta.path, "message");
            }
            if meta.path.is_ident("help") {
                let value: LitStr = meta.value()?.parse()?;
                return duplicate(&mut args.help, value, meta.path, "help");
            }
            if meta.path.is_ident("transparent") {
                if args.transparent {
                    return Err(meta.error("duplicate diagnostic `transparent` argument"));
                }
                args.transparent = true;
                return Ok(());
            }
            if meta.path.is_ident("forward") {
                if args.forward.is_some() {
                    return Err(meta.error("duplicate diagnostic `forward` argument"));
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

fn validate_severity(severity: &Ident) -> syn::Result<()> {
    match severity.to_string().as_str() {
        "Error" | "Warning" | "Info" | "Hint" => Ok(()),
        _ => Err(syn::Error::new_spanned(
            severity,
            "severity must be one of Error, Warning, Info, or Hint",
        )),
    }
}

fn validate_tag(tag: &Ident) -> syn::Result<()> {
    match tag.to_string().as_str() {
        "Unnecessary" | "Deprecated" => Ok(()),
        _ => Err(syn::Error::new_spanned(tag, "tag must be Unnecessary or Deprecated")),
    }
}

fn validate_applicability(applicability: &Ident) -> syn::Result<()> {
    match applicability.to_string().as_str() {
        "MachineApplicable" | "MaybeIncorrect" | "HasPlaceholders" | "Unspecified" => Ok(()),
        _ => Err(syn::Error::new_spanned(applicability, "unsupported suggestion applicability")),
    }
}

fn parse_label(attribute: &Attribute) -> syn::Result<LabelAttr> {
    if matches!(attribute.meta, syn::Meta::Path(_)) {
        return Ok(LabelAttr {
            kind: LabelKind::Context,
            message: None,
        });
    }
    let values = attribute
        .parse_args_with(syn::punctuated::Punctuated::<Expr, syn::Token![,]>::parse_terminated)?;
    let mut values = values.into_iter();
    let Some(first) = values.next() else {
        return Ok(LabelAttr {
            kind: LabelKind::Context,
            message: None,
        });
    };
    let (kind, message) = match first {
        Expr::Path(path) if path.path.is_ident("primary") => {
            let message = values.next().map(expect_string).transpose()?;
            (LabelKind::Primary, message)
        }
        value => (LabelKind::Context, Some(expect_string(value)?)),
    };
    if let Some(extra) = values.next() {
        return Err(syn::Error::new_spanned(
            extra,
            "label accepts only `primary` and one optional message",
        ));
    }
    Ok(LabelAttr { kind, message })
}

fn expect_string(expression: Expr) -> syn::Result<LitStr> {
    let Expr::Lit(expression) = expression else {
        return Err(syn::Error::new_spanned(expression, "expected a string literal"));
    };
    let syn::Lit::Str(value) = expression.lit else {
        return Err(syn::Error::new_spanned(expression, "expected a string literal"));
    };
    Ok(value)
}

impl syn::parse::Parse for SuggestionAttr {
    fn parse(input: syn::parse::ParseStream<'_>) -> syn::Result<Self> {
        let message: LitStr = input.parse()?;
        input.parse::<syn::Token![,]>()?;
        let replacement_key: Ident = input.parse()?;
        if replacement_key != "replacement" {
            return Err(syn::Error::new_spanned(replacement_key, "expected `replacement`"));
        }
        input.parse::<syn::Token![=]>()?;
        let replacement: LitStr = input.parse()?;
        input.parse::<syn::Token![,]>()?;
        let applicability_key: Ident = input.parse()?;
        if applicability_key != "applicability" {
            return Err(syn::Error::new_spanned(applicability_key, "expected `applicability`"));
        }
        input.parse::<syn::Token![=]>()?;
        let applicability: Ident = input.parse()?;
        validate_applicability(&applicability)?;
        if input.peek(syn::Token![,]) {
            input.parse::<syn::Token![,]>()?;
        }
        if !input.is_empty() {
            return Err(input.error("unexpected suggestion argument"));
        }
        Ok(Self {
            message,
            replacement,
            applicability,
        })
    }
}

fn parse_field_roles(attributes: &[Attribute]) -> syn::Result<FieldRoles> {
    let mut roles = FieldRoles::default();
    for attribute in attributes {
        let path = attribute.path();
        if path.is_ident("label") {
            if roles.label.is_some() {
                return Err(syn::Error::new_spanned(attribute, "duplicate `label` role"));
            }
            roles.label = Some(parse_label(attribute)?);
        } else if path.is_ident("labels") {
            ensure_bare(attribute)?;
            if roles.labels {
                return Err(syn::Error::new_spanned(attribute, "duplicate `labels` role"));
            }
            roles.labels = true;
        } else if path.is_ident("note") {
            ensure_bare(attribute)?;
            if roles.note {
                return Err(syn::Error::new_spanned(attribute, "duplicate `note` role"));
            }
            roles.note = true;
        } else if path.is_ident("help") {
            ensure_bare(attribute)?;
            if roles.help {
                return Err(syn::Error::new_spanned(attribute, "duplicate `help` role"));
            }
            roles.help = true;
        } else if path.is_ident("suggestion") {
            if roles.suggestion.is_some() {
                return Err(syn::Error::new_spanned(attribute, "duplicate `suggestion` role"));
            }
            roles.suggestion = Some(attribute.parse_args()?);
        } else if path.is_ident("suggestions") {
            ensure_bare(attribute)?;
            if roles.suggestions {
                return Err(syn::Error::new_spanned(attribute, "duplicate `suggestions` role"));
            }
            roles.suggestions = true;
        } else if path.is_ident("related") {
            ensure_bare(attribute)?;
            if roles.related {
                return Err(syn::Error::new_spanned(attribute, "duplicate `related` role"));
            }
            roles.related = true;
        } else if path.is_ident("source") {
            ensure_bare(attribute)?;
            if roles.source {
                return Err(syn::Error::new_spanned(attribute, "duplicate `source` role"));
            }
            roles.source = true;
        } else if path.is_ident("diagnostic_source") {
            ensure_bare(attribute)?;
            if roles.diagnostic_source {
                return Err(syn::Error::new_spanned(
                    attribute,
                    "duplicate `diagnostic_source` role",
                ));
            }
            roles.diagnostic_source = true;
        }
    }
    let exclusive_roles = [
        roles.label.is_some(),
        roles.labels,
        roles.note,
        roles.help,
        roles.suggestion.is_some(),
        roles.suggestions,
        roles.related,
    ]
    .into_iter()
    .filter(|present| *present)
    .count();
    if exclusive_roles > 1 {
        return Err(syn::Error::new(
            Span::call_site(),
            "a field cannot have multiple diagnostic visitor roles",
        ));
    }
    if (roles.source || roles.diagnostic_source) && exclusive_roles != 0 {
        return Err(syn::Error::new(
            Span::call_site(),
            "`source` roles cannot be combined with visitor roles",
        ));
    }
    Ok(roles)
}

fn ensure_bare(attribute: &Attribute) -> syn::Result<()> {
    match &attribute.meta {
        syn::Meta::Path(_) => Ok(()),
        _ => Err(syn::Error::new_spanned(
            attribute,
            "this diagnostic field role takes no arguments",
        )),
    }
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
        let binding = format_ident!("__miden_diagnostics_field_{position}");
        let shape = type_shape(&field.ty);
        let roles = parse_field_roles(&field.attrs)?;
        if roles.label.as_ref().is_some_and(|label| label.kind == LabelKind::Primary)
            && matches!(shape.cardinality, Cardinality::Many | Cardinality::OptionalMany)
        {
            return Err(syn::Error::new_spanned(
                field,
                "a primary label field cannot be a collection",
            ));
        }
        if roles.source
            && matches!(shape.cardinality, Cardinality::Many | Cardinality::OptionalMany)
        {
            return Err(syn::Error::new_spanned(
                field,
                "a conventional source cannot be a collection",
            ));
        }
        if roles.diagnostic_source
            && matches!(shape.cardinality, Cardinality::Many | Cardinality::OptionalMany)
        {
            return Err(syn::Error::new_spanned(
                field,
                "a diagnostic source cannot be a collection",
            ));
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

fn validate_case(case: &CaseSpec) -> syn::Result<()> {
    let args = &case.args;
    if let Some(severity) = &args.severity {
        validate_severity(severity)?;
    }
    if args.descriptor.is_some()
        && (args.code.is_some()
            || args.summary.is_some()
            || args.explanation.is_some()
            || args.documentation_url.is_some()
            || !args.tags.is_empty())
    {
        return Err(syn::Error::new(
            Span::call_site(),
            "`descriptor` cannot be combined with inline descriptor metadata",
        ));
    }
    if let Some(code) = &args.code {
        let canonical = code.value();
        let Some((namespace, code_part)) = canonical.split_once('/') else {
            return Err(syn::Error::new_spanned(code, "diagnostic code must be `namespace/code`"));
        };
        if namespace.is_empty() || code_part.is_empty() || code_part.contains('/') {
            return Err(syn::Error::new_spanned(
                code,
                "diagnostic code must contain exactly one nonempty `/` separator",
            ));
        }
        if args.summary.is_none() {
            return Err(syn::Error::new_spanned(
                code,
                "an inline diagnostic descriptor requires `summary`",
            ));
        }
    } else if args.summary.is_some()
        || args.explanation.is_some()
        || args.documentation_url.is_some()
        || !args.tags.is_empty()
    {
        return Err(syn::Error::new(
            Span::call_site(),
            "inline descriptor metadata requires `code`",
        ));
    }
    let primary_fields = case
        .fields
        .iter()
        .filter(|field| {
            field.roles.label.as_ref().is_some_and(|label| label.kind == LabelKind::Primary)
        })
        .collect::<Vec<_>>();
    if primary_fields.len() > 1 {
        let mut errors = syn::Error::new_spanned(
            &primary_fields[1].ty,
            "at most one statically declared primary label is allowed",
        );
        for field in primary_fields.iter().skip(2) {
            errors.combine(syn::Error::new_spanned(
                &field.ty,
                "additional primary label declared here",
            ));
        }
        return Err(errors);
    }
    if case.fields.iter().filter(|field| field.roles.source).count() > 1 {
        return Err(syn::Error::new(
            Span::call_site(),
            "at most one conventional `source` field is allowed",
        ));
    }
    if case.fields.iter().filter(|field| field.roles.diagnostic_source).count() > 1 {
        return Err(syn::Error::new(
            Span::call_site(),
            "at most one `diagnostic_source` field is allowed",
        ));
    }
    if args.transparent && args.forward.is_some() {
        return Err(syn::Error::new(
            Span::call_site(),
            "`transparent` and `forward(field)` are mutually exclusive",
        ));
    }
    if args.transparent || args.forward.is_some() {
        let target = forwarding_field(case)?;
        if !matches!(target.shape, Cardinality::One) {
            return Err(syn::Error::new_spanned(
                &target.ty,
                "a forwarding target must be singular and non-optional",
            ));
        }
        if args.descriptor.is_some()
            || args.code.is_some()
            || args.severity.is_some()
            || args.explanation.is_some()
            || args.documentation_url.is_some()
            || !args.tags.is_empty()
            || args.message.is_some()
            || args.help.is_some()
        {
            return Err(syn::Error::new(
                Span::call_site(),
                "a forwarding diagnostic cannot declare local metadata or messages",
            ));
        }
        for field in &case.fields {
            let roles = &field.roles;
            let documentary_target =
                core::ptr::eq(field, target) && roles.diagnostic_source && !roles.source;
            let any_role = roles.label.is_some()
                || roles.labels
                || roles.note
                || roles.help
                || roles.suggestion.is_some()
                || roles.suggestions
                || roles.related
                || roles.source
                || roles.diagnostic_source;
            if any_role && !documentary_target {
                return Err(syn::Error::new_spanned(
                    &field.ty,
                    "forwarding cases cannot declare local semantic field roles",
                ));
            }
        }
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

fn descriptor_body(case: &CaseSpec, runtime: &TokenStream2) -> TokenStream2 {
    if let Some(target) = case_forward_target(case, runtime) {
        return quote!(#runtime::Diagnostic::descriptor(#target));
    }
    if let Some(descriptor) = &case.args.descriptor {
        quote!(Some(&#descriptor))
    } else if let Some(descriptor) = &case.descriptor_ident {
        quote!(Some(&#descriptor))
    } else {
        quote!(None)
    }
}

fn code_body(case: &CaseSpec, runtime: &TokenStream2) -> TokenStream2 {
    if let Some(target) = case_forward_target(case, runtime) {
        quote!(#runtime::Diagnostic::code(#target))
    } else if case.args.descriptor.is_some() || case.descriptor_ident.is_some() {
        let descriptor = descriptor_body(case, runtime);
        quote!((#descriptor).map(|descriptor| descriptor.code.as_ref()))
    } else {
        quote!(None)
    }
}

fn severity_body(case: &CaseSpec, runtime: &TokenStream2) -> TokenStream2 {
    if let Some(target) = case_forward_target(case, runtime) {
        return quote!(#runtime::Diagnostic::severity(#target));
    }
    if let Some(severity) = &case.args.severity {
        return quote!(#runtime::Severity::#severity);
    }
    if case.args.descriptor.is_some() || case.descriptor_ident.is_some() {
        let descriptor = descriptor_body(case, runtime);
        quote!((#descriptor).map_or(#runtime::Severity::Error, |descriptor| descriptor.default_severity))
    } else {
        quote!(#runtime::Severity::Error)
    }
}

fn tags_body(case: &CaseSpec, runtime: &TokenStream2) -> TokenStream2 {
    if let Some(target) = case_forward_target(case, runtime) {
        quote!(#runtime::Diagnostic::tags(#target))
    } else if case.args.descriptor.is_some() || case.descriptor_ident.is_some() {
        let descriptor = descriptor_body(case, runtime);
        quote!((#descriptor).map_or(&[], |descriptor| descriptor.tags))
    } else {
        quote!(&[])
    }
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

fn message_body(case: &CaseSpec, runtime: &TokenStream2) -> TokenStream2 {
    if let Some(target) = case_forward_target(case, runtime) {
        return quote!(#runtime::Diagnostic::message(#target, __miden_diagnostics_out));
    }
    if let Some(message) = &case.args.message {
        quote!(core::write!(__miden_diagnostics_out, #message))
    } else {
        quote!(core::write!(__miden_diagnostics_out, "{}", self))
    }
}

fn emit_each(field: &FieldSpec, one: impl Fn(TokenStream2) -> TokenStream2) -> TokenStream2 {
    let binding = &field.binding;
    match field.shape {
        Cardinality::One => one(quote!(#binding)),
        Cardinality::Optional => {
            let body = one(quote!(__miden_diagnostics_value));
            quote!(if let Some(__miden_diagnostics_value) = #binding.as_ref() { #body })
        }
        Cardinality::Many => {
            let body = one(quote!(__miden_diagnostics_value));
            quote!(for __miden_diagnostics_value in #binding { #body })
        }
        Cardinality::OptionalMany => {
            let body = one(quote!(__miden_diagnostics_value));
            quote! {
                if let Some(__miden_diagnostics_values) = #binding.as_ref() {
                    for __miden_diagnostics_value in __miden_diagnostics_values {
                        #body
                    }
                }
            }
        }
    }
}

fn visit_body(case: &CaseSpec, runtime: &TokenStream2) -> TokenStream2 {
    if let Some(target) = case_forward_target(case, runtime) {
        return quote!(#runtime::Diagnostic::visit(#target, __miden_diagnostics_visitor));
    }
    let mut statements = Vec::new();
    for field in &case.fields {
        if let Some(label) = &field.roles.label {
            let style = match label.kind {
                LabelKind::Primary => quote!(#runtime::LabelStyle::Primary),
                LabelKind::Context => quote!(#runtime::LabelStyle::Context),
            };
            let message = label
                .message
                .as_ref()
                .map_or_else(|| quote!(None), |message| quote!(Some(format_args!(#message))));
            statements.push(emit_each(field, |span| {
                quote! {
                    __miden_diagnostics_visitor.label(#runtime::Label {
                        span: #runtime::Spanned::span(#span),
                        style: #style,
                        message: #message,
                    });
                }
            }));
        }
        if field.roles.labels {
            statements.push(emit_each(field, |label| {
                quote! {
                    match &#label.message {
                        Some(__miden_diagnostics_message) => {
                            __miden_diagnostics_visitor.label(#runtime::Label {
                                span: #label.span,
                                style: #label.style,
                                message: Some(format_args!("{}", __miden_diagnostics_message)),
                            });
                        }
                        None => {
                            __miden_diagnostics_visitor.label(#runtime::Label {
                                span: #label.span,
                                style: #label.style,
                                message: None,
                            });
                        }
                    }
                }
            }));
        }
        if field.roles.note || field.roles.help {
            let kind = if field.roles.note {
                quote!(#runtime::NoteKind::Note)
            } else {
                quote!(#runtime::NoteKind::Help)
            };
            statements.push(emit_each(field, |value| {
                quote! {
                    __miden_diagnostics_visitor.note(#runtime::Note {
                        kind: #kind,
                        message: format_args!("{}", #value),
                    });
                }
            }));
        }
        if let Some(suggestion) = &field.roles.suggestion {
            let message = &suggestion.message;
            let replacement = &suggestion.replacement;
            let applicability = &suggestion.applicability;
            statements.push(emit_each(field, |span| {
                quote! {
                    {
                        let __miden_diagnostics_edits = [#runtime::TextEdit {
                            span: #runtime::Spanned::span(#span),
                            replacement: format_args!(#replacement),
                        }];
                        __miden_diagnostics_visitor.suggestion(#runtime::Suggestion {
                            message: format_args!(#message),
                            applicability: #runtime::Applicability::#applicability,
                            edits: &__miden_diagnostics_edits,
                        });
                    }
                }
            }));
        }
        if field.roles.suggestions {
            statements.push(emit_each(field, |suggestion| {
                quote! {
                    __miden_diagnostics_visitor.suggestion(#runtime::Suggestion {
                        message: format_args!("{}", #suggestion.message),
                        applicability: #suggestion.applicability,
                        edits: &#suggestion.edits,
                    });
                }
            }));
        }
        if field.roles.related {
            statements.push(emit_each(
                field,
                |related| quote!(__miden_diagnostics_visitor.related(#related);),
            ));
        }
    }
    if let Some(help) = &case.args.help {
        statements.push(quote! {
            __miden_diagnostics_visitor.note(#runtime::Note {
                kind: #runtime::NoteKind::Help,
                message: format_args!(#help),
            });
        });
    }
    quote!(#(#statements)*)
}

fn cause_body(case: &CaseSpec, runtime: &TokenStream2) -> TokenStream2 {
    if let Some(target) = case_forward_target(case, runtime) {
        return quote!(#runtime::Diagnostic::cause(#target));
    }
    let Some(field) = case.fields.iter().find(|field| field.roles.source) else {
        return quote!(None);
    };
    let binding = &field.binding;
    match field.shape {
        Cardinality::One if field.boxed => {
            quote!(Some(&**#binding as &(dyn core::error::Error + 'static)))
        }
        Cardinality::One => quote!(Some(#binding as &(dyn core::error::Error + 'static))),
        Cardinality::Optional if field.boxed => quote!(
            #binding
                .as_ref()
                .map(|value| &**value as &(dyn core::error::Error + 'static))
        ),
        Cardinality::Optional => quote!(
            #binding
                .as_ref()
                .map(|value| value as &(dyn core::error::Error + 'static))
        ),
        Cardinality::Many | Cardinality::OptionalMany => unreachable!(),
    }
}

fn diagnostic_source_body(case: &CaseSpec, runtime: &TokenStream2) -> TokenStream2 {
    if let Some(target) = case_forward_target(case, runtime) {
        return quote!(#runtime::Diagnostic::diagnostic_source(#target));
    }
    let Some(field) = case.fields.iter().find(|field| field.roles.diagnostic_source) else {
        return quote!(None);
    };
    let binding = &field.binding;
    match field.shape {
        Cardinality::One if field.boxed => {
            quote!(Some(&**#binding as &dyn #runtime::Diagnostic))
        }
        Cardinality::One => quote!(Some(#binding as &dyn #runtime::Diagnostic)),
        Cardinality::Optional if field.boxed => quote!(
            #binding
                .as_ref()
                .map(|value| &**value as &dyn #runtime::Diagnostic)
        ),
        Cardinality::Optional => quote!(
            #binding
                .as_ref()
                .map(|value| value as &dyn #runtime::Diagnostic)
        ),
        Cardinality::Many | Cardinality::OptionalMany => {
            unreachable!("diagnostic-source collections are rejected")
        }
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

fn descriptor_static(case: &CaseSpec, runtime: &TokenStream2) -> TokenStream2 {
    let Some(descriptor_ident) = &case.descriptor_ident else {
        return TokenStream2::new();
    };
    let code = case.args.code.as_ref().expect("inline code was validated");
    let canonical = code.value();
    let (namespace, code_part) = canonical.split_once('/').expect("inline code was validated");
    let namespace = LitStr::new(namespace, code.span());
    let code_part = LitStr::new(code_part, code.span());
    let summary = case.args.summary.as_ref().expect("summary was validated");
    let severity = case.args.severity.as_ref().map_or_else(|| format_ident!("Error"), Clone::clone);
    let explanation = case.args.explanation.as_ref().map_or_else(
        || quote!(#runtime::Explanation::NotProvided),
        |explanation| quote!(#runtime::__include_explanation!(#explanation)),
    );
    let documentation_url = case
        .args
        .documentation_url
        .as_ref()
        .map_or_else(|| quote!(None), |url| quote!(Some(#url)));
    let tags = &case.args.tags;
    quote! {
        static #descriptor_ident: #runtime::DiagnosticDescriptor =
            #runtime::DiagnosticDescriptor {
                code: #runtime::DiagnosticCode {
                    namespace: #namespace,
                    code: #code_part,
                },
                summary: #summary,
                default_severity: #runtime::Severity::#severity,
                explanation: #explanation,
                documentation_url: #documentation_url,
                tags: &[#(#runtime::DiagnosticTag::#tags),*],
                origin: #runtime::DescriptorOrigin {
                    module_path: module_path!(),
                    file: file!(),
                    line: line!(),
                },
            };
        #runtime::__register_descriptor!(&#descriptor_ident);
    }
}

fn add_required_bounds(
    generics: &mut Generics,
    name: &Ident,
    cases: &[CaseSpec],
    runtime: &TokenStream2,
) -> syn::Result<()> {
    let mut predicates = Vec::<syn::WherePredicate>::new();
    if cases.iter().any(|case| {
        !case.args.transparent && case.args.forward.is_none() && case.args.message.is_none()
    }) {
        let (_, type_generics, _) = generics.split_for_impl();
        predicates.push(syn::parse2(quote!(#name #type_generics: core::fmt::Display))?);
    }
    for case in cases {
        for field in &case.fields {
            let ty = &field.element_ty;
            if field.roles.related
                || field.roles.diagnostic_source
                || case
                    .args
                    .forward
                    .as_ref()
                    .is_some_and(|member| members_equal(member, &field.member))
                || (case.args.transparent && case.fields.len() == 1)
            {
                predicates.push(syn::parse2(quote!(#ty: #runtime::Diagnostic))?);
            }
            if field.roles.label.is_some() || field.roles.suggestion.is_some() {
                predicates.push(syn::parse2(quote!(#ty: #runtime::Spanned))?);
            }
            if field.roles.source {
                predicates.push(syn::parse2(quote!(#ty: core::error::Error + 'static))?);
            }
        }
    }
    generics.make_where_clause().predicates.extend(predicates);
    Ok(())
}

#[cfg(test)]
mod tests {
    use quote::quote;
    use syn::DeriveInput;

    #[test]
    fn generated_target_code_uses_facades_without_consumer_feature_tests() {
        let input: DeriveInput = syn::parse2(quote! {
            #[derive(Debug)]
            #[diagnostic(
                crate = runtime_api,
                code = "test/E1",
                summary = "inline",
                explanation = include_str!("not-expanded-by-this-test.md"),
                message = "value {value}"
            )]
            struct Example<T: core::fmt::Display + core::fmt::Debug> {
                value: T,
            }
        })
        .unwrap();

        let expanded = super::expand_diagnostic(&input).unwrap().to_string();
        assert!(!expanded.contains("cfg"));
        assert!(!expanded.contains("feature"));
        assert!(expanded.contains("__include_explanation"));
        assert!(expanded.contains("__register_descriptor"));
        assert!(expanded.contains("const _"));
        assert!(expanded.contains("runtime_api"));
    }

    #[test]
    fn complete_forwarding_delegates_every_protocol_method() {
        let input: DeriveInput = syn::parse2(quote! {
            #[derive(Debug)]
            #[diagnostic(crate = runtime_api, forward(inner))]
            struct Wrapper<T> {
                context: u8,
                #[diagnostic_source]
                inner: T,
            }
        })
        .unwrap();

        let expanded = super::expand_diagnostic(&input).unwrap().to_string();
        for method in [
            "message",
            "descriptor",
            "code",
            "severity",
            "tags",
            "visit",
            "cause",
            "diagnostic_source",
        ] {
            assert!(
                expanded.contains(&format!("Diagnostic :: {method}")),
                "missing delegation for {method}: {expanded}"
            );
        }
    }
}
