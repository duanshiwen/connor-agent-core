use std::{fs, path::PathBuf};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .expect("agentos-kernel crate should live under crates/")
        .to_path_buf()
}

#[test]
fn release_gate_script_documents_and_runs_required_checks() {
    let root = workspace_root();
    let script_path = root.join("scripts/release-gate.sh");
    let script = fs::read_to_string(&script_path).expect("release gate script should exist");

    assert!(
        script.contains("cargo fmt --all --check"),
        "release gate must run rustfmt check"
    );
    assert!(
        script.contains("cargo clippy --workspace -- -D warnings"),
        "release gate must run clippy with warnings denied"
    );
    assert!(
        script.contains("cargo test --workspace"),
        "release gate must run full workspace tests"
    );
    assert!(
        script.contains("docs/feature-matrix.md"),
        "release gate must include a feature matrix check"
    );
    assert!(
        script.contains("docs/host-api-freeze.md")
            && script.contains("Stable Host-Facing Boundary"),
        "release gate must include host API freeze contract checks"
    );
    assert!(
        script.contains("Release Checklist"),
        "release gate must include a docs check for release checklist docs"
    );
    assert!(
        script.contains("Host-Level Pilot Backend Decision")
            && script.contains("Host-Level Pilot Rehearsal Evidence"),
        "release gate must include PR202 host-level credential rehearsal checks"
    );
    assert!(
        script.contains("Production-Like File Export Sink"),
        "release gate must include PR203 production observability file sink checks"
    );
    assert!(
        script.contains("docs/release-artifact-rollback-rehearsal.md")
            && script.contains("Release Artifact Rehearsal")
            && script.contains("Rollback Rehearsal Evidence"),
        "release gate must include PR204 release artifact and rollback rehearsal evidence checks"
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&script_path).unwrap().permissions().mode();
        assert_ne!(mode & 0o111, 0, "release gate script should be executable");
    }
}

#[test]
fn release_checklist_is_documented_in_readme_and_feature_matrix() {
    let root = workspace_root();
    let readme = fs::read_to_string(root.join("README.md")).unwrap();
    let feature_matrix = fs::read_to_string(root.join("docs/feature-matrix.md"))
        .expect("feature matrix document should exist");

    assert!(
        readme.contains("## Release Checklist"),
        "README must document release checklist usage"
    );
    assert!(
        readme.contains("./scripts/release-gate.sh"),
        "README must show the one-command release gate"
    );
    assert!(
        feature_matrix.contains("agentos-kernel")
            && feature_matrix.contains("action-runtime")
            && feature_matrix.contains("audit-log")
            && feature_matrix.contains("enterprise-permission-core"),
        "feature matrix must cover the stable public API boundary crates"
    );
}

#[test]
fn release_artifact_rollback_rehearsal_records_pr204_evidence() {
    let root = workspace_root();
    let rehearsal = fs::read_to_string(root.join("docs/release-artifact-rollback-rehearsal.md"))
        .expect("release artifact rollback rehearsal evidence should exist");
    let runbook = fs::read_to_string(root.join("docs/release-operations-runbook.md"))
        .expect("release operations runbook should exist");
    let plan = fs::read_to_string(root.join("docs/commercial-pilot-readiness-plan.md"))
        .expect("commercial pilot readiness plan should exist");

    for required in [
        "## Release Artifact Rehearsal",
        "## Rollback Rehearsal Evidence",
        "## Incident Escalation Tabletop",
        "v0.1.0-beta.rehearsal",
        "release gate passed",
        "non-production storage root",
    ] {
        assert!(
            rehearsal.contains(required),
            "PR204 rehearsal evidence must contain {required}"
        );
    }

    assert!(
        runbook.contains("release-artifact-rollback-rehearsal.md"),
        "release runbook must link PR204 rehearsal evidence"
    );
    assert!(
        plan.contains("PR204: Beta/Pilot Release Artifact and Rollback Rehearsal ✅ Completed"),
        "commercial pilot readiness plan must mark PR204 complete"
    );
}
