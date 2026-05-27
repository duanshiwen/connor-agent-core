use std::{fs, path::PathBuf};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .expect("agentos-kernel crate should live under crates/")
        .to_path_buf()
}

#[test]
fn security_review_checklist_covers_required_high_risk_areas() {
    let root = workspace_root();
    let checklist = fs::read_to_string(root.join("docs/security-review-checklist.md"))
        .expect("security review checklist should exist");

    for heading in [
        "## Browser Risk Checklist",
        "## Credential Checklist",
        "## Connector Checklist",
        "## Enterprise Permission Checklist",
    ] {
        assert!(
            checklist.contains(heading),
            "security checklist must include {heading}"
        );
    }

    assert!(
        checklist.contains("High-risk PRs must reference this checklist"),
        "security checklist must define the high-risk PR citation rule"
    );
    assert!(
        checklist.contains("[ ]"),
        "security checklist should use actionable checkbox items"
    );
}

#[test]
fn release_gate_and_readme_reference_security_review_checklist() {
    let root = workspace_root();
    let release_gate = fs::read_to_string(root.join("scripts/release-gate.sh")).unwrap();
    let readme = fs::read_to_string(root.join("README.md")).unwrap();

    assert!(
        release_gate.contains("docs/security-review-checklist.md"),
        "release gate docs check must verify the security review checklist exists"
    );
    assert!(
        readme.contains("docs/security-review-checklist.md"),
        "README release checklist must reference the security review checklist"
    );
}
