use std::collections::BTreeMap;

use omnigraph_ontology::access::{Access, Permission};
use omnigraph_ontology::action::{Action, Check, Mode, Outcome, Parameter, Predicate};
use omnigraph_ontology::delivery::{Delivery, DeliveryState, Intent};
use omnigraph_ontology::digest;
use omnigraph_ontology::evidence::{Evidence, Readiness, Requirement};
use omnigraph_ontology::model::BusinessVersion;
use serde_json::{Value, json};

mod support;
use support::*;

#[test]
fn proposal_confirmation_preserves_both_actor_and_version_chains() {
    let action = action();
    let proposal = action
        .propose("p1", actor("alice", "buyer"), params(), snapshot(), 1000)
        .unwrap();
    assert_eq!(proposal.outcome, Outcome::Review);
    assert!(!proposal.allows_effects());
    let mut current = snapshot();
    current.pin.commit = "commit:11".into();
    current.captured_at_ms = 1100;
    let decision = action
        .confirm("d1", &proposal, actor("bob", "manager"), current, 1100)
        .unwrap();
    assert!(decision.allows_effects());
    assert_eq!(decision.previous.as_deref(), Some("p1"));
    assert_eq!(decision.proposed_by.id, "alice");
    assert_eq!(decision.confirmed_by.unwrap().id, "bob");
    assert_eq!(decision.proposed_at_ms, 1000);
    assert_eq!(decision.evaluated_at_ms, 1100);
    assert_eq!(proposal.inputs.pin.commit, "commit:10");
    assert_eq!(decision.inputs.pin.commit, "commit:11");
    assert_eq!(decision.versions, proposal.versions);
}

#[test]
fn same_actor_cannot_confirm_through_another_client() {
    let action = action();
    let proposal = action
        .propose("p1", actor("alice", "buyer"), params(), snapshot(), 1000)
        .unwrap();
    let mut confirmer = actor("alice", "manager");
    confirmer.client = Some("client:agent".into());
    let denied = action
        .confirm("d1", &proposal, confirmer, snapshot(), 1000)
        .unwrap();
    assert_eq!(denied.outcome, Outcome::Denied);
    assert_eq!(denied.checks["policy:separation"], Check::Predicate(false));
}

#[test]
fn a_manager_role_does_not_turn_an_agent_into_a_human_confirmer() {
    let action = action();
    let proposal = action
        .propose("p1", actor("alice", "buyer"), params(), snapshot(), 1000)
        .unwrap();
    let mut agent = actor("agent:reviewer", "manager");
    agent.kind = omnigraph_ontology::action::PrincipalKind::Agent;
    let decision = action
        .confirm("d1", &proposal, agent, snapshot(), 1000)
        .unwrap();
    assert_eq!(decision.outcome, Outcome::Denied);
    assert_eq!(decision.checks["policy:confirmer"], Check::Predicate(false));
    let mut manual = action;
    manual.mode = Mode::Manual;
    let mut agent = actor("agent:buyer", "buyer");
    agent.kind = omnigraph_ontology::action::PrincipalKind::Agent;
    assert_eq!(
        manual
            .propose("p2", agent, params(), snapshot(), 1000)
            .unwrap()
            .outcome,
        Outcome::Denied
    );
}

#[test]
fn confirmation_rechecks_business_state_and_clock_without_graph_movement() {
    let action = action();
    let proposal = action
        .propose("p1", actor("alice", "buyer"), params(), snapshot(), 1000)
        .unwrap();
    let stale = action
        .confirm("d1", &proposal, actor("bob", "manager"), snapshot(), 1401)
        .unwrap();
    assert_eq!(stale.outcome, Outcome::Denied);
    assert_eq!(
        stale.checks["evidence:eligible"],
        Check::Readiness(Readiness::Stale)
    );
    assert_eq!(stale.inputs.pin.commit, proposal.inputs.pin.commit);
    let mut changed = snapshot();
    changed
        .items
        .insert("eligible".into(), fact(json!(false), 900, 950));
    let denied = action
        .confirm("d2", &proposal, actor("bob", "manager"), changed, 1000)
        .unwrap();
    assert_eq!(
        denied.checks["guard:purchase_is_eligible"],
        Check::Predicate(false)
    );
}

