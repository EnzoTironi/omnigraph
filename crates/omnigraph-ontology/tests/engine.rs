#![cfg(feature = "engine")]

mod support;

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use omnigraph::Session;
use omnigraph::db::Omnigraph;
use omnigraph::loader::LoadMode;
use omnigraph_compiler::ir::ParamMap;
use omnigraph_compiler::query::ast::Literal;
use omnigraph_ontology::action::Outcome;
use omnigraph_ontology::engine::{
    Binding, Clock, EngineAction, EvidenceColumn, Query, RECORD_SCHEMA,
};
use serde_json::{Value, json};
use support::*;

const PURCHASE_SCHEMA: &str = r#"
node Purchase {
    id: String @key
    eligible: Bool
    observed: I64
    recorded: I64
    approved: Bool
    amount: I64
}
"#;

const EVIDENCE: &str = r#"
query evidence($amount: I64) {
    match { $p: Purchase { id: "purchase:1" } }
    return { $p.eligible as eligible, $p.observed as observed,
             $p.recorded as recorded, $p.id as source_id }
}
"#;

const EFFECT: &str = r#"
query approve($amount: I64, $og_id: String, $og_record: String, $og_consumes: String?) {
    update Purchase set { approved: true, amount: $amount } where id = "purchase:1"
    insert OntologyDecision { decision_id: $og_id, record: $og_record, consumes: $og_consumes }
}
"#;

struct TestClock(AtomicI64);
impl Clock for TestClock {
    fn now_ms(&self) -> omnigraph_ontology::Result<i64> {
        Ok(self.0.load(Ordering::SeqCst))
    }
}

fn binding() -> Binding {
    Binding {
        evidence: Query {
            source: EVIDENCE.into(),
            name: "evidence".into(),
        },
        effect: Query {
            source: EFFECT.into(),
            name: "approve".into(),
        },
        columns: BTreeMap::from([(
            "eligible".into(),
            EvidenceColumn {
                value: "eligible".into(),
                observed_at_ms: "observed".into(),
                recorded_at_ms: "recorded".into(),
                source_record: "source_id".into(),
                source: "purchasing".into(),
                producer_version: "sync:v1".into(),
            },
        )]),
    }
}

async fn setup() -> (tempfile::TempDir, Session, EngineAction, Arc<TestClock>) {
    let dir = tempfile::tempdir().unwrap();
    let schema = format!("{PURCHASE_SCHEMA}\n{RECORD_SCHEMA}");
    let db = Omnigraph::init(dir.path().to_str().unwrap(), &schema)
        .await
        .unwrap();
    let session = Session::from_defaults(Arc::new(db), Default::default());
    session.load_jsonl(r#"{"type":"Purchase","data":{"id":"purchase:1","eligible":true,"observed":900,"recorded":950,"approved":false,"amount":0}}"#,
        LoadMode::Append).await.unwrap();
    let clock = Arc::new(TestClock(AtomicI64::new(1000)));
    let runtime = EngineAction::new(
        session.clone(),
        action(),
        binding(),
        "main",
        "service:ontology",
        clock.clone(),
    )
    .unwrap();
    (dir, session, runtime, clock)
}

async fn rows(session: &Session, query: &str) -> Value {
    session
        .query("main", query, "q", &ParamMap::new())
        .await
        .unwrap()
        .to_rust_json()
        .unwrap()
}

#[tokio::test]
async fn approve_and_audit_publish_together_and_replay_survives_reopen() {
    let (dir, session, runtime, clock) = setup().await;
    let proposal = runtime
        .submit("proposal:1", actor("alice", "buyer"), params())
        .await
        .unwrap();
    assert_eq!(proposal.decision.outcome, Outcome::Review);
    let purchase = "query q() { match { $p: Purchase } return { $p.approved as approved, $p.amount as amount } }";
    assert_eq!(
        rows(&session, purchase).await,
        json!([{"approved":false,"amount":0}])
    );
    let commits = session.list_commits(None).await.unwrap().len();
    let approved = runtime
        .confirm("decision:1", "proposal:1", actor("bob", "manager"))
        .await
        .unwrap();
    assert_eq!(approved.decision.outcome, Outcome::Allowed);
    assert_eq!(session.list_commits(None).await.unwrap().len(), commits + 1);
    assert_eq!(
        rows(&session, purchase).await,
        json!([{"approved":true,"amount":500}])
    );
    let records = rows(
        &session,
        "query q() { match { $d: OntologyDecision } return { $d.decision_id as id } }",
    )
    .await;
    assert_eq!(records.as_array().unwrap().len(), 2);
    let reopened = Omnigraph::open(dir.path().to_str().unwrap()).await.unwrap();
    let reopened = Session::from_defaults(Arc::new(reopened), Default::default());
    let runtime = EngineAction::new(
        reopened.clone(),
        action(),
        binding(),
        "main",
        "service:ontology",
        clock,
    )
    .unwrap();
    let replayed = runtime
        .confirm("decision:1", "proposal:1", actor("bob", "manager"))
        .await
        .unwrap();
    assert!(replayed.replayed);
    assert_eq!(replayed.decision, approved.decision);
    assert_eq!(
        reopened.list_commits(None).await.unwrap().len(),
        commits + 1
    );
}

