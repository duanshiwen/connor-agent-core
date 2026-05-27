use agentos_kernel::{HostApiError, HostApiErrorResponse, KernelError, KernelErrorCategory};

#[test]
fn kernel_error_category_serializes_as_stable_snake_case() {
    assert_eq!(
        serde_json::to_string(&KernelErrorCategory::Recoverable).unwrap(),
        "\"recoverable\""
    );
    assert_eq!(
        serde_json::to_string(&KernelErrorCategory::UserActionable).unwrap(),
        "\"user_actionable\""
    );
    assert_eq!(
        serde_json::to_string(&KernelErrorCategory::Bug).unwrap(),
        "\"bug\""
    );
    assert_eq!(
        serde_json::to_string(&KernelErrorCategory::External).unwrap(),
        "\"external\""
    );
}

#[test]
fn kernel_errors_map_to_stable_categories() {
    assert_eq!(
        KernelError::MissingService {
            service: "audit_log"
        }
        .category(),
        KernelErrorCategory::Bug
    );
    assert_eq!(
        KernelError::InvalidLifecycleTransition {
            from: "shutdown",
            to: "recovering",
        }
        .category(),
        KernelErrorCategory::Recoverable
    );
    assert_eq!(
        KernelError::ServiceNotFound {
            registry: "model_provider",
            service_id: "missing".to_string(),
        }
        .category(),
        KernelErrorCategory::UserActionable
    );
    assert_eq!(
        KernelError::DiagnosticsFailed {
            reason: "audit sink unavailable".to_string(),
        }
        .category(),
        KernelErrorCategory::External
    );
}

#[test]
fn host_api_errors_map_to_stable_categories() {
    assert_eq!(
        HostApiError::RunNotFound {
            run_id: "missing-run".to_string(),
        }
        .category(),
        KernelErrorCategory::UserActionable
    );
    assert_eq!(
        HostApiError::PermissionStoreUnavailable.category(),
        KernelErrorCategory::Bug
    );
    assert_eq!(
        HostApiError::PermissionDenied {
            actor: "user-1".to_string(),
            action: "write".to_string(),
            resource_type: "conversation".to_string(),
            resource_id: "conv-1".to_string(),
        }
        .category(),
        KernelErrorCategory::UserActionable
    );
    assert_eq!(
        HostApiError::KernelOperationFailed {
            reason: "storage unavailable".to_string(),
        }
        .category(),
        KernelErrorCategory::External
    );
}

#[test]
fn host_api_error_response_contains_stable_category_code_and_message() {
    let error = HostApiError::PermissionDenied {
        actor: "user-1".to_string(),
        action: "admin".to_string(),
        resource_type: "conversation".to_string(),
        resource_id: "conv-1".to_string(),
    };

    let response = HostApiErrorResponse::from(&error);

    assert_eq!(response.category, KernelErrorCategory::UserActionable);
    assert_eq!(response.code, "permission_denied");
    assert_eq!(
        response.message,
        "permission denied: user-1 cannot admin conversation:conv-1"
    );

    let json = serde_json::to_value(response).unwrap();
    assert_eq!(json["category"], "user_actionable");
    assert_eq!(json["code"], "permission_denied");
    assert_eq!(
        json["message"],
        "permission denied: user-1 cannot admin conversation:conv-1"
    );
}
