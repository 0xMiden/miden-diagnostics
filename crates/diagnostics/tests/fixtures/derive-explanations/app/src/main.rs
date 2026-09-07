use runtime_api::Explanation;

fn main() {
    match derive_definitions::embedded_state() {
        Explanation::Embedded(markdown) => {
            assert!(markdown.contains("Derive sentinel"));
        }
        state => panic!("expected embedded explanation, got {state:?}"),
    }
    assert_eq!(
        derive_definitions::not_provided_state(),
        Explanation::NotProvided
    );
    println!("embedded=true; not_provided=true");
}
