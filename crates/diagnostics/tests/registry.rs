use core::error::Error as _;

use miden_diagnostics::{
    DescriptorOrigin, DiagnosticCode, DiagnosticDescriptor, Explanation, LookupError,
    RegistryError, Severity, StaticRegistry,
};

mod alpha {
    miden_diagnostics::diagnostic_codes! {
        namespace = "registry::alpha";
        catalog = DIAGNOSTICS;

        pub E1000 {
            summary: "authored explanation",
            severity: Error,
            explanation: "# Authored explanation\n\nExact UTF-8 sentinel: λ\n",
            documentation_url: "https://example.com/diagnostics/E1000",
        }

        pub W2000 {
            summary: "no authored explanation",
            severity: Warning,
        }
    }
}

mod beta {
    miden_diagnostics::diagnostic_codes! {
        namespace = "registry::beta";
        catalog = DIAGNOSTICS;

        pub E1000 {
            summary: "same short code in another namespace",
            severity: Error,
        }
    }
}

use alpha::{DIAGNOSTICS as ALPHA_DIAGNOSTICS, E1000, W2000};
use beta::DIAGNOSTICS as BETA_DIAGNOSTICS;

static ALWAYS_EMBEDDED: DiagnosticDescriptor = DiagnosticDescriptor {
    code: DiagnosticCode {
        namespace: "registry::manual",
        code: "I3000",
    },
    summary: "manually embedded explanation",
    default_severity: Severity::Info,
    explanation: Explanation::Embedded("manual embedded sentinel"),
    documentation_url: None,
    tags: &[],
    origin: DescriptorOrigin {
        module_path: "registry",
        file: file!(),
        line: line!(),
    },
};
static MANUAL_DIAGNOSTICS: &[&DiagnosticDescriptor] = &[&ALWAYS_EMBEDDED];
static CATALOGS: &[&[&DiagnosticDescriptor]] =
    &[ALPHA_DIAGNOSTICS, BETA_DIAGNOSTICS, MANUAL_DIAGNOSTICS];
static REGISTRY: StaticRegistry<'static> = StaticRegistry::from_slices(CATALOGS);

#[test]
fn explicit_registry_public_contract_is_repeatable_and_exact() {
    let expected = [E1000.code, W2000.code, BETA_DIAGNOSTICS[0].code, ALWAYS_EMBEDDED.code];

    for _ in 0..2 {
        assert_eq!(REGISTRY.iter().map(|descriptor| descriptor.code).collect::<Vec<_>>(), expected);
        assert_eq!(REGISTRY.validate(), Ok(()));
        assert!(core::ptr::eq(REGISTRY.lookup("registry::alpha/E1000").unwrap(), &E1000,));
        assert!(core::ptr::eq(REGISTRY.lookup("W2000").unwrap(), &W2000));
        assert_eq!(
            REGISTRY.lookup("E1000"),
            Err(LookupError::AmbiguousCode {
                first: E1000.code,
                second: BETA_DIAGNOSTICS[0].code,
            })
        );
        assert_eq!(REGISTRY.lookup("e1000"), Err(LookupError::UnknownCode));
        assert_eq!(REGISTRY.lookup("registry::alpha/E1000/extra"), Err(LookupError::UnknownCode));
        assert_eq!(REGISTRY.lookup("missing"), Err(LookupError::UnknownCode));
    }
}

#[test]
fn explanation_states_remain_orthogonal_to_lookup() {
    let authored = REGISTRY.lookup("registry::alpha/E1000").unwrap();
    #[cfg(feature = "embed-explanations")]
    assert_eq!(
        authored.explanation,
        Explanation::Embedded("# Authored explanation\n\nExact UTF-8 sentinel: λ\n")
    );
    #[cfg(not(feature = "embed-explanations"))]
    assert_eq!(authored.explanation, Explanation::NotEmbedded);

    assert_eq!(REGISTRY.lookup("W2000").unwrap().explanation, Explanation::NotProvided);
    assert_eq!(
        REGISTRY.lookup("I3000").unwrap().explanation,
        Explanation::Embedded("manual embedded sentinel")
    );
    assert_eq!(REGISTRY.lookup("unknown"), Err(LookupError::UnknownCode));
}

#[test]
fn duplicate_errors_retain_origins_and_are_error_sources() {
    let repeated: &[&DiagnosticDescriptor] = &[&E1000, &E1000];
    let catalogs: &[&[&DiagnosticDescriptor]] = &[repeated];
    let registry = StaticRegistry::from_slices(catalogs);
    let duplicate = RegistryError::DuplicateCode {
        code: E1000.code,
        first_origin: E1000.origin,
        second_origin: E1000.origin,
    };

    assert_eq!(registry.validate(), Err(duplicate));
    let lookup = registry.lookup("registry::alpha/E1000").unwrap_err();
    assert_eq!(lookup, LookupError::InvalidRegistry(duplicate));
    assert!(lookup.source().is_some());
    assert!(lookup.to_string().contains("duplicate diagnostic code"));
    assert_eq!(LookupError::from(duplicate), lookup);
}

#[test]
fn fixture_catalog_governance_is_auditable() {
    assert_eq!(
        REGISTRY.iter().map(|descriptor| descriptor.code).collect::<Vec<_>>(),
        [E1000.code, W2000.code, BETA_DIAGNOSTICS[0].code, ALWAYS_EMBEDDED.code,]
    );
    for descriptor in REGISTRY.iter() {
        assert!(!descriptor.code.namespace.is_empty());
        assert!(!descriptor.code.code.is_empty());
        assert!(!descriptor.code.namespace.contains('/'));
        assert!(!descriptor.code.code.contains('/'));
        assert!(!descriptor.summary.is_empty());
        assert!(!descriptor.origin.module_path.is_empty());
        assert!(!descriptor.origin.file.is_empty());
        assert!(descriptor.origin.line > 0);
        if let Some(url) = descriptor.documentation_url {
            assert!(url.starts_with("https://") || url.starts_with("http://"));
            assert!(!url.bytes().any(|byte| byte.is_ascii_control() || byte == b' '));
        }
    }
}
