use std::{env, process::ExitCode};

#[cfg(feature = "duplicate")]
use diag_runtime::RegistryError;
use diag_runtime::linked_registry;
#[cfg(feature = "anchors")]
use diag_runtime::{Explanation, LookupError};

#[cfg(feature = "anchors")]
fn retain_contributors() {
    std::hint::black_box(registry_definitions_alpha::anchor());
    std::hint::black_box(registry_definitions_beta::anchor());
    #[cfg(feature = "duplicate")]
    std::hint::black_box(registry_definitions_duplicate::anchor());
}

#[cfg(not(feature = "anchors"))]
fn retain_contributors() {}

fn main() -> ExitCode {
    retain_contributors();
    let mode = env::args().nth(1).unwrap_or_else(|| "linked".into());
    match mode.as_str() {
        "linked" => linked_mode(),
        #[cfg(feature = "duplicate")]
        "duplicate" => duplicate_mode(),
        #[cfg(feature = "anchors")]
        "explain-explicit" => explain_mode(false, env::args().nth(2).as_deref().unwrap_or("")),
        #[cfg(feature = "anchors")]
        "explain-linked" => explain_mode(true, env::args().nth(2).as_deref().unwrap_or("")),
        #[cfg(feature = "anchors")]
        "normal-output" => normal_output(),
        #[cfg(feature = "anchors")]
        "panic-output" => panic_output(),
        #[cfg(feature = "anchors")]
        "governance" => governance(),
        _ => {
            eprintln!("unsupported mode: {mode}");
            ExitCode::from(64)
        }
    }
}

#[cfg(not(feature = "duplicate"))]
fn linked_mode() -> ExitCode {
    let index = linked_registry().expect("unique linked definitions must build");
    let codes = index
        .iter()
        .map(|descriptor| descriptor.code.to_string())
        .collect::<Vec<_>>();

    #[cfg(feature = "anchors")]
    {
        assert_eq!(
            codes,
            [
                "registry::alpha/E1000",
                "registry::alpha/E2000",
                "registry::alpha/I5000",
                "registry::alpha/N4000",
                "registry::beta/E1000",
                "registry::beta/W3000",
            ]
        );
        assert!(index.lookup("registry::alpha/I5000").is_ok());
        assert_eq!(
            index.lookup("E1000"),
            Err(LookupError::AmbiguousCode {
                first: registry_definitions_alpha::E1000.code,
                second: registry_definitions_beta::E1000.code,
            })
        );

        let before = index as *const _;
        let occurrence = diag_runtime::diagnostic! {
            severity: Warning,
            code: "registry::occurrence/ONLY",
            message: "expression-created occurrence",
        };
        std::hint::black_box(occurrence);
        let after = linked_registry().expect("cached registry") as *const _;
        assert_eq!(before, after);
        assert_eq!(index.len(), 6);
        assert!(!index.is_empty());
        assert_eq!(
            index.lookup("registry::occurrence/ONLY"),
            Err(LookupError::UnknownCode)
        );
        println!(
            "linked={};count=6;definition-only=true;occurrence-registration=false",
            codes.join(",")
        );
    }
    #[cfg(not(feature = "anchors"))]
    {
        assert!(codes.is_empty());
        assert!(index.is_empty());
        println!("linked=;count=0");
    }
    ExitCode::SUCCESS
}

#[cfg(feature = "duplicate")]
fn linked_mode() -> ExitCode {
    duplicate_mode()
}

#[cfg(feature = "duplicate")]
fn duplicate_mode() -> ExitCode {
    let error = linked_registry().expect_err("duplicate code must reject the linked index");
    let expected = RegistryError::DuplicateCode {
        code: registry_definitions_alpha::E1000.code,
        first_origin: registry_definitions_alpha::E1000.origin,
        second_origin: registry_definitions_duplicate::E1000.origin,
    };
    assert_eq!(error, &expected);
    println!(
        "duplicate={};origins={},{}",
        registry_definitions_alpha::E1000.code,
        registry_definitions_alpha::E1000.origin.module_path,
        registry_definitions_duplicate::E1000.origin.module_path,
    );
    ExitCode::SUCCESS
}

