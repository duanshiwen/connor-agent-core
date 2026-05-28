#[test]
fn release_contract_declares_public_api_stability_boundary() {
    let contract_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("schemas/release-contract.toml");
    let contract =
        std::fs::read_to_string(contract_path).expect("release contract should be readable");

    assert!(
        contract.contains("[public_api]"),
        "release contract must declare public API metadata"
    );
    assert!(
        contract.contains("stability_boundary"),
        "release contract must name the public API stability boundary"
    );
    assert!(
        contract.contains("stable_crates"),
        "release contract must list stable API crates"
    );
    assert!(
        contract.contains("[public_api.unstable]"),
        "release contract must list unstable API categories"
    );
    assert!(
        contract.contains("[public_api.policy]"),
        "release contract must define API change/deprecation policy"
    );
    assert!(
        contract.contains("agentos-kernel")
            && contract.contains("action-runtime")
            && contract.contains("audit-log")
            && contract.contains("enterprise-permission-core"),
        "release contract must cover the stable public API boundary crates"
    );
}
