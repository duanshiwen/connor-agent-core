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

#[test]
fn host_api_freeze_document_records_beta_commercial_contract() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let freeze_doc_path = root.join("docs/host-api-freeze.md");
    let freeze_doc =
        std::fs::read_to_string(&freeze_doc_path).expect("host API freeze document should exist");
    let readme = std::fs::read_to_string(root.join("README.md"))
        .expect("workspace README should be readable");
    let feature_matrix = std::fs::read_to_string(root.join("docs/feature-matrix.md"))
        .expect("feature matrix should be readable");

    for required in [
        "## Stable Host-Facing Boundary",
        "## Compatibility Rules",
        "## Breaking Change Process",
        "## Pilot Acceptance Status",
        "agentos-kernel",
        "action-runtime",
        "audit-log",
        "enterprise-permission-core",
    ] {
        assert!(
            freeze_doc.contains(required),
            "host API freeze document must contain {required}"
        );
    }

    assert!(
        readme.contains("docs/host-api-freeze.md"),
        "README must link the host API freeze contract"
    );
    assert!(
        feature_matrix.contains("host-api-freeze.md"),
        "feature matrix must link the host API freeze contract"
    );
}