#[test]
fn ttl_and_version_changes_fail_closed() {
    let mut action = action();
    let proposal = action
        .propose("p1", actor("alice", "buyer"), params(), snapshot(), 1000)
        .unwrap();
    action.proposal_ttl_ms = 10_000;
    let expired = action
        .confirm("d1", &proposal, actor("bob", "manager"), snapshot(), 11_000)
        .unwrap();
    assert_eq!(expired.checks["policy:lifetime"], Check::Predicate(false));
    action.versions.rule = "purchase:v2".into();
    assert!(
        action
            .confirm("d2", &proposal, actor("bob", "manager"), snapshot(), 1000)
            .is_err()
    );
}

#[test]
fn confirmation_cannot_change_scope_schema_or_reuse_record_id() {
    let action = action();
    let proposal = action
        .propose("p1", actor("alice", "buyer"), params(), snapshot(), 1000)
        .unwrap();
    for field in ["graph", "branch", "schema"] {
        let mut changed = snapshot();
        match field {
            "graph" => changed.pin.graph = "other".into(),
            "branch" => changed.pin.branch = "other".into(),
            _ => changed.pin.schema = "other".into(),
        }
        assert!(
            action
                .confirm("d1", &proposal, actor("bob", "manager"), changed, 1000)
                .is_err()
        );
    }
    assert!(
        action
            .confirm("p1", &proposal, actor("bob", "manager"), snapshot(), 1000)
            .is_err()
    );
}

#[test]
fn a_stored_proposal_with_inconsistent_guard_or_actor_evidence_is_refused() {
    let action = action();
    let proposal = action
        .propose("p1", actor("alice", "buyer"), params(), snapshot(), 1000)
        .unwrap();
    let mut corrupt = proposal.clone();
    corrupt
        .checks
        .insert("guard:purchase_is_eligible".into(), Check::Predicate(false));
    assert!(
        action
            .confirm("d1", &corrupt, actor("bob", "manager"), snapshot(), 1000)
            .is_err()
    );
    let mut corrupt = proposal;
    corrupt.proposed_by.roles = roles(&["visitor"]);
    assert!(
        action
            .confirm("d2", &corrupt, actor("bob", "manager"), snapshot(), 1000)
            .is_err()
    );
}

#[test]
fn unknown_missing_and_not_applicable_have_distinct_verdicts() {
    for (item, readiness, outcome) in [
        (Evidence::Missing, Readiness::Missing, Outcome::Denied),
        (Evidence::Unknown, Readiness::Unknown, Outcome::Unknown),
        (
            Evidence::NotApplicable {
                reason: "exempt".into(),
            },
            Readiness::NotApplicable,
            Outcome::Denied,
        ),
    ] {
        let mut inputs = snapshot();
        inputs.items.insert("eligible".into(), item);
        let decision = action()
            .propose("p1", actor("alice", "buyer"), params(), inputs, 1000)
            .unwrap();
        assert_eq!(decision.outcome, outcome);
        assert_eq!(
            decision.checks["evidence:eligible"],
            Check::Readiness(readiness)
        );
    }
}

#[test]
fn unknown_evidence_stays_unknown_at_confirmation() {
    let action = action();
    let proposal = action
        .propose("p1", actor("alice", "buyer"), params(), snapshot(), 1000)
        .unwrap();
    let mut current = snapshot();
    current.items.insert("eligible".into(), Evidence::Unknown);
    let result = action
        .confirm("d1", &proposal, actor("bob", "manager"), current, 1000)
        .unwrap();
    assert_eq!(result.outcome, Outcome::Unknown);
    assert!(!result.allows_effects());
}