#[tokio::test]
async fn duplicate_confirmation_cannot_consume_the_proposal_twice() {
    let (_dir, session, runtime, _clock) = setup().await;
    runtime
        .submit("p1", actor("alice", "buyer"), params())
        .await
        .unwrap();
    runtime
        .confirm("d1", "p1", actor("bob", "manager"))
        .await
        .unwrap();
    let before = session.list_commits(None).await.unwrap().len();
    assert!(
        runtime
            .confirm("d2", "p1", actor("carol", "manager"))
            .await
            .is_err()
    );
    assert_eq!(session.list_commits(None).await.unwrap().len(), before);
    let records = rows(
        &session,
        "query q() { match { $d: OntologyDecision } return { $d.decision_id as id } }",
    )
    .await;
    assert_eq!(records.as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn refusal_is_durable_and_changes_no_business_rows() {
    let (_dir, session, runtime, clock) = setup().await;
    runtime
        .submit("p1", actor("alice", "buyer"), params())
        .await
        .unwrap();
    clock.0.store(1401, Ordering::SeqCst);
    let denied = runtime
        .confirm("d1", "p1", actor("bob", "manager"))
        .await
        .unwrap();
    assert_eq!(denied.decision.outcome, Outcome::Denied);
    assert!(denied.commit_id.is_some());
    assert_eq!(
        rows(
            &session,
            "query q() { match { $p: Purchase } return { $p.approved as approved } }"
        )
        .await,
        json!([{"approved":false}])
    );
    let replayed = runtime
        .confirm("d1", "p1", actor("bob", "manager"))
        .await
        .unwrap();
    assert!(replayed.replayed);
    assert_eq!(replayed.decision, denied.decision);
}

#[tokio::test]
async fn invalid_effect_rolls_back_business_and_dossier_visibility() {
    let (_dir, session, _runtime, clock) = setup().await;
    let mut bad = binding();
    // A required property receives null. Native execution must not publish
    // either the Purchase update or the appended audit record.
    bad.effect.source = EFFECT.replace("amount: $amount", "amount: $og_consumes");
    let runtime = EngineAction::new(
        session.clone(),
        action(),
        bad,
        "main",
        "service:ontology",
        clock,
    )
    .unwrap();
    runtime
        .submit("p1", actor("alice", "buyer"), params())
        .await
        .unwrap();
    let before = session.list_commits(None).await.unwrap().len();
    assert!(
        runtime
            .confirm("d1", "p1", actor("bob", "manager"))
            .await
            .is_err()
    );
    assert_eq!(session.list_commits(None).await.unwrap().len(), before);
    assert_eq!(
        rows(
            &session,
            "query q() { match { $p: Purchase } return { $p.approved as approved } }"
        )
        .await,
        json!([{"approved":false}])
    );
    let ids = rows(
        &session,
        "query q() { match { $d: OntologyDecision } return { $d.decision_id as id } }",
    )
    .await;
    assert_eq!(ids, json!([{"id":"p1"}]));
}

#[tokio::test]
async fn id_reuse_and_unauthorized_calls_do_not_write() {
    let (_dir, session, runtime, _clock) = setup().await;
    let before = session.list_commits(None).await.unwrap().len();
    assert!(
        runtime
            .submit("p1", actor("eve", "visitor"), params())
            .await
            .is_err()
    );
    assert_eq!(session.list_commits(None).await.unwrap().len(), before);
    runtime
        .submit("p1", actor("alice", "buyer"), params())
        .await
        .unwrap();
    let before = session.list_commits(None).await.unwrap().len();
    assert!(
        runtime
            .submit(
                "p1",
                actor("alice", "buyer"),
                BTreeMap::from([("amount".into(), json!(600))])
            )
            .await
            .is_err()
    );
    assert_eq!(session.list_commits(None).await.unwrap().len(), before);
}

#[tokio::test]
async fn registration_requires_strict_audit_schema_and_host_bound_append() {
    let (_dir, session, _runtime, clock) = setup().await;
    for source in [
        EFFECT.replace("record: $og_record", "record: \"forged\""),
        EFFECT.replace("insert OntologyDecision", "insert Purchase"),
        EFFECT.replace("update Purchase", "delete Purchase"),
    ] {
        let mut bad = binding();
        bad.effect.source = source;
        assert!(
            EngineAction::new(
                session.clone(),
                action(),
                bad,
                "main",
                "service:ontology",
                clock.clone()
            )
            .is_err()
        );
    }
}

#[tokio::test]
async fn observed_read_binds_schema_and_head_to_its_rows() {
    let (_dir, session, _runtime, _clock) = setup().await;
    let query = "query q() { match { $p: Purchase } return { $p.amount as amount } }";
    let observed = session
        .query_observed("main", query, "q", &ParamMap::new())
        .await
        .unwrap();
    assert_eq!(
        observed.schema_ir_hash,
        session.schema_contract_digest().schema_ir_hash
    );
    session.mutate("main", "query change($amount: I64) { update Purchase set { amount: $amount } where id = \"purchase:1\" }",
        "change", &ParamMap::from([("amount".into(), Literal::Integer(7))])).await.unwrap();
    assert_eq!(
        observed.result.to_rust_json().unwrap(),
        json!([{"amount":0}])
    );
    let next = session
        .query_observed("main", query, "q", &ParamMap::new())
        .await
        .unwrap();
    assert_ne!(observed.graph_commit_id, next.graph_commit_id);
    assert_eq!(observed.schema_ir_hash, next.schema_ir_hash);
}

#[tokio::test]
async fn concurrent_confirmations_have_only_one_business_publication() {
    let (_dir, session, runtime, _clock) = setup().await;
    runtime
        .submit("p1", actor("alice", "buyer"), params())
        .await
        .unwrap();
    let before = session.list_commits(None).await.unwrap().len();
    let (a, b) = tokio::join!(
        runtime.confirm("d1", "p1", actor("bob", "manager")),
        runtime.confirm("d2", "p1", actor("carol", "manager"))
    );
    assert_eq!(usize::from(a.is_ok()) + usize::from(b.is_ok()), 1);
    assert_eq!(session.list_commits(None).await.unwrap().len(), before + 1);
    let ids = rows(
        &session,
        "query q() { match { $d: OntologyDecision } return { $d.decision_id as id } }",
    )
    .await;
    assert_eq!(ids.as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn native_policy_denies_direct_business_writes_by_action_callers() {
    let (_dir, session, _runtime, clock) = setup().await;
    let policy = omnigraph_policy::PolicyEngine::load_graph_from_source(
        r#"
version: 1
groups:
  service: ["service:ontology"]
  humans: [alice, bob]
rules:
  - id: service-writes
    allow:
      actors: {group: service}
      actions: [change]
      branch_scope: any
  - id: humans-read
    allow:
      actors: {group: humans}
      actions: [read]
      branch_scope: any
"#,
        "office",
    )
    .unwrap();
    let view = session
        .with_runtime_bindings(Some(Arc::new(policy)), None, Default::default())
        .unwrap();
    let secured = Session::from_defaults(Arc::new(view), Default::default());
    let before = secured.list_commits(None).await.unwrap().len();
    let direct = secured
        .mutate_as(
            "main",
            "query bypass() { update Purchase set { approved: true } where id = \"purchase:1\" }",
            "bypass",
            &ParamMap::new(),
            Some("alice"),
        )
        .await;
    assert!(direct.is_err());
    assert_eq!(secured.list_commits(None).await.unwrap().len(), before);
    let runtime = EngineAction::new(
        secured.clone(),
        action(),
        binding(),
        "main",
        "service:ontology",
        clock,
    )
    .unwrap();
    runtime
        .submit("p1", actor("alice", "buyer"), params())
        .await
        .unwrap();
    let decision = runtime
        .confirm("d1", "p1", actor("bob", "manager"))
        .await
        .unwrap();
    assert_eq!(decision.decision.outcome, Outcome::Allowed);
    assert_eq!(decision.decision.proposed_by.id, "alice");
    assert_eq!(decision.decision.confirmed_by.unwrap().id, "bob");
}

#[tokio::test]
async fn automatic_action_publishes_once_and_resolves_the_same_request() {
    let (_dir, session, _runtime, clock) = setup().await;
    let mut automatic = action();
    automatic.mode = omnigraph_ontology::action::Mode::Automatic;
    let runtime = EngineAction::new(
        session.clone(),
        automatic,
        binding(),
        "main",
        "service:ontology",
        clock,
    )
    .unwrap();
    let before = session.list_commits(None).await.unwrap().len();
    let first = runtime
        .submit("d1", actor("alice", "buyer"), params())
        .await
        .unwrap();
    assert_eq!(first.decision.outcome, Outcome::Allowed);
    assert!(first.commit_id.is_some());
    let replay = runtime
        .submit("d1", actor("alice", "buyer"), params())
        .await
        .unwrap();
    assert!(replay.replayed);
    assert_eq!(replay.decision, first.decision);
    assert_eq!(session.list_commits(None).await.unwrap().len(), before + 1);
}

#[tokio::test]
async fn changed_schema_refuses_registered_actions_before_any_effect() {
    let (_dir, session, runtime, _clock) = setup().await;
    let old = session.schema_contract_digest().schema_ir_hash;
    let source = session.schema_source();
    session
        .apply_schema(&format!("{source}\nnode Extra {{ name: String @key }}\n"))
        .await
        .unwrap();
    let before = session.list_commits(None).await.unwrap().len();
    assert_ne!(old, session.schema_contract_digest().schema_ir_hash);
    assert!(
        runtime
            .submit("p1", actor("alice", "buyer"), params())
            .await
            .is_err()
    );
    assert_eq!(session.list_commits(None).await.unwrap().len(), before);
    let observed = session
        .query_observed(
            "main",
            "query q() { match { $p: Purchase } return { $p.amount } }",
            "q",
            &ParamMap::new(),
        )
        .await
        .unwrap();
    assert_eq!(
        observed.schema_ir_hash,
        session.schema_contract_digest().schema_ir_hash
    );
}
