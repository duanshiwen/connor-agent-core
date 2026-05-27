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
    assert!(
        script.contains("Commercial-Pilot Fixture Freeze Acceptance")
            && script.contains("Long-Lived Fixture Support Policy")
            && script.contains("Migration Release Note Template"),
        "release gate must include PR205 commercial storage/journal fixture freeze checks"
    );
    assert!(
        script.contains("PR206 OAuth Provider Lifecycle Evidence"),
        "release gate must include PR206 OAuth provider lifecycle evidence checks"
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

#[test]
fn storage_journal_fixture_freeze_records_pr205_commercial_acceptance() {
    let root = workspace_root();
    let acceptance =
        fs::read_to_string(root.join("docs/storage-journal-fixture-freeze-acceptance.md"))
            .expect("storage/journal fixture freeze acceptance evidence should exist");
    let policy = fs::read_to_string(root.join("docs/storage-journal-fixture-freeze-policy.md"))
        .expect("storage/journal fixture freeze policy should exist");
    let plan = fs::read_to_string(root.join("docs/commercial-pilot-readiness-plan.md"))
        .expect("commercial pilot readiness plan should exist");

    for required in [
        "## Commercial-Pilot Fixture Freeze Acceptance",
        "## Long-Lived Fixture Support Policy",
        "## Migration Release Note Template",
        "## Rollback and Backup Expectations",
        "commercial-pilot compatibility baseline",
        "migration + fixture + rollback evidence",
        "pilot owner accepts current fixtures",
    ] {
        assert!(
            acceptance.contains(required),
            "PR205 commercial fixture acceptance must contain {required}"
        );
    }

    assert!(
        policy.contains("Commercial-pilot fixtures cannot be removed without pilot approval")
            && policy.contains("storage-journal-fixture-freeze-acceptance.md"),
        "fixture freeze policy must preserve commercial-pilot removal and acceptance references"
    );
    assert!(
        plan.contains("PR205: Storage/Journal Commercial Fixture Freeze Acceptance ✅ Completed"),
        "commercial pilot readiness plan must mark PR205 complete"
    );
}

#[test]
fn oauth_provider_lifecycle_records_pr206_evidence() {
    let root = workspace_root();
    let evidence =
        fs::read_to_string(root.join("docs/connector-browser-commercial-review-evidence.md"))
            .expect("connector/browser commercial review evidence should exist");
    let plan = fs::read_to_string(root.join("docs/commercial-pilot-readiness-plan.md"))
        .expect("commercial pilot readiness plan should exist");

    for required in [
        "## PR206 OAuth Provider Lifecycle Evidence",
        "OAuthProviderEndpointConfig",
        "FakeOAuthTokenRevoker",
        "revoke_oauth_credential_calls_provider_and_deletes_store_record",
        "offboard_connector_account_revokes_credentials_and_refresh_fails_closed",
        "oauth_lifecycle_audit_event_is_metadata_only",
        "metadata-only and omits access/refresh token material",
    ] {
        assert!(
            evidence.contains(required),
            "PR206 OAuth lifecycle evidence must contain {required}"
        );
    }

    assert!(
        plan.contains(
            "PR206: OAuth Provider Endpoint, Revocation, and Offboarding Evidence ✅ Completed"
        ),
        "commercial pilot readiness plan must mark PR206 complete"
    );
}
