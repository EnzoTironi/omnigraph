#![allow(dead_code)]

use omnigraph_ontology::action::{Action, Mode, Parameter, Predicate, Principal};
use omnigraph_ontology::evidence::{
    Evidence, EvidenceSnapshot, Provenance, Requirement, SnapshotPin,
};
use omnigraph_ontology::model::Versions;
use serde_json::{Value, json};

use std::collections::{BTreeMap, BTreeSet};

pub fn roles(values: &[&str]) -> BTreeSet<String> {
    values.iter().map(|v| (*v).into()).collect()
}

pub fn actor(id: &str, role: &str) -> Principal {
    Principal {
        id: id.into(),
        kind: omnigraph_ontology::action::PrincipalKind::Human,
        client: Some("client:office".into()),
        roles: roles(&[role]),
    }
}

pub fn fact(value: Value, observed: i64, recorded: i64) -> Evidence {
    Evidence::Present {
        value,
        observed_at_ms: observed,
        recorded_at_ms: recorded,
        provenance: Provenance {
            source: "purchasing".into(),
            source_record: "purchase:1".into(),
            producer_version: "sync:v1".into(),
        },
    }
}

pub fn snapshot() -> EvidenceSnapshot {
    EvidenceSnapshot {
        pin: SnapshotPin {
            graph: "graph:office".into(),
            branch: "main".into(),
            commit: "commit:10".into(),
            schema: "schema:1".into(),
        },
        captured_at_ms: 1000,
        items: BTreeMap::from([("eligible".into(), fact(json!(true), 900, 950))]),
    }
}

pub fn action() -> Action {
    Action {
        id: "approve_purchase".into(),
        versions: Versions {
            rule: "purchase:v1".into(),
            logic: "logic:v1".into(),
            model: None,
        },
        mode: Mode::Confirm,
        parameters: BTreeMap::from([(
            "amount".into(),
            Parameter::Integer {
                min: 1,
                max: 10_000,
            },
        )]),
        requirements: BTreeMap::from([(
            "eligible".into(),
            Requirement {
                max_age_ms: 500,
                allow_not_applicable: false,
            },
        )]),
        guards: BTreeMap::from([(
            "purchase_is_eligible".into(),
            Predicate::Equals {
                evidence: "eligible".into(),
                value: json!(true),
            },
        )]),
        proposer_roles: roles(&["buyer"]),
        confirmer_roles: roles(&["manager"]),
        proposal_ttl_ms: 10_000,
    }
}

pub fn params() -> BTreeMap<String, Value> {
    BTreeMap::from([("amount".into(), json!(500))])
}
