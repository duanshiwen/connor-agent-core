use std::{fs, path::PathBuf};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .expect("agentos-kernel crate should live under crates/")
        .to_path_buf()
}

#[test]
fn workspace_declares_minimal_kernel_host_examples() {
    let root = workspace_root();
    let cargo_toml = fs::read_to_string(root.join("crates/agentos-kernel/Cargo.toml")).unwrap();

    for example in [
        "minimal-cli-host",
        "minimal-server-host",
        "minimal-desktop-host",
    ] {
        assert!(
            cargo_toml.contains(&format!("name = \"{example}\"")),
            "agentos-kernel Cargo.toml must declare {example} example"
        );
    }
}

#[test]
fn minimal_kernel_host_examples_are_documented_and_intentionally_thin() {
    let root = workspace_root();
    let readme = fs::read_to_string(root.join("examples/README.md"))
        .expect("examples README should document host examples");

    for (example, file_name) in [
        ("minimal CLI host", "minimal_cli_host.rs"),
        ("minimal server host", "minimal_server_host.rs"),
        ("minimal desktop host boundary", "minimal_desktop_host.rs"),
    ] {
        assert!(readme.contains(example), "README must mention {example}");
        assert!(readme.contains(file_name), "README must link {file_name}");
        assert!(
            root.join("examples").join(file_name).exists(),
            "{file_name} should exist"
        );
    }

    assert!(
        readme.contains("do not implement product behavior"),
        "examples must state that they only prove API integration"
    );
    assert!(
        readme.contains("PR201 commercial pilot host integration evidence"),
        "README must identify the PR201 host integration evidence scope"
    );
}

#[test]
fn server_and_desktop_examples_cover_pr201_host_integration_paths() {
    let root = workspace_root();
    let server = fs::read_to_string(root.join("examples/minimal_server_host.rs")).unwrap();
    let desktop = fs::read_to_string(root.join("examples/minimal_desktop_host.rs")).unwrap();

    for required in [
        "submit_user_message",
        "start_agent_run",
        "process_action",
        "approve_action",
        "execute_approved_action",
        "diagnostics_bundle",
    ] {
        assert!(
            server.contains(required),
            "server example must cover backend host path: {required}"
        );
    }

    for required in [
        "AgentOsStorage::init",
        "credential_backend",
        "list_pending_approvals",
        "deny_action",
        "diagnostics_bundle",
    ] {
        assert!(
            desktop.contains(required),
            "desktop example must cover macOS host boundary: {required}"
        );
    }
}

#[test]
fn release_gate_checks_host_examples_compile() {
    let root = workspace_root();
    let release_gate = fs::read_to_string(root.join("scripts/release-gate.sh")).unwrap();

    for example in [
        "minimal-cli-host",
        "minimal-server-host",
        "minimal-desktop-host",
    ] {
        assert!(
            release_gate.contains(&format!(
                "cargo check -p agentos-kernel --example {example}"
            )),
            "release gate must compile {example}"
        );
    }
}