#[test]
fn a_known_guard_denial_is_not_masked_by_other_unknown_evidence() {
    let mut action = action();
    action.requirements.insert(
        "other".into(),
        Requirement {
            max_age_ms: 500,
            allow_not_applicable: false,
        },
    );
    let mut inputs = snapshot();
    inputs
        .items
        .insert("eligible".into(), fact(json!(false), 900, 950));
    inputs.items.insert("other".into(), Evidence::Unknown);
    let result = action
        .propose("p1", actor("alice", "buyer"), params(), inputs, 1000)
        .unwrap();
    assert_eq!(result.outcome, Outcome::Denied);
}

#[test]
fn readiness_uses_business_time_and_checks_provenance_and_overflow_boundaries() {
    let requirement = Requirement {
        max_age_ms: 500,
        allow_not_applicable: false,
    };
    assert_eq!(
        requirement.check(&fact(json!(true), 100, 999), 1000),
        Readiness::Stale
    );
    assert_eq!(
        requirement.check(&fact(json!(true), 500, 999), 1000),
        Readiness::Ready
    );
    assert_eq!(
        requirement.check(&fact(json!(true), 1001, 1002), 1000),
        Readiness::Invalid
    );
    assert_eq!(
        requirement.check(&fact(Value::Null, 900, 950), 1000),
        Readiness::Invalid
    );
    assert_eq!(
        requirement.check(&fact(json!(1), i64::MIN, i64::MIN), i64::MAX),
        Readiness::Stale
    );
    let mut invalid = fact(json!(1), 900, 950);
    if let Evidence::Present { provenance, .. } = &mut invalid {
        provenance.source.clear();
    }
    assert_eq!(requirement.check(&invalid, 1000), Readiness::Invalid);
    let mut impossible = snapshot();
    impossible
        .items
        .insert("eligible".into(), fact(json!(true), 999, 1001));
    assert!(impossible.validate(1002).is_err());
}

#[test]
fn unauthorized_actors_get_no_guard_results() {
    let action = action();
    let denied = action
        .propose("p1", actor("eve", "visitor"), params(), snapshot(), 1000)
        .unwrap();
    assert_eq!(denied.outcome, Outcome::Denied);
    assert!(denied.checks.keys().all(|key| key.starts_with("policy:")));
    let proposal = action
        .propose("p2", actor("alice", "buyer"), params(), snapshot(), 1000)
        .unwrap();
    let denied = action
        .confirm("d1", &proposal, actor("eve", "visitor"), snapshot(), 1000)
        .unwrap();
    assert!(denied.checks.keys().all(|key| key.starts_with("policy:")));
}

#[test]
fn modes_and_parameters_are_explicit_and_bounded() {
    for (mode, expected) in [
        (Mode::Manual, Outcome::Review),
        (Mode::Automatic, Outcome::Allowed),
        (Mode::Confirm, Outcome::Review),
        (Mode::Disabled, Outcome::Denied),
    ] {
        let mut action = action();
        action.mode = mode;
        assert_eq!(
            action
                .propose("p1", actor("alice", "buyer"), params(), snapshot(), 1000)
                .unwrap()
                .outcome,
            expected
        );
    }
    for bad in [
        json!(0),
        json!(10001),
        json!(500.0),
        json!("500"),
        json!(null),
        json!(u64::MAX),
    ] {
        assert!(
            action()
                .propose(
                    "p1",
                    actor("alice", "buyer"),
                    BTreeMap::from([("amount".into(), bad)]),
                    snapshot(),
                    1000
                )
                .is_err()
        );
    }
    let mut extra = params();
    extra.insert("actor".into(), json!("admin"));
    assert!(
        action()
            .propose("p1", actor("alice", "buyer"), extra, snapshot(), 1000)
            .is_err()
    );
    assert!(
        action()
            .propose(
                "p1",
                actor("alice", "buyer"),
                BTreeMap::new(),
                snapshot(),
                1000
            )
            .is_err()
    );
}

