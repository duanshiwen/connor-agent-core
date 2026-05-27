use std::{fs, path::PathBuf};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .expect("agentos-kernel crate should live under crates/")
        .to_path_buf()
}

#[test]
fn commercial_readiness_review_documents_required_sections() {
    let root = workspace_root();
    let report = fs::read_to_string(root.join("docs/v2-commercial-readiness-review.md"))
        .expect("v2 commercial readiness review should exist");

    for heading in [
        "## Readiness Report",
        "## Remaining Gaps",
        "## API Freeze Proposal",
        "## Storage Format Freeze Proposal",
        "## Beta Entry Conditions",
        "## Commercial Pilot Entry Conditions",
    ] {
        assert!(
            report.contains(heading),
            "readiness report must include {heading}"
        );
    }

    for stable_boundary in [
        "agentos-kernel",
        "action-runtime",
        "audit-log",
        "enterprise-permission-core",
    ] {
        assert!(
            report.contains(stable_boundary),
            "readiness review must name stable boundary crate {stable_boundary}"
        );
    }
}

#[test]
fn release_gate_and_readme_reference_commercial_readiness_review() {
    let root = workspace_root();
    let release_gate = fs::read_to_string(root.join("scripts/release-gate.sh")).unwrap();
    let readme = fs::read_to_string(root.join("README.md")).unwrap();

    assert!(
        release_gate.contains("docs/v2-commercial-readiness-review.md"),
        "release gate docs check must verify the v2 readiness review exists"
    );
    assert!(
        readme.contains("docs/v2-commercial-readiness-review.md"),
        "README release checklist must reference the v2 readiness review"
    );
}
