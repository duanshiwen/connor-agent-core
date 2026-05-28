//! # Skill Core
//!
//! `skill-core` is the SDK contract layer for AgentOS runtime skills.
//!
//! A skill is a task-domain capability package: it organizes instructions,
//! action bindings, permission requirements, context requirements, runtime/model
//! profile preferences, availability metadata, and validation contracts.
//!
//! This crate depends on the action and model contracts, but it does not execute
//! actions and it does not call models. It is not a marketplace, installer UI,
//! remote distribution system, or payment/subscription surface.
//!
//! Client and server hosts can use the same [`SkillManifest`] to evaluate
//! readiness and enablement before exposing or running a skill.

use action_core::ActionKind;
use model_adapter::ModelId;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use thiserror::Error;

// ────────────────────────────────────────────────────────────────────────────
// Identity
// ────────────────────────────────────────────────────────────────────────────

/// Stable skill identifier.
///
/// Valid identifiers are lowercase ASCII kebab-case names, optionally separated
/// into dot namespaces, for example `email-assistant` or `mail.email-assistant`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct SkillId(String);

impl SkillId {
    pub const MAX_LEN: usize = 128;

    pub fn new(value: impl Into<String>) -> Result<Self, SkillIdentityError> {
        let value = value.into();
        validate_skill_id(&value)?;
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SkillId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl TryFrom<String> for SkillId {
    type Error = SkillIdentityError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl TryFrom<&str> for SkillId {
    type Error = SkillIdentityError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

fn validate_skill_id(value: &str) -> Result<(), SkillIdentityError> {
    if value.is_empty() {
        return Err(SkillIdentityError::InvalidSkillId {
            value: value.to_string(),
            reason: "skill id must not be empty".to_string(),
        });
    }
    if value.len() > SkillId::MAX_LEN {
        return Err(SkillIdentityError::InvalidSkillId {
            value: value.to_string(),
            reason: format!("skill id must be at most {} bytes", SkillId::MAX_LEN),
        });
    }
    if value.starts_with(['-', '.']) || value.ends_with(['-', '.']) {
        return Err(SkillIdentityError::InvalidSkillId {
            value: value.to_string(),
            reason: "skill id must not start or end with '-' or '.'".to_string(),
        });
    }
    if value.contains("..") {
        return Err(SkillIdentityError::InvalidSkillId {
            value: value.to_string(),
            reason: "skill id must not contain consecutive dots".to_string(),
        });
    }
    if !value.bytes().all(|byte| {
        byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-' || byte == b'.'
    }) {
        return Err(SkillIdentityError::InvalidSkillId {
            value: value.to_string(),
            reason: "skill id may only contain lowercase ASCII letters, numbers, '-', and '.'"
                .to_string(),
        });
    }
    Ok(())
}

/// Stable skill version string.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct SkillVersion(String);

impl SkillVersion {
    pub fn new(value: impl Into<String>) -> Result<Self, SkillIdentityError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(SkillIdentityError::EmptySkillVersion);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SkillVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl TryFrom<String> for SkillVersion {
    type Error = SkillIdentityError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl TryFrom<&str> for SkillVersion {
    type Error = SkillIdentityError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum SkillIdentityError {
    #[error("invalid skill id `{value}`: {reason}")]
    InvalidSkillId { value: String, reason: String },
    #[error("skill version must not be empty")]
    EmptySkillVersion,
}

// ────────────────────────────────────────────────────────────────────────────
// Manifest
// ────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillManifest {
    pub id: SkillId,
    pub name: String,
    pub version: SkillVersion,
    pub description: Option<String>,
    pub instructions: Vec<SkillInstruction>,
    pub action_bindings: Vec<SkillActionBinding>,
    pub permissions: Vec<SkillPermissionRequirement>,
    pub context: Vec<SkillContextRequirement>,
    pub runtime: SkillRuntimeProfile,
    pub availability: SkillAvailability,
    pub metadata: SkillMetadata,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillInstruction {
    pub kind: SkillInstructionKind,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillInstructionKind {
    System,
    Developer,
    UserGuidance,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillActionBinding {
    pub action_kind: ActionKind,
    pub mode: SkillActionMode,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillActionMode {
    Allow,
    RequireApproval,
    Deny,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillPermissionRequirement {
    pub scope: String,
    pub reason: String,
    pub required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillContextRequirement {
    pub provider: String,
    pub required: bool,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillRuntimeProfile {
    pub model_profile: Option<ModelId>,
    pub max_tool_turns: Option<u32>,
    pub requires_network: bool,
    pub supports_local_only: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillMetadata {
    pub author: Option<String>,
    pub tags: Vec<String>,
    pub homepage: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillAvailability {
    Production,
    DevelopmentOnly,
    TestOnly,
}

// ────────────────────────────────────────────────────────────────────────────
// Registry
// ────────────────────────────────────────────────────────────────────────────

pub trait SkillRegistry {
    fn get(&self, id: &SkillId) -> Option<SkillManifest>;
    fn list(&self) -> Vec<SkillManifestSummary>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillManifestSummary {
    pub id: SkillId,
    pub name: String,
    pub version: SkillVersion,
    pub description: Option<String>,
    pub availability: SkillAvailability,
}

impl From<&SkillManifest> for SkillManifestSummary {
    fn from(value: &SkillManifest) -> Self {
        Self {
            id: value.id.clone(),
            name: value.name.clone(),
            version: value.version.clone(),
            description: value.description.clone(),
            availability: value.availability.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct MemorySkillRegistry {
    skills: BTreeMap<SkillId, SkillManifest>,
    validation_environment: SkillValidationEnvironment,
}

impl MemorySkillRegistry {
    pub fn new(validation_environment: SkillValidationEnvironment) -> Self {
        Self {
            skills: BTreeMap::new(),
            validation_environment,
        }
    }

    pub fn register(&mut self, manifest: SkillManifest) -> Result<(), SkillRegistryError> {
        if self.skills.contains_key(&manifest.id) {
            return Err(SkillRegistryError::DuplicateSkill(manifest.id));
        }

        let report = manifest.validate(&self.validation_environment);
        if !report.is_valid() {
            return Err(SkillRegistryError::InvalidManifest {
                id: manifest.id,
                reason: report.error_summary(),
                report,
            });
        }

        self.skills.insert(manifest.id.clone(), manifest);
        Ok(())
    }

    pub fn get(&self, id: &SkillId) -> Option<SkillManifest> {
        self.skills.get(id).cloned()
    }

    pub fn list(&self) -> Vec<SkillManifestSummary> {
        self.skills
            .values()
            .map(SkillManifestSummary::from)
            .collect()
    }

    pub fn contains(&self, id: &SkillId) -> bool {
        self.skills.contains_key(id)
    }

    pub fn len(&self) -> usize {
        self.skills.len()
    }

    pub fn is_empty(&self) -> bool {
        self.skills.is_empty()
    }
}

impl SkillRegistry for MemorySkillRegistry {
    fn get(&self, id: &SkillId) -> Option<SkillManifest> {
        Self::get(self, id)
    }

    fn list(&self) -> Vec<SkillManifestSummary> {
        Self::list(self)
    }
}

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum SkillRegistryError {
    #[error("duplicate skill id: {0}")]
    DuplicateSkill(SkillId),
    #[error("invalid skill manifest {id}: {reason}")]
    InvalidManifest {
        id: SkillId,
        reason: String,
        report: SkillValidationReport,
    },
}

// ────────────────────────────────────────────────────────────────────────────
// Validation
// ────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillValidationEnvironment {
    pub available_actions: BTreeSet<ActionKind>,
    pub granted_permissions: BTreeSet<String>,
    pub available_context_providers: BTreeSet<String>,
    pub available_model_profiles: BTreeSet<ModelId>,
    pub production_mode: bool,
    pub network_available: bool,
    pub local_only: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillValidationReport {
    pub status: SkillValidationStatus,
    pub errors: Vec<SkillValidationIssue>,
    pub warnings: Vec<SkillValidationIssue>,
}

impl SkillValidationReport {
    pub fn new(errors: Vec<SkillValidationIssue>, warnings: Vec<SkillValidationIssue>) -> Self {
        Self {
            status: SkillValidationStatus::from_issues(&errors, &warnings),
            errors,
            warnings,
        }
    }

    pub fn is_valid(&self) -> bool {
        self.status != SkillValidationStatus::Invalid
    }

    pub fn has_warnings(&self) -> bool {
        !self.warnings.is_empty()
    }

    pub fn error_summary(&self) -> String {
        if self.errors.is_empty() {
            return "manifest has no validation errors".to_string();
        }
        self.errors
            .iter()
            .map(|issue| issue.message.as_str())
            .collect::<Vec<_>>()
            .join("; ")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillValidationStatus {
    Valid,
    ValidWithWarnings,
    Invalid,
}

impl SkillValidationStatus {
    pub fn from_issues(errors: &[SkillValidationIssue], warnings: &[SkillValidationIssue]) -> Self {
        if !errors.is_empty() {
            Self::Invalid
        } else if !warnings.is_empty() {
            Self::ValidWithWarnings
        } else {
            Self::Valid
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillValidationIssue {
    pub code: SkillValidationIssueCode,
    pub message: String,
    pub field: Option<String>,
}

impl SkillValidationIssue {
    pub fn error(
        code: SkillValidationIssueCode,
        field: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code,
            field: Some(field.into()),
            message: message.into(),
        }
    }

    pub fn warning(
        code: SkillValidationIssueCode,
        field: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code,
            field: Some(field.into()),
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillValidationIssueCode {
    SkillIdInvalid,
    SkillNameEmpty,
    SkillVersionEmpty,
    InstructionMissing,
    InstructionContentEmpty,
    ActionUnavailable,
    ActionBindingConflict,
    PermissionScopeEmpty,
    RequiredPermissionMissing,
    OptionalPermissionMissing,
    ContextProviderEmpty,
    RequiredContextMissing,
    OptionalContextMissing,
    ModelProfileUnavailable,
    MaxToolTurnsZero,
    NetworkRequiredButUnavailable,
    LocalOnlyUnsupported,
    DevelopmentOnlySkillInProduction,
    TestOnlySkillInProduction,
    MetadataTagEmpty,
}

impl SkillManifest {
    pub fn validate(&self, env: &SkillValidationEnvironment) -> SkillValidationReport {
        let mut errors = Vec::new();
        let mut warnings = Vec::new();

        if let Err(error) = validate_skill_id(self.id.as_str()) {
            errors.push(SkillValidationIssue::error(
                SkillValidationIssueCode::SkillIdInvalid,
                "id",
                error.to_string(),
            ));
        }
        if self.name.trim().is_empty() {
            errors.push(SkillValidationIssue::error(
                SkillValidationIssueCode::SkillNameEmpty,
                "name",
                "skill name must not be empty",
            ));
        }
        if self.version.as_str().trim().is_empty() {
            errors.push(SkillValidationIssue::error(
                SkillValidationIssueCode::SkillVersionEmpty,
                "version",
                "skill version must not be empty",
            ));
        }

        if self.instructions.is_empty() {
            errors.push(SkillValidationIssue::error(
                SkillValidationIssueCode::InstructionMissing,
                "instructions",
                "skill must include at least one instruction",
            ));
        }
        for (index, instruction) in self.instructions.iter().enumerate() {
            if instruction.content.trim().is_empty() {
                errors.push(SkillValidationIssue::error(
                    SkillValidationIssueCode::InstructionContentEmpty,
                    format!("instructions[{index}].content"),
                    "instruction content must not be empty",
                ));
            }
        }

        let mut action_modes: BTreeMap<&ActionKind, &SkillActionMode> = BTreeMap::new();
        for (index, binding) in self.action_bindings.iter().enumerate() {
            if !env.available_actions.contains(&binding.action_kind) {
                errors.push(SkillValidationIssue::error(
                    SkillValidationIssueCode::ActionUnavailable,
                    format!("action_bindings[{index}].action_kind"),
                    format!("action is unavailable: {}", binding.action_kind),
                ));
            }
            if let Some(existing_mode) = action_modes.insert(&binding.action_kind, &binding.mode)
                && existing_mode != &binding.mode
            {
                errors.push(SkillValidationIssue::error(
                    SkillValidationIssueCode::ActionBindingConflict,
                    format!("action_bindings[{index}].mode"),
                    format!("conflicting modes for action {}", binding.action_kind),
                ));
            }
        }

        for (index, permission) in self.permissions.iter().enumerate() {
            if permission.scope.trim().is_empty() {
                errors.push(SkillValidationIssue::error(
                    SkillValidationIssueCode::PermissionScopeEmpty,
                    format!("permissions[{index}].scope"),
                    "permission scope must not be empty",
                ));
            } else if !env.granted_permissions.contains(&permission.scope) {
                if permission.required {
                    errors.push(SkillValidationIssue::error(
                        SkillValidationIssueCode::RequiredPermissionMissing,
                        format!("permissions[{index}].scope"),
                        format!("required permission is missing: {}", permission.scope),
                    ));
                } else {
                    warnings.push(SkillValidationIssue::warning(
                        SkillValidationIssueCode::OptionalPermissionMissing,
                        format!("permissions[{index}].scope"),
                        format!("optional permission is missing: {}", permission.scope),
                    ));
                }
            }
        }

        for (index, context) in self.context.iter().enumerate() {
            if context.provider.trim().is_empty() {
                errors.push(SkillValidationIssue::error(
                    SkillValidationIssueCode::ContextProviderEmpty,
                    format!("context[{index}].provider"),
                    "context provider must not be empty",
                ));
            } else if !env.available_context_providers.contains(&context.provider) {
                if context.required {
                    errors.push(SkillValidationIssue::error(
                        SkillValidationIssueCode::RequiredContextMissing,
                        format!("context[{index}].provider"),
                        format!("required context provider is missing: {}", context.provider),
                    ));
                } else {
                    warnings.push(SkillValidationIssue::warning(
                        SkillValidationIssueCode::OptionalContextMissing,
                        format!("context[{index}].provider"),
                        format!("optional context provider is missing: {}", context.provider),
                    ));
                }
            }
        }

        if let Some(model_profile) = &self.runtime.model_profile
            && !env.available_model_profiles.contains(model_profile)
        {
            errors.push(SkillValidationIssue::error(
                SkillValidationIssueCode::ModelProfileUnavailable,
                "runtime.model_profile",
                format!("model profile is unavailable: {model_profile}"),
            ));
        }
        if self.runtime.max_tool_turns == Some(0) {
            errors.push(SkillValidationIssue::error(
                SkillValidationIssueCode::MaxToolTurnsZero,
                "runtime.max_tool_turns",
                "max_tool_turns must be greater than zero",
            ));
        }
        if self.runtime.requires_network && !env.network_available {
            errors.push(SkillValidationIssue::error(
                SkillValidationIssueCode::NetworkRequiredButUnavailable,
                "runtime.requires_network",
                "skill requires network but network is unavailable",
            ));
        }
        if env.local_only && !self.runtime.supports_local_only {
            errors.push(SkillValidationIssue::error(
                SkillValidationIssueCode::LocalOnlyUnsupported,
                "runtime.supports_local_only",
                "local-only host cannot run this skill",
            ));
        }

        if env.production_mode {
            match self.availability {
                SkillAvailability::Production => {}
                SkillAvailability::DevelopmentOnly => errors.push(SkillValidationIssue::error(
                    SkillValidationIssueCode::DevelopmentOnlySkillInProduction,
                    "availability",
                    "development-only skill cannot be enabled in production",
                )),
                SkillAvailability::TestOnly => errors.push(SkillValidationIssue::error(
                    SkillValidationIssueCode::TestOnlySkillInProduction,
                    "availability",
                    "test-only skill cannot be enabled in production",
                )),
            }
        }

        for (index, tag) in self.metadata.tags.iter().enumerate() {
            if tag.trim().is_empty() {
                errors.push(SkillValidationIssue::error(
                    SkillValidationIssueCode::MetadataTagEmpty,
                    format!("metadata.tags[{index}]"),
                    "metadata tag must not be empty",
                ));
            }
        }

        SkillValidationReport::new(errors, warnings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use action_core::ActionKind;
    use model_adapter::ModelId;
    use std::collections::BTreeSet;

    fn valid_env() -> SkillValidationEnvironment {
        SkillValidationEnvironment {
            available_actions: BTreeSet::from([
                ActionKind::from("mail.search"),
                ActionKind::from("mail.read"),
                ActionKind::from("mail.create_draft"),
                ActionKind::from("mail.send"),
            ]),
            granted_permissions: BTreeSet::from([
                "mail.read".to_string(),
                "mail.write".to_string(),
                "mail.send".to_string(),
            ]),
            available_context_providers: BTreeSet::from([
                "contacts".to_string(),
                "user_preferences".to_string(),
            ]),
            available_model_profiles: BTreeSet::from([ModelId::from("test/default")]),
            production_mode: true,
            network_available: true,
            local_only: false,
        }
    }

    fn valid_manifest() -> SkillManifest {
        SkillManifest {
            id: SkillId::new("mail.email-assistant").unwrap(),
            name: "Email Assistant".to_string(),
            version: SkillVersion::new("0.1.0").unwrap(),
            description: Some("Helps search, draft, and send email.".to_string()),
            instructions: vec![SkillInstruction {
                kind: SkillInstructionKind::System,
                content: "Prefer drafts before sending email.".to_string(),
            }],
            action_bindings: vec![
                SkillActionBinding {
                    action_kind: ActionKind::from("mail.search"),
                    mode: SkillActionMode::Allow,
                    reason: None,
                },
                SkillActionBinding {
                    action_kind: ActionKind::from("mail.send"),
                    mode: SkillActionMode::RequireApproval,
                    reason: Some("Sending email mutates an external system.".to_string()),
                },
            ],
            permissions: vec![SkillPermissionRequirement {
                scope: "mail.send".to_string(),
                reason: "Required to send approved emails.".to_string(),
                required: true,
            }],
            context: vec![SkillContextRequirement {
                provider: "contacts".to_string(),
                required: true,
                reason: Some("Needed to resolve recipients.".to_string()),
            }],
            runtime: SkillRuntimeProfile {
                model_profile: Some(ModelId::from("test/default")),
                max_tool_turns: Some(8),
                requires_network: true,
                supports_local_only: true,
            },
            availability: SkillAvailability::Production,
            metadata: SkillMetadata {
                author: Some("AgentOS".to_string()),
                tags: vec!["mail".to_string(), "assistant".to_string()],
                homepage: Some("https://example.com/skills/mail".to_string()),
            },
        }
    }

    fn assert_error(report: &SkillValidationReport, code: SkillValidationIssueCode) {
        assert!(
            report.errors.iter().any(|issue| issue.code == code),
            "expected error {code:?}, got {report:?}"
        );
    }

    fn assert_warning(report: &SkillValidationReport, code: SkillValidationIssueCode) {
        assert!(
            report.warnings.iter().any(|issue| issue.code == code),
            "expected warning {code:?}, got {report:?}"
        );
    }

    #[test]
    fn skill_id_accepts_kebab_and_dot_namespace() {
        for id in [
            "email-assistant",
            "mail.email-assistant",
            "knowledge.researcher",
        ] {
            assert_eq!(SkillId::new(id).unwrap().as_str(), id);
        }
    }

    #[test]
    fn skill_id_rejects_empty_uppercase_spaces_and_edge_separators() {
        for id in [
            "",
            "EmailAssistant",
            "email assistant",
            "-email",
            "email-",
            ".email",
            "email.",
            "mail..email",
            "mail_email",
        ] {
            assert!(SkillId::new(id).is_err(), "{id} should be rejected");
        }
    }

    #[test]
    fn skill_version_rejects_empty() {
        assert!(SkillVersion::new("").is_err());
        assert!(SkillVersion::new("   ").is_err());
        assert_eq!(SkillVersion::new("0.1.0").unwrap().as_str(), "0.1.0");
    }

    #[test]
    fn skill_manifest_json_roundtrip() {
        let manifest = valid_manifest();
        let json = serde_json::to_string(&manifest).unwrap();
        let decoded: SkillManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, manifest);
    }

    #[test]
    fn memory_registry_registers_and_gets_skill() {
        let manifest = valid_manifest();
        let mut registry = MemorySkillRegistry::new(valid_env());
        registry.register(manifest.clone()).unwrap();
        assert_eq!(registry.get(&manifest.id), Some(manifest));
        assert!(registry.contains(&SkillId::new("mail.email-assistant").unwrap()));
        assert_eq!(registry.len(), 1);
        assert!(!registry.is_empty());
    }

    #[test]
    fn memory_registry_lists_summaries_in_deterministic_order() {
        let mut first = valid_manifest();
        first.id = SkillId::new("knowledge.researcher").unwrap();
        first.name = "Knowledge Researcher".to_string();

        let mut second = valid_manifest();
        second.id = SkillId::new("mail.email-assistant").unwrap();

        let mut registry = MemorySkillRegistry::new(valid_env());
        registry.register(second).unwrap();
        registry.register(first).unwrap();

        let ids: Vec<String> = registry
            .list()
            .into_iter()
            .map(|summary| summary.id.to_string())
            .collect();
        assert_eq!(ids, vec!["knowledge.researcher", "mail.email-assistant"]);
    }

    #[test]
    fn memory_registry_rejects_duplicate_skill_id() {
        let manifest = valid_manifest();
        let mut registry = MemorySkillRegistry::new(valid_env());
        registry.register(manifest.clone()).unwrap();
        assert!(matches!(
            registry.register(manifest),
            Err(SkillRegistryError::DuplicateSkill(id)) if id == SkillId::new("mail.email-assistant").unwrap()
        ));
    }

    #[test]
    fn memory_registry_rejects_invalid_manifest() {
        let mut manifest = valid_manifest();
        manifest.permissions.push(SkillPermissionRequirement {
            scope: "missing.scope".to_string(),
            reason: "Needed for test".to_string(),
            required: true,
        });
        let mut registry = MemorySkillRegistry::new(valid_env());
        assert!(matches!(
            registry.register(manifest),
            Err(SkillRegistryError::InvalidManifest { .. })
        ));
    }

    #[test]
    fn validation_accepts_complete_production_skill() {
        let report = valid_manifest().validate(&valid_env());
        assert_eq!(report.status, SkillValidationStatus::Valid);
        assert!(report.is_valid());
        assert!(!report.has_warnings());
    }

    #[test]
    fn validation_warns_for_missing_optional_permission() {
        let mut manifest = valid_manifest();
        manifest.permissions.push(SkillPermissionRequirement {
            scope: "mail.archive".to_string(),
            reason: "Improves cleanup but is optional.".to_string(),
            required: false,
        });
        let report = manifest.validate(&valid_env());
        assert_eq!(report.status, SkillValidationStatus::ValidWithWarnings);
        assert_warning(&report, SkillValidationIssueCode::OptionalPermissionMissing);
    }

    #[test]
    fn validation_rejects_missing_required_permission() {
        let mut manifest = valid_manifest();
        manifest.permissions.push(SkillPermissionRequirement {
            scope: "mail.delete".to_string(),
            reason: "Required for destructive cleanup.".to_string(),
            required: true,
        });
        let report = manifest.validate(&valid_env());
        assert_error(&report, SkillValidationIssueCode::RequiredPermissionMissing);
    }

    #[test]
    fn validation_rejects_missing_required_context_provider() {
        let mut manifest = valid_manifest();
        manifest.context.push(SkillContextRequirement {
            provider: "recent_emails".to_string(),
            required: true,
            reason: None,
        });
        let report = manifest.validate(&valid_env());
        assert_error(&report, SkillValidationIssueCode::RequiredContextMissing);
    }

    #[test]
    fn validation_rejects_unavailable_action() {
        let mut manifest = valid_manifest();
        manifest.action_bindings.push(SkillActionBinding {
            action_kind: ActionKind::from("calendar.create_event"),
            mode: SkillActionMode::Allow,
            reason: None,
        });
        let report = manifest.validate(&valid_env());
        assert_error(&report, SkillValidationIssueCode::ActionUnavailable);
    }

    #[test]
    fn validation_rejects_conflicting_action_bindings() {
        let mut manifest = valid_manifest();
        manifest.action_bindings.push(SkillActionBinding {
            action_kind: ActionKind::from("mail.send"),
            mode: SkillActionMode::Deny,
            reason: Some("Conflicting test binding.".to_string()),
        });
        let report = manifest.validate(&valid_env());
        assert_error(&report, SkillValidationIssueCode::ActionBindingConflict);
    }

    #[test]
    fn validation_rejects_unavailable_model_profile() {
        let mut manifest = valid_manifest();
        manifest.runtime.model_profile = Some(ModelId::from("missing/model"));
        let report = manifest.validate(&valid_env());
        assert_error(&report, SkillValidationIssueCode::ModelProfileUnavailable);
    }

    #[test]
    fn validation_rejects_zero_max_tool_turns() {
        let mut manifest = valid_manifest();
        manifest.runtime.max_tool_turns = Some(0);
        let report = manifest.validate(&valid_env());
        assert_error(&report, SkillValidationIssueCode::MaxToolTurnsZero);
    }

    #[test]
    fn validation_rejects_network_required_when_network_unavailable() {
        let mut env = valid_env();
        env.network_available = false;
        let report = valid_manifest().validate(&env);
        assert_error(
            &report,
            SkillValidationIssueCode::NetworkRequiredButUnavailable,
        );
    }

    #[test]
    fn validation_rejects_local_only_when_skill_does_not_support_local_only() {
        let mut env = valid_env();
        env.local_only = true;
        let mut manifest = valid_manifest();
        manifest.runtime.supports_local_only = false;
        let report = manifest.validate(&env);
        assert_error(&report, SkillValidationIssueCode::LocalOnlyUnsupported);
    }

    #[test]
    fn validation_rejects_development_only_skill_in_production() {
        let mut manifest = valid_manifest();
        manifest.availability = SkillAvailability::DevelopmentOnly;
        let report = manifest.validate(&valid_env());
        assert_error(
            &report,
            SkillValidationIssueCode::DevelopmentOnlySkillInProduction,
        );
    }

    #[test]
    fn validation_rejects_test_only_skill_in_production() {
        let mut manifest = valid_manifest();
        manifest.availability = SkillAvailability::TestOnly;
        let report = manifest.validate(&valid_env());
        assert_error(&report, SkillValidationIssueCode::TestOnlySkillInProduction);
    }

    #[test]
    fn validation_rejects_empty_instruction_content() {
        let mut manifest = valid_manifest();
        manifest.instructions[0].content = " ".to_string();
        let report = manifest.validate(&valid_env());
        assert_error(&report, SkillValidationIssueCode::InstructionContentEmpty);
    }

    #[test]
    fn validation_rejects_empty_metadata_tag() {
        let mut manifest = valid_manifest();
        manifest.metadata.tags.push(" ".to_string());
        let report = manifest.validate(&valid_env());
        assert_error(&report, SkillValidationIssueCode::MetadataTagEmpty);
    }
}
