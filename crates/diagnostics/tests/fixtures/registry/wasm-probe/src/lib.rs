use std::sync::OnceLock;

use diag_runtime::{__private::RegistryEntry, linked_registry};

static INITIALIZED: OnceLock<()> = OnceLock::new();

unsafe extern "C" {
    fn __wasm_call_ctors();
}

#[cfg(feature = "anchors")]
#[inline(never)]
fn retain_contributors() {
    std::hint::black_box(registry_definitions_alpha::anchor());
    std::hint::black_box(registry_definitions_beta::anchor());
}

#[cfg(not(feature = "anchors"))]
fn retain_contributors() {}

/// Run retained Rust references and wasm-ld constructors exactly once.
#[unsafe(no_mangle)]
pub extern "C" fn registry_initialize() -> i32 {
    INITIALIZED.get_or_init(|| {
        retain_contributors();
        // SAFETY: wasm-ld synthesizes this function. The OnceLock guard calls
        // it once, before this fixture permits any sorted registry query.
        unsafe {
            __wasm_call_ctors();
        }
    });
    i32::from(INITIALIZED.get().is_some())
}

/// Count raw submissions without initializing the sorted OnceLock.
#[unsafe(no_mangle)]
pub extern "C" fn registry_raw_inventory_len() -> i32 {
    i32::try_from(
        diag_runtime::__private::inventory::iter::<RegistryEntry>
            .into_iter()
            .count(),
    )
    .unwrap_or(i32::MAX)
}

/// Return `-1` until constructors are initialized.
#[unsafe(no_mangle)]
pub extern "C" fn registry_registry_len() -> i32 {
    if INITIALIZED.get().is_none() {
        return -1;
    }
    linked_registry()
        .ok()
        .and_then(|registry| i32::try_from(registry.len()).ok())
        .unwrap_or(-2)
}

/// Return a compact tag for one sorted canonical code.
#[unsafe(no_mangle)]
pub extern "C" fn registry_registry_code(index: i32) -> i32 {
    if INITIALIZED.get().is_none() {
        return -1;
    }
    let Ok(index) = usize::try_from(index) else {
        return 0;
    };
    let Some(descriptor) = linked_registry()
        .ok()
        .and_then(|registry| registry.iter().nth(index))
    else {
        return 0;
    };
    match (descriptor.code.namespace, descriptor.code.code) {
        ("registry::alpha", "E1000") => 0xA1000,
        ("registry::alpha", "E2000") => 0xA2000,
        ("registry::alpha", "I5000") => 0xA5000,
        ("registry::alpha", "N4000") => 0xA4000,
        ("registry::beta", "E1000") => 0xB1000,
        ("registry::beta", "W3000") => 0xB3000,
        _ => 0,
    }
}
