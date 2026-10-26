use std::collections::BTreeMap;
use std::sync::Arc;

use omnigraph::Session;
use omnigraph::db::{InitOptions, Omnigraph};
use omnigraph::loader::LoadMode;
use omnigraph_ontology::action::{Action, Mode, Outcome, Parameter, Predicate, Principal};
use omnigraph_ontology::engine::{
    Binding, Clock, EngineAction, EvidenceColumn, Query, RECORD_SCHEMA,
};
use omnigraph_ontology::evidence::Requirement;
use omnigraph_ontology::model::Versions;
use serde_json::json;

struct FixedClock;
impl Clock for FixedClock {
    fn now_ms(&self) -> omnigraph_ontology::Result<i64> {
        Ok(1000)
    }
}

fn actor(id: &str, role: &str) -> Principal {
    Principal {
        id: id.into(),
        kind: omnigraph_ontology::action::PrincipalKind::Human,
        client: None,
        roles: [role.into()].into(),
    }
}

fn runtime(session: Session) -> EngineAction {
    let action = Action {
        id: "approve_case".into(),
        versions: Versions {
            rule: "rule:1".into(),
            logic: "logic:1".into(),
            model: None,
        },
        mode: Mode::Confirm,
        parameters: BTreeMap::from([("approved".into(), Parameter::Bool)]),
        requirements: BTreeMap::from([(
            "eligible".into(),
            Requirement {
                max_age_ms: 500,
                allow_not_applicable: false,
            },
        )]),
        guards: BTreeMap::from([(
            "eligible".into(),
            Predicate::Equals {
                evidence: "eligible".into(),
                value: json!(true),
            },
        )]),
        proposer_roles: ["proposer".into()].into(),
        confirmer_roles: ["confirmer".into()].into(),
        proposal_ttl_ms: 1000,
    };
    let binding = Binding {
        evidence: Query {
            name: "evidence".into(),
            source: r#"
query evidence($approved: Bool) {
  match { $c: Case { id: "one" } }
  return { $c.eligible as value, $c.observed as observed, $c.recorded as recorded, $c.id as source }
}
"#
            .into(),
        },
        effect: Query {
            name: "approve".into(),
            source: r#"
query approve($approved: Bool, $og_id: String, $og_record: String, $og_consumes: String?) {
  update Case set { approved: $approved } where id = "one"
  insert OntologyDecision { decision_id: $og_id, record: $og_record, consumes: $og_consumes }
}
"#
            .into(),
        },
        columns: BTreeMap::from([(
            "eligible".into(),
            EvidenceColumn {
                value: "value".into(),
                observed_at_ms: "observed".into(),
                recorded_at_ms: "recorded".into(),
                source_record: "source".into(),
                source: "case_registry".into(),
                producer_version: "sync:1".into(),
            },
        )]),
    };
    EngineAction::new(
        session,
        action,
        binding,
        "main",
        "ontology-service",
        Arc::new(FixedClock),
    )
    .unwrap()
}

pub(super) fn rows(replay: &str) -> String {
    struct OntologyCost;
    impl omnigraph_dst::UniverseScenario<omnigraph_dst::memory::MemoryStorage> for OntologyCost {
        type Output = ();

        async fn run(&self, resources: &mut omnigraph_dst::memory::MemoryStorage, _: u64) {
            omnigraph_dst::cost::set_label("_ontology_setup");
            let schema = format!(
                r#"
node Case {{
  id: String @key
  eligible: Bool
  observed: I64
  recorded: I64
  approved: Bool
}}
{RECORD_SCHEMA}
"#
            );
            let db = Omnigraph::init_with_storage(
                &resources.root,
                &schema,
                resources.adapter.clone(),
                InitOptions::default(),
            )
            .await
            .unwrap();
            let session = Session::from_defaults(Arc::new(db), Default::default());
            session.load_jsonl(r#"{"type":"Case","data":{"id":"one","eligible":true,"observed":900,"recorded":950,"approved":false}}"#,
                LoadMode::Append).await.unwrap();
            drop(session);
            let db = Omnigraph::open_with_storage(&resources.root, resources.adapter.clone())
                .await
                .unwrap();
            let service = runtime(Session::from_defaults(Arc::new(db), Default::default()));
            omnigraph_dst::cost::set_label("OntologyPropose");
            let proposal = service
                .submit(
                    "p1",
                    actor("alice", "proposer"),
                    BTreeMap::from([("approved".into(), json!(true))]),
                )
                .await
                .unwrap();
            omnigraph_dst::cost::set_label("_ontology_setup");
            assert_eq!(proposal.decision.outcome, Outcome::Review);
            drop(service);
            let db = Omnigraph::open_with_storage(&resources.root, resources.adapter.clone())
                .await
                .unwrap();
            let service = runtime(Session::from_defaults(Arc::new(db), Default::default()));
            omnigraph_dst::cost::set_label("OntologyConfirm");
            let confirmed = service
                .confirm("d1", "p1", actor("bob", "confirmer"))
                .await
                .unwrap();
            omnigraph_dst::cost::set_label("_ontology_verify");
            assert_eq!(confirmed.decision.outcome, Outcome::Allowed);
            drop(service);
            let db = Omnigraph::open_with_storage(&resources.root, resources.adapter.clone())
                .await
                .unwrap();
            let service = runtime(Session::from_defaults(Arc::new(db), Default::default()));
            omnigraph_dst::cost::set_label("OntologyReplay");
            let replayed = service
                .confirm("d1", "p1", actor("bob", "confirmer"))
                .await
                .unwrap();
            omnigraph_dst::cost::set_label("_ontology_verify");
            assert!(replayed.replayed);
            assert_eq!(replayed.decision, confirmed.decision);
        }
    }
    super::labeled_cost_rows_of(
        format!("shared-memory://dst-ontology-cost-{replay}"),
        &OntologyCost,
        &["OntologyPropose", "OntologyConfirm", "OntologyReplay"],
    )
}
