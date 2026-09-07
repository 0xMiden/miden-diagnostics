use diag_runtime::Explanation;

#[test]
fn reusable_definition_build_omits_authored_explanations() {
    assert_eq!(
        derive_definitions::embedded_state(),
        Explanation::NotEmbedded
    );
    assert_eq!(
        derive_definitions::not_provided_state(),
        Explanation::NotProvided
    );
    assert_eq!(derive_definitions::collision_sentinel(), "consumer item");
}
