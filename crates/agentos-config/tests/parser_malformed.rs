use agentos_config::{AgentOsConfig, AgentOsConfigDocument, ConfigError};

#[test]
fn malformed_config_inputs_return_parse_errors_without_panicking() {
    let malformed_inputs = [
        "= not toml",
        "[model\ndefault_provider = \"openai\"",
        "[storage]\nroot = [\"not\", \"a\", \"string\"]",
        "[model.providers.openai]\ntimeout_secs = \"not-a-number\"",
        "\u{0}",
    ];

    for input in malformed_inputs {
        let config_result = AgentOsConfig::from_toml_str(input);
        assert!(
            matches!(config_result, Err(ConfigError::ParseToml { .. })),
            "malformed config should return ParseToml for input: {input:?}"
        );

        let document_result = AgentOsConfigDocument::from_toml_str(input);
        assert!(
            matches!(document_result, Err(ConfigError::ParseToml { .. })),
            "malformed config document should return ParseToml for input: {input:?}"
        );
    }
}
