use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};
use std::thread;

use chrono::Utc;
use enterprise_permission_core::{
    EnterpriseRole, EnterpriseUserId, EnterpriseUserLifecycle, EnterpriseUserStatus,
    PermissionAction, PermissionDecision, PermissionGrant, PermissionStore, ResourceId,
    ResourceType,
};

fn grant(grant_id: &str, user_id: &EnterpriseUserId, resource_id: &str) -> PermissionGrant {
    PermissionGrant {
        grant_id: grant_id.to_string(),
        user_id: user_id.clone(),
        role: EnterpriseRole::User,
        resource_type: ResourceType::KnowledgeBase,
        resource_id: ResourceId(resource_id.to_string()),
        actions: vec![PermissionAction::Read],
        granted_at: Utc::now(),
        expires_at: None,
        revoked: false,
    }
}

#[test]
fn concurrent_permission_grant_revoke_and_offboarding_keeps_denial_invariant() {
    let user_id = EnterpriseUserId::from("user-1");
    let store = Arc::new(Mutex::new(PermissionStore::new()));

    let mut workers = Vec::new();
    for idx in 0..24 {
        let store = store.clone();
        let user_id = user_id.clone();
        workers.push(thread::spawn(move || {
            let mut store = store.lock().unwrap();
            store.add_grant(grant(&format!("grant-{idx:02}"), &user_id, "kb-main"));
        }));
    }

    for worker in workers {
        worker.join().unwrap();
    }

    assert_eq!(
        store.lock().unwrap().check(
            &user_id,
            &ResourceType::KnowledgeBase,
            &ResourceId("kb-main".to_string()),
            &PermissionAction::Read,
            Utc::now(),
        ),
        PermissionDecision::Allow
    );

    let mut workers = Vec::new();
    for lifecycle in [
        EnterpriseUserLifecycle::Suspended,
        EnterpriseUserLifecycle::Disabled,
        EnterpriseUserLifecycle::Offboarded,
    ] {
        let store = store.clone();
        let user_id = user_id.clone();
        workers.push(thread::spawn(move || {
            let mut store = store.lock().unwrap();
            store.set_user_lifecycle(EnterpriseUserStatus {
                user_id,
                lifecycle,
                reason: Some(format!("transition to {lifecycle}")),
                changed_at: Utc::now(),
            })
        }));
    }

    let results = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect::<Vec<_>>();
    assert!(results.iter().any(|changed| *changed));

    let mut store = store.lock().unwrap();
    assert_eq!(
        store.get_user_lifecycle(&user_id),
        EnterpriseUserLifecycle::Offboarded
    );
    assert_eq!(
        store.check(
            &user_id,
            &ResourceType::KnowledgeBase,
            &ResourceId("kb-main".to_string()),
            &PermissionAction::Read,
            Utc::now(),
        ),
        PermissionDecision::Deny
    );
    assert_eq!(store.revoke_all_grants_for_user(&user_id), 24);
    let revoked_ids = store
        .get_grants_for_user(&user_id)
        .into_iter()
        .filter(|grant| grant.revoked)
        .map(|grant| grant.grant_id.clone())
        .collect::<BTreeSet<_>>();
    assert_eq!(revoked_ids.len(), 24);
}
