use agentos_storage::{AgentOsStorage, FsKnowledgeEngineStore, KnowledgeRecordProjectionKind};
use chrono::{TimeZone, Utc};
use knowledge_entity::{
    EvidenceRefId, KnowledgeActor, KnowledgeAssetBindingConfidence,
    KnowledgeAssetPropertyBindingRecord, KnowledgeAttributeId, KnowledgeAttributeRecord,
    KnowledgeAuditRecord, KnowledgeBindingTarget, KnowledgeEventEnvelope, KnowledgeEventId,
    KnowledgeObjectId, KnowledgeObjectRecord, KnowledgeRelationId, KnowledgeRelationRecord,
    KnowledgeTransactionId,
};
use serde_json::json;

fn ts() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 6, 3, 10, 30, 0).unwrap()
}

#[test]
fn typed_object_attribute_relation_and_binding_records_roundtrip_through_projection_store() {
    let dir = tempfile::tempdir().unwrap();
    let storage = AgentOsStorage::init(dir.path()).unwrap();
    let store = FsKnowledgeEngineStore::for_storage(&storage).unwrap();
    let now = ts();

    let object = KnowledgeObjectRecord::new(
        "obj_mass",
        "质量",
        vec![
            "physical_quantity".to_string(),
            "primitive_object".to_string(),
        ],
        "evt_object_created",
        now,
    );
    let attribute = KnowledgeAttributeRecord {
        record_type: "attribute".to_string(),
        attribute_id: KnowledgeAttributeId("attr_mass_unit".to_string()),
        object_id: KnowledgeObjectId("obj_mass".to_string()),
        attribute_key: "unit".to_string(),
        attribute_type: "physical_unit".to_string(),
        value: json!({ "kind": "string", "data": "kilogram" }),
        constraints: json!({ "unit_symbol": "kg", "dimension": "M" }),
        status: "verified".to_string(),
        confidence: 0.99,
        evidence_refs: vec![EvidenceRefId("ev_si_unit_001".to_string())],
        created_at: now,
        updated_at: now,
        last_event_id: KnowledgeEventId("evt_attribute_verified".to_string()),
    };
    let relation = KnowledgeRelationRecord {
        record_type: "relation".to_string(),
        relation_id: KnowledgeRelationId("rel_mass_inertia".to_string()),
        from_object_id: KnowledgeObjectId("obj_mass".to_string()),
        relation_type: "contributes_to".to_string(),
        to_object_id: KnowledgeObjectId("obj_inertia".to_string()),
        direction: "directed".to_string(),
        relation_attributes: json!({ "strength": "definition_level" }),
        status: "verified".to_string(),
        confidence: 0.97,
        evidence_refs: vec![EvidenceRefId("ev_physics_textbook_001".to_string())],
        created_at: now,
        updated_at: now,
        last_event_id: KnowledgeEventId("evt_relation_verified".to_string()),
    };
    let binding = KnowledgeAssetPropertyBindingRecord::candidate(
        "apb_mass_diagram_unit",
        "asset_mass_diagram",
        KnowledgeBindingTarget::object_attribute("obj_mass", "unit"),
        "document_observation",
        json!({ "kind": "string", "data": "kg" }),
        KnowledgeAssetBindingConfidence::candidate(0.88, 0.91),
        now,
    );

    store
        .replace_projection_records(KnowledgeRecordProjectionKind::Objects, &[object.clone()])
        .unwrap();
    store
        .replace_projection_records(
            KnowledgeRecordProjectionKind::Attributes,
            &[attribute.clone()],
        )
        .unwrap();
    store
        .replace_projection_records(
            KnowledgeRecordProjectionKind::Relations,
            &[relation.clone()],
        )
        .unwrap();
    store
        .replace_projection_records(
            KnowledgeRecordProjectionKind::AssetPropertyBindings,
            &[binding.clone()],
        )
        .unwrap();

    let objects: Vec<KnowledgeObjectRecord> = store
        .read_projection_records(KnowledgeRecordProjectionKind::Objects)
        .unwrap();
    let attributes: Vec<KnowledgeAttributeRecord> = store
        .read_projection_records(KnowledgeRecordProjectionKind::Attributes)
        .unwrap();
    let relations: Vec<KnowledgeRelationRecord> = store
        .read_projection_records(KnowledgeRecordProjectionKind::Relations)
        .unwrap();
    let bindings: Vec<KnowledgeAssetPropertyBindingRecord> = store
        .read_projection_records(KnowledgeRecordProjectionKind::AssetPropertyBindings)
        .unwrap();

    assert_eq!(objects, vec![object]);
    assert_eq!(attributes, vec![attribute]);
    assert_eq!(relations, vec![relation]);
    assert_eq!(bindings, vec![binding]);
}

#[test]
fn typed_event_and_audit_envelopes_roundtrip_through_authoritative_logs() {
    let dir = tempfile::tempdir().unwrap();
    let storage = AgentOsStorage::init(dir.path()).unwrap();
    let store = FsKnowledgeEngineStore::for_storage(&storage).unwrap();
    let now = ts();

    let event = KnowledgeEventEnvelope::new(
        "evt_object_created",
        "object.created",
        "txn_create_mass",
        KnowledgeActor::engine("user:shiwen"),
        json!({
            "object_id": "obj_mass",
            "canonical_name": "质量",
            "object_types": ["physical_quantity", "primitive_object"]
        }),
        now,
    );
    let audit = KnowledgeAuditRecord::query(
        "aud_query_mass",
        "op_object_get_mass",
        KnowledgeActor::llm("llm_gateway:test", "user:shiwen"),
        "object.get",
        "sha256:params",
        1,
        vec!["obj_mass".to_string()],
        now,
    );

    store.append_event(now, &event).unwrap();
    store.append_audit(now, &audit).unwrap();

    let events: Vec<KnowledgeEventEnvelope> = store.read_events(2026, 6).unwrap();
    let audits: Vec<KnowledgeAuditRecord> = store.read_audit(2026, 6).unwrap();

    assert_eq!(events, vec![event]);
    assert_eq!(audits, vec![audit]);
    assert_eq!(
        events[0].transaction_id,
        KnowledgeTransactionId("txn_create_mass".to_string())
    );
}