#[test]
fn contract_validation_catches_unbound_guards_empty_roles_and_inverted_ranges() {
    let mut bad = action();
    bad.requirements.clear();
    assert!(bad.validate().is_err());
    let mut bad = action();
    bad.proposer_roles.clear();
    assert!(bad.validate().is_err());
    let mut bad = action();
    bad.confirmer_roles.clear();
    assert!(bad.validate().is_err());
    let mut bad = action();
    bad.proposal_ttl_ms = u64::MAX;
    assert!(bad.validate().is_err());
    let mut bad = action();
    bad.parameters
        .insert("amount".into(), Parameter::Integer { min: 5, max: 1 });
    assert!(bad.validate().is_err());
    let mut bad = action();
    bad.guards.insert(
        "match".into(),
        Predicate::EqualsParameter {
            evidence: "eligible".into(),
            parameter: "missing".into(),
        },
    );
    assert!(bad.validate().is_err());
}

#[test]
fn serialization_is_strict_and_contract_digest_covers_security_semantics() {
    let value = serde_json::to_value(action()).unwrap();
    let parsed: Action = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(digest(&parsed).unwrap(), digest(&action()).unwrap());
    let mut changed = action();
    changed.confirmer_roles.insert("agent".into());
    assert_ne!(digest(&changed).unwrap(), digest(&action()).unwrap());
    let mut unknown = value;
    unknown["ignore_security"] = json!(true);
    assert!(serde_json::from_value::<Action>(unknown).is_err());
}

fn access() -> Access {
    let public = Permission {
        read_roles: roles(&["reader", "editor"]),
        write_roles: roles(&["editor"]),
    };
    Access {
        type_permission: public.clone(),
        entities: BTreeMap::from([("purchase:1".into(), public.clone())]),
        properties: BTreeMap::from([
            ("amount".into(), public),
            (
                "private".into(),
                Permission {
                    read_roles: roles(&["finance"]),
                    write_roles: roles(&["finance"]),
                },
            ),
        ]),
    }
}

#[test]
fn view_oracle_is_invariant_under_hidden_values() {
    let access = access();
    let reader = actor("alice", "reader");
    let mut row = BTreeMap::from([
        ("amount".into(), json!(500)),
        ("private".into(), json!("secret")),
    ]);
    let visible = access
        .visible(&reader, "purchase:1", &row)
        .unwrap()
        .unwrap();
    assert_eq!(visible, BTreeMap::from([("amount".into(), json!(500))]));
    row.insert("private".into(), json!("another secret"));
    assert_eq!(
        access
            .visible(&reader, "purchase:1", &row)
            .unwrap()
            .unwrap(),
        visible
    );
    assert!(access.visible(&reader, "hidden:2", &row).unwrap().is_none());
    assert!(!access.allows(&reader, "purchase:1", "undeclared", false));
}

#[test]
fn partial_writes_deny_hidden_properties_without_disclosing_their_names() {
    let access = access();
    let editor = actor("alice", "editor");
    access
        .validate_update(&editor, "purchase:1", &roles(&["amount"]))
        .unwrap();
    let first = access
        .validate_update(&editor, "purchase:1", &roles(&["private"]))
        .unwrap_err();
    let second = access
        .validate_update(&editor, "purchase:1", &roles(&["unknown"]))
        .unwrap_err();
    assert_eq!(first, second);
    assert!(!first.to_string().contains("private"));
}

fn intent(deduplicates: bool) -> Intent {
    Intent {
        decision: "d1".into(),
        effect: "send_purchase".into(),
        destination: "purchasing".into(),
        payload: json!({"amount":500}),
        max_attempts: 2,
        deduplicates,
        compensation_action: Some("cancel_purchase".into()),
    }
}

