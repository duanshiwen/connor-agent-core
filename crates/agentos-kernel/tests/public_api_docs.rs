#[test]
fn workspace_readme_documents_public_api_stability_boundary() {
    let readme_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("README.md");
    let readme = std::fs::read_to_string(readme_path).expect("workspace README should be readable");

    assert!(
        readme.contains("## Public API Stability Boundary"),
        "README must document the public API stability boundary"
    );
    assert!(
        readme.contains("### Stable API"),
        "README must list stable API"
    );
    assert!(
        readme.contains("### Unstable API"),
        "README must list unstable API"
    );
    assert!(
        readme.contains("### Deprecation Policy"),
        "README must define deprecation policy"
    );
    assert!(
        readme.contains("agentos-kernel")
            && readme.contains("action-runtime")
            && readme.contains("audit-log")
            && readme.contains("enterprise-permission-core"),
        "README must cover the M24 public API boundary crates"
    );
}
