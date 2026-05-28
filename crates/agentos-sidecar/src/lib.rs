use identity_core::{Ed25519CryptoProvider, SignedChallenge};
use tonic::{Request, Response, Status};

pub mod pb {
    tonic::include_proto!("agentos.sidecar.v1");
}

use pb::identity_core_service_server::IdentityCoreService;
use pb::sidecar_health_service_server::SidecarHealthService;
use pb::{
    HealthCheckRequest, HealthCheckResponse, VerifyEd25519ChallengeRequest,
    VerifyEd25519ChallengeResponse,
};

pub const SDK_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Default)]
pub struct HealthService;

#[tonic::async_trait]
impl SidecarHealthService for HealthService {
    async fn check(
        &self,
        _request: Request<HealthCheckRequest>,
    ) -> Result<Response<HealthCheckResponse>, Status> {
        Ok(Response::new(HealthCheckResponse {
            status: "ok".to_string(),
            sdk_version: SDK_VERSION.to_string(),
            enabled_capabilities: vec!["health".to_string(), "identity.ed25519".to_string()],
        }))
    }
}

#[derive(Debug, Default)]
pub struct IdentityService {
    crypto: Ed25519CryptoProvider,
}

impl IdentityService {
    pub fn new() -> Self {
        Self {
            crypto: Ed25519CryptoProvider::new(),
        }
    }
}

#[tonic::async_trait]
impl IdentityCoreService for IdentityService {
    async fn verify_ed25519_challenge(
        &self,
        request: Request<VerifyEd25519ChallengeRequest>,
    ) -> Result<Response<VerifyEd25519ChallengeResponse>, Status> {
        let req = request.into_inner();
        let signed = SignedChallenge {
            challenge: req.challenge,
            signature: req.signature_hex,
            public_key: req.public_key_hex,
        };

        match self.crypto.verify_challenge(&signed) {
            Ok(valid) => Ok(Response::new(VerifyEd25519ChallengeResponse {
                valid,
                error_code: String::new(),
                error_message: String::new(),
            })),
            Err(err) => Ok(Response::new(VerifyEd25519ChallengeResponse {
                valid: false,
                error_code: "identity_verification_error".to_string(),
                error_message: err.to_string(),
            })),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use identity_core::{CryptoProvider, Ed25519CryptoProvider};

    #[tokio::test]
    async fn health_check_reports_identity_capability() {
        let svc = HealthService;
        let response = svc
            .check(Request::new(HealthCheckRequest {}))
            .await
            .unwrap()
            .into_inner();

        assert_eq!(response.status, "ok");
        assert!(
            response
                .enabled_capabilities
                .contains(&"identity.ed25519".to_string())
        );
    }

    #[tokio::test]
    async fn verify_ed25519_challenge_accepts_valid_signature() {
        let crypto = Ed25519CryptoProvider::new();
        let keypair = crypto.generate_keypair().unwrap();
        let signed = crypto
            .sign_challenge("challenge-123", &keypair.private_key, &keypair.public_key)
            .unwrap();

        let svc = IdentityService::new();
        let response = svc
            .verify_ed25519_challenge(Request::new(VerifyEd25519ChallengeRequest {
                challenge: signed.challenge,
                signature_hex: signed.signature,
                public_key_hex: signed.public_key,
            }))
            .await
            .unwrap()
            .into_inner();

        assert!(response.valid);
        assert!(response.error_code.is_empty());
    }

    #[tokio::test]
    async fn verify_ed25519_challenge_rejects_wrong_signature() {
        let crypto = Ed25519CryptoProvider::new();
        let keypair = crypto.generate_keypair().unwrap();
        let signed = crypto
            .sign_challenge("challenge-123", &keypair.private_key, &keypair.public_key)
            .unwrap();

        let svc = IdentityService::new();
        let response = svc
            .verify_ed25519_challenge(Request::new(VerifyEd25519ChallengeRequest {
                challenge: "different-challenge".to_string(),
                signature_hex: signed.signature,
                public_key_hex: signed.public_key,
            }))
            .await
            .unwrap()
            .into_inner();

        assert!(!response.valid);
        assert!(response.error_code.is_empty());
    }

    #[tokio::test]
    async fn verify_ed25519_challenge_returns_error_for_malformed_hex() {
        let svc = IdentityService::new();
        let response = svc
            .verify_ed25519_challenge(Request::new(VerifyEd25519ChallengeRequest {
                challenge: "challenge-123".to_string(),
                signature_hex: "not-hex".to_string(),
                public_key_hex: "also-not-hex".to_string(),
            }))
            .await
            .unwrap()
            .into_inner();

        assert!(!response.valid);
        assert_eq!(response.error_code, "identity_verification_error");
        assert!(response.error_message.contains("hex value"));
    }
}