#[cfg(feature = "anchors")]
fn explicit_registry() -> diag_runtime::StaticRegistry<'static> {
    static CATALOGS: &[&[&diag_runtime::DiagnosticDescriptor]] = &[
        registry_definitions_alpha::ALPHA_DIAGNOSTICS,
        registry_definitions_beta::BETA_DIAGNOSTICS,
    ];
    diag_runtime::StaticRegistry::from_slices(CATALOGS)
}

#[cfg(feature = "anchors")]
fn explain_mode(linked: bool, code: &str) -> ExitCode {
    let result = if linked {
        linked_registry()
            .map_err(|error| LookupError::InvalidRegistry(*error))
            .and_then(|registry| registry.lookup(code))
    } else {
        explicit_registry().lookup(code)
    };
    let descriptor = match result {
        Ok(descriptor) => descriptor,
        Err(LookupError::UnknownCode) => {
            eprintln!("unknown={code}");
            return ExitCode::from(2);
        }
        Err(LookupError::AmbiguousCode { first, second }) => {
            eprintln!("ambiguous={code};first={first};second={second}");
            return ExitCode::from(2);
        }
        Err(LookupError::InvalidRegistry(error)) => {
            eprintln!("invalid={error}");
            return ExitCode::from(70);
        }
    };

    println!("code={};summary={}", descriptor.code, descriptor.summary);
    if let Some(url) = descriptor.documentation_url {
        println!("url={url}");
    }
    match descriptor.explanation {
        Explanation::Embedded(markdown) => {
            print!("{markdown}");
            ExitCode::SUCCESS
        }
        Explanation::NotProvided => {
            println!("explanation=not-provided");
            ExitCode::SUCCESS
        }
        Explanation::NotEmbedded => {
            eprintln!("explanation=not-embedded");
            ExitCode::from(3)
        }
    }
}

#[cfg(feature = "anchors")]
fn normal_output() -> ExitCode {
    let report = diag_runtime::Report::new(diag_runtime::diagnostic! {
        descriptor: &registry_definitions_alpha::E1000,
        message: "normal output sentinel",
    });
    println!("{report:?}");
    ExitCode::SUCCESS
}

#[cfg(feature = "anchors")]
fn panic_output() -> ExitCode {
    diag_runtime::install_panic_hook(diag_runtime::PanicHookOptions::default())
        .expect("fixture owns the process panic hook");
    let report = diag_runtime::Report::new(diag_runtime::diagnostic! {
        descriptor: &registry_definitions_alpha::E1000,
        message: "panic output sentinel",
    });
    diag_runtime::panic_report!(report)
}

#[cfg(feature = "anchors")]
fn governance() -> ExitCode {
    let explicit = explicit_registry();
    explicit
        .validate()
        .expect("explicit catalogs must be unique");
    let expected = [
        "registry::alpha/E1000",
        "registry::alpha/E2000",
        "registry::alpha/N4000",
        "registry::beta/E1000",
        "registry::beta/W3000",
    ];
    assert_eq!(
        explicit
            .iter()
            .map(|descriptor| descriptor.code.to_string())
            .collect::<Vec<_>>(),
        expected
    );
    for descriptor in explicit.iter() {
        assert!(!descriptor.origin.module_path.is_empty());
        assert!(!descriptor.origin.file.is_empty());
        assert!(descriptor.origin.line > 0);
        if let Some(url) = descriptor.documentation_url {
            assert!(safe_documentation_url(url));
        }
    }
    #[cfg(feature = "embed")]
    assert_eq!(
        registry_definitions_alpha::E1000.explanation,
        Explanation::Embedded(registry_definitions_alpha::EXPLANATION_SENTINEL)
    );
    println!("governance=ok;catalog-coverage=5;markdown-examples=0");
    ExitCode::SUCCESS
}

#[cfg(feature = "anchors")]
fn safe_documentation_url(url: &str) -> bool {
    let Some(rest) = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
    else {
        return false;
    };
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    if authority.is_empty() || authority.contains('@') {
        return false;
    }
    let bytes = url.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte == b'%' {
            if index + 2 >= bytes.len()
                || !bytes[index + 1].is_ascii_hexdigit()
                || !bytes[index + 2].is_ascii_hexdigit()
            {
                return false;
            }
            index += 3;
            continue;
        }
        if !(0x21..=0x7e).contains(&byte)
            || matches!(
                byte,
                b'\\' | b'"' | b'\'' | b'<' | b'>' | b'`' | b'{' | b'}' | b'|' | b'^'
            )
        {
            return false;
        }
        index += 1;
    }
    true
}