#[test]
fn lost_delivery_requires_reconciliation_and_a_stable_receiver_identity() {
    let pending = Delivery::pending(intent(false), 1000).unwrap();
    let sent = pending.start(1000).unwrap();
    let ambiguous = sent.response_lost().unwrap();
    assert_eq!(ambiguous.state, DeliveryState::Ambiguous);
    assert!(ambiguous.retry(2000, false).is_err());
    assert!(ambiguous.start(2000).is_err());
    let confirmed = ambiguous.confirmed("receiver:123").unwrap();
    assert_eq!(confirmed.state, DeliveryState::Confirmed);
    assert!(confirmed.start(2000).is_err());
    assert_eq!(
        confirmed.intent.key().unwrap(),
        pending.intent.key().unwrap()
    );
    assert_eq!(confirmed.revision, 3);
}

#[test]
fn retries_are_bounded_backed_off_and_keep_the_same_key() {
    let pending = Delivery::pending(intent(true), 1000).unwrap();
    let retry = pending
        .start(1000)
        .unwrap()
        .response_lost()
        .unwrap()
        .retry(1000, false)
        .unwrap();
    assert_eq!(retry.next_attempt_ms, 3000);
    assert!(retry.start(2999).is_err());
    let last = retry.start(3000).unwrap();
    assert_eq!(last.attempts, 2);
    assert_eq!(last.intent.key().unwrap(), pending.intent.key().unwrap());
    let exhausted = last.retry(4000, true).unwrap();
    assert_eq!(exhausted.state, DeliveryState::Failed);
    assert!(exhausted.start(100_000).is_err());
    let mut other = intent(true);
    other.payload = json!({"amount":501});
    assert_ne!(other.key().unwrap(), pending.intent.key().unwrap());
    let mut policy_change = pending.intent.clone();
    policy_change.max_attempts = 3;
    policy_change.deduplicates = false;
    assert_eq!(policy_change.key().unwrap(), pending.intent.key().unwrap());
}

#[test]
fn delivery_refuses_corrupt_state_and_numeric_overflow() {
    let mut invalid = Delivery::pending(intent(true), 0).unwrap();
    invalid.attempts = 1;
    assert!(invalid.start(0).is_err());
    let mut invalid = Delivery::pending(intent(true), 0).unwrap();
    invalid.revision = u64::MAX;
    assert!(invalid.start(0).is_err());
    let sent = Delivery::pending(intent(true), 0)
        .unwrap()
        .start(0)
        .unwrap();
    assert!(sent.retry(i64::MAX, false).is_err());
}

#[test]
fn valid_time_and_recording_time_answer_different_questions() {
    let version = BusinessVersion {
        id: "rule:v1".into(),
        recorded_at_ms: 1200,
        valid_from_ms: 800,
        valid_until_ms: Some(2000),
        approved: true,
    };
    assert!(!version.applicable(1000, 1100));
    assert!(version.applicable(1000, 1200));
    assert!(!version.applicable(2000, 2000));
    let invalid = BusinessVersion {
        valid_until_ms: Some(700),
        ..version
    };
    assert!(!invalid.applicable(800, 1200));
}
#[test]
fn digests_sort_nested_object_keys_but_preserve_array_order() {
    let first: serde_json::Value =
        serde_json::from_str(r#"{"z":{"b":2,"a":1},"a":[1,2]}"#).unwrap();
    let second: serde_json::Value =
        serde_json::from_str(r#"{"a":[1,2],"z":{"a":1,"b":2}}"#).unwrap();
    assert_eq!(
        omnigraph_ontology::digest(&first).unwrap(),
        omnigraph_ontology::digest(&second).unwrap()
    );
    assert_ne!(
        omnigraph_ontology::digest(&first).unwrap(),
        omnigraph_ontology::digest(&serde_json::json!({"a":[2,1],"z":{"a":1,"b":2}})).unwrap()
    );
}
