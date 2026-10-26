---
rfc: "2026-10-26-governed-decisions"
title: "Governed decisions"
track: public
status: draft
implementation: partial
authors:
  - EnzoTironi
created: 2026-10-26
updated: 2026-10-26
discussion: https://github.com/EnzoTironi/omnigraph/issues/1
supersedes: []
superseded_by: []
blocked_on:
  - Trusted server integration and refusal archive
  - Actor-view integration covering every observation and mutation surface
  - Time-sensitive pre-effect validation and crash qualification
---

# RFC: Governed decisions

## Summary

Extend OmniGraph with explicit action, evidence, decision and external-commitment
contracts. Preserve the accepted schema and manifest as the only graph authority.
The experimental `omnigraph-ontology` crate implements deterministic contract
evaluators and an optional embedded host that publishes constructive or destructive effects
with their dossier and consumes proposals using native uniqueness. It does not
install a new server, CLI or MCP authorization boundary.

This is a fork-local proposal requested by the fork owner. It is not upstream
acceptance, and no unmerged upstream RFC is considered an implemented dependency.
The full integration is intentionally not marked complete.

## Motivation

The reference is Bailing Zhang's *Operational Ontology: From Business Mirror to
Decision Runtime*, first public edition, source commit
`3468edaa76cd831823401c4a4b63360b339a1123`:
<https://github.com/aussie-bzhang/operational-ontology-book>.

The book combines engineering patterns, organizational controls and research.
It supplies no normative conformance suite. Coverage therefore means satisfying
the explicit requirements below, under stated deployment and failure assumptions,
not passing an invented percentage. The implementation is original; no book
text, figures, or executable source is incorporated.

## User and operational behavior

An action has typed bounded parameters, permitted proposers and confirmers,
declared evidence requirements, deterministic guards, a mode and version pins.
Evaluation returns allow, review, deny or unknown with named check results.
Confirmation is a new decision over current evidence. A different actor must
confirm; a different OAuth client for the same actor is not a different actor.

A successful native execution must make business effects, the consumed proposal,
the decision dossier, and external intents visible in one publication. A denial
has no business effects but is itself an auditable decision. Permission denials
must not disclose hidden evidence in their responses. No response acknowledges
durability before the manifest publisher establishes it.

An external operation has separate requested, sent, confirmed and ambiguous
states. No database commit promises an external rollback or exactly-once effect.
Compensation is a new governed action. Reconciliation establishes receiver facts.

## Design

### Authority and execution

1. Resolve the actor and client at the server boundary. Request parameters cannot
   introduce roles, actor identities, guard verdicts or approval facts.
2. Capture one accepted graph/schema/action view and the declared evidence slice.
   Pin data, rules, derived logic and model versions. No later head lookup is a
   substitute for the read's identity.
3. Validate parameters and authorization before evaluating or disclosing evidence.
   Evaluate simple guards as typed predicates; complex verification has a separate
   bounded, versioned verifier result. A model cannot supply authorization.
4. For approval, load a stored pending proposal, revalidate the complete action
   identity and current actor, and evaluate current evidence. Editing parameters
   creates a new proposal. Preserve both the original and confirmation snapshots.
5. Evaluate rows and guards in the existing pre-effect changeset evaluator, before
   `stage_all`. Publication revalidates the attempt's authority; it must not run a
   second independently maintained business evaluator.
6. Consume the proposal once and append the dossier with the business changes in
   one publication. Repeated request identities resolve the original result;
   conflicting reuse fails. An ambiguous publication is reconciled before replay.
7. Reconcile durable external intents from accepted graph state. No second queue
   becomes authority. Persist an attempt transition before sending to the receiver.

Clock conditions require their own publication contract. An unchanged graph head
does not imply that evidence remains fresh. Waiting for a write gate must not
extend an expired approval. A client timeout is not a publication deadline.

#### Native admission contract

The native mutation protocol accepts one trusted, bounded synchronous
`MutationAdmission` instance with an explicit expected head. The same instance
checks validity before staging, under the schema/branch/table gates after their
wait and authority revalidation, and immediately before submitting the publisher.
It performs no I/O, acquires no engine lock and cannot widen native permissions.
The embedded action host reuses its readiness and approval-lifetime evaluator at
those boundaries. A clock moving backwards is a refusal, not extra validity.

The boundary is admission to effects/publication, not the time a remote object
store durably finishes an already-submitted request. Crossing expiry while that
request is in flight cannot retroactively cancel a successful CAS. An uncertain
storage completion remains uncertain and resolves through the durable request id.
No timeout or expiry is reported as proof of rollback. Refusal after staging or
detached commits leaves unreachable garbage, never visible partial business state.

#### Typed append envelope

The business query remains constructive or destructive under the existing D2
compiler check. A `MutationEnvelope` may attach a separately compiled
`MutationAppend` containing 1..256 strict inserts into unkeyed node tables.
The append is not another publication. Both sets execute over the same captured
transaction, enter one combined changeset/integrity check and share the existing
stage/commit/publish tail. Every append table must be disjoint from the complete
business effect set, including cascade effects, before append execution. Staging
also refuses overlapping pending-write and delete tables before durable I/O.
There is no in-query delete view, same-table write/delete ordering or second
commit. Keyed upserts, updates, deletes and edge inserts are refused in the tail.

The embedded host uses this envelope to append its exact dossier for delete-only
actions; existing constructive bindings keep their explicit host-bound append.
Audit uniqueness/integrity failure prevents both the deletion and dossier from
becoming visible. The root-shared gates and exact publisher remain the only
native publication mechanism.

### Coverage matrix

`Evaluator` means executable library logic, not native engine enforcement.
`Existing` means a current substrate capability with its documented boundaries.
`Pending` means required integration/evidence, not a supported product promise.

| Book domain | Required observable contract | Current evidence | Remaining acceptance gate |
|---|---|---|---|
| 1–3: runtime architecture | Data, logic, action and security have explicit owners | Existing compiler/planner/engine/policy; evaluator crate | One enforced end-to-end action path |
| 4: objects and values | Typed identity and bounded value domains | Existing `.pg`; evaluator parameter domains | Unit/value types and generated action descriptors |
| 5: links | Typed directed relations, cardinality, provenance | Existing edges/constraints | Governed entity resolution and source authority |
| 6: interfaces | Reusable declarations and stable identity | Existing property reuse | Runtime polymorphism per upstream #884 |
| 7: business time | Separate valid time, recorded time and retention | Evaluator business interval and evidence timestamps | Temporal queries and corrections over retained state |
| 7: evidence readiness | Required, current, missing, unknown and N/A remain distinct | Evaluator readiness checks | Native admission revalidation; source qualification and remote in-flight timing remain explicit |
| 7: provenance | Every consumed fact names its source and producer version | Evaluator records and explicit embedded read-column bindings | General automatic capture and evidence retention |
| 8: functions/models | Pin logic/model versions and applicability | Evaluator version and prediction records | Governed deployment, invocation and numerical model adapters |
| 9: action contract | Typed inputs, guards, modes and explicit outcomes | Evaluators and embedded registered read/effect bindings | Trusted server registry and parameter codegen |
| 9: approval | Separate proposer/confirmer and single consumption | Embedded graph-backed proposals, native uniqueness, service write gate | Shared authenticated transport boundary and escalation |
| 9: external effects | Stable identity, bounded attempts and ambiguity | Evaluator delivery transitions | Durable CAS, receiver adapter, reconciliation and compensation |
| 10: access | Type/entity/property restrictions and non-interference | Application ACL evaluator only | Native actor views across every access path |
| 10: audit | Dossier includes evidence, versions, checks and actors | Embedded constructive/destructive effects/audit and business refusal audit | Authorization-refusal archive and integrity/retention controls |
| 10: rule lifecycle | Propose, review, release and shadow differences | Existing content branches/diffs | Governed rule registry and shadow comparison workflow |
| 10: override | Structured disagreement with accountable disposition | Pending | Override actions, routing and linked alternatives |
| 11: agents | Authenticated principal, client and bounded capabilities | Existing MCP and coarse policy | Delegation, client attribution and action-only capabilities |
| 12–14: worked domains | One complete closed loop per modeled domain | Pending | Original synthetic domain fixtures and failure journeys |
| 15: implementation method | Versioned acceptance and observable operating envelope | This matrix and test owners | Repeated deployment/recovery and workload qualification |
| 16: verification | Separate local checking, adjudication and bounded proof | Evaluator predicates | Verifier adapter, counterexamples and declared proof fragment |
| 17: feedback | Refusals, overrides and model drift drive reviewed changes | Pending | Queryable feedback, disposition and operational metrics |

External truth, the sufficiency of the declared evidence set, and organizational
responsibility cannot be proved solely from graph consistency. They need source
qualification and domain ownership. Universal action-system proofs are not a
promise; any proof states its rule fragment, assumptions and horizon.

### Upstream coordination

Survey: upstream main `da1c46368c33876d61615add0ceb6778f9ce2b58`, 2026-10-26;
108 open issues and 43 open pull requests. All titles and bodies were collected;
the items below were selected for substantive overlap. Counts are an inventory,
not a claim of independent security review of every patch.

| Open work | Finding relevant to this proposal | Integration decision |
|---|---|---|
| [PR #951](https://github.com/ModernRelay/omnigraph/pull/951) | Actor-view RFC only; latest review at `d700c334` leaves pre-effect ordering, schema identity deletion, branch-window wording and hidden-column merge disclosure unresolved | Do not cherry-pick as implemented security; preserve its materialized-view oracle |
| [PR #1149](https://github.com/ModernRelay/omnigraph/pull/1149) | Narrows accepted graph-access RFC to client contracts and withdraws engine redesign | Do not depend on withdrawn cross-process fences or `disposition.check` |
| [PR #963](https://github.com/ModernRelay/omnigraph/pull/963) | Draft schema-with-live-branches design, including stamp 17; not merged | Keep current schema restrictions; qualify historical replay separately |
| [PR #884](https://github.com/ModernRelay/omnigraph/pull/884) | Property-reuse interfaces do not yet imply runtime abstract bindings | Do not claim full interface runtime support |
| [#978](https://github.com/ModernRelay/omnigraph/issues/978), [#1173](https://github.com/ModernRelay/omnigraph/issues/1173) | Delegated credentials and verified OAuth client attribution are incomplete | Preserve principal identity; add client provenance independently |
| [#1054](https://github.com/ModernRelay/omnigraph/issues/1054), [#1167](https://github.com/ModernRelay/omnigraph/issues/1167) | Read schema/table authority and snapshot commit identity need explicit contracts | `query_observed` adds exact embedded schema/head capture; HTTP read sets and snapshot DTOs remain unchanged |
| [#1055](https://github.com/ModernRelay/omnigraph/issues/1055) | Multiple queries do not automatically share a snapshot | A guard bundle needs one accepted capture |
| [#1058](https://github.com/ModernRelay/omnigraph/issues/1058), [#645](https://github.com/ModernRelay/omnigraph/issues/645) | Historical whole-object schema interpretation and retention are constrained | Materialize audit inputs or prove retention/reconstruction |
| [#958](https://github.com/ModernRelay/omnigraph/issues/958) | Active historical reads lack a GC horizon | Do not treat an active reader as a durable evidence retention root |
| [#513](https://github.com/ModernRelay/omnigraph/issues/513), [#415](https://github.com/ModernRelay/omnigraph/issues/415) | Correlation/idempotency and asynchronous completion need durable identity | A timeout leaves the action unresolved; never silently replay |
| [#657](https://github.com/ModernRelay/omnigraph/issues/657), [PR #952](https://github.com/ModernRelay/omnigraph/pull/952) | Embedding model/source provenance is a separate ongoing contract | Dimension equality is not prediction/model provenance |
| [PR #1001](https://github.com/ModernRelay/omnigraph/pull/1001), [PR #1124](https://github.com/ModernRelay/omnigraph/pull/1124) | Traces/logs improve correlation, not durable decision evidence | Keep decision dossiers in authoritative storage |
| [#1018](https://github.com/ModernRelay/omnigraph/issues/1018), [PR #1177](https://github.com/ModernRelay/omnigraph/pull/1177), [PR #1181](https://github.com/ModernRelay/omnigraph/pull/1181) | Writer-process and maintenance/recovery boundaries remain active design areas | Preserve deployment admission; no new distributed-lock claim |

Open issues may describe older code. Their existence alone does not establish a
current reproducible defect. The inventory is dependency analysis; each issue fix
still requires its own reproduction at the owning test boundary.

## Invariants

- One graph publication owns effects, decision identity, approval consumption and
  delivery intents. No audit sidecar or maintained queue becomes parallel truth.
- One coherent capture owns data, accepted schema, action and policy evidence;
  a retry captures anew. Time-based expiry is checked separately.
- Validation occurs before staging. Pre-publication effects remain unreachable.
- Schema identities and protected principals survive supported renames, not
  drop/re-add. Schema apply must participate in access-identity protection.
- Indexes and caches cannot decide whether a guard or permission is true.
- Authentication establishes actors; models and request bodies cannot establish
  trusted identities or verdicts. No new public Lance writer is added.
- Every capture, dossier, verifier call and external retry is bounded and has an
  explicit outcome. Uncertain publication is not a denial or successful rollback.

## Compatibility and reversibility

The crate is non-publishable and adds no persisted engine field, protocol route,
storage stamp, raw writer or native policy vocabulary. `query_observed` adds an
embedded read result containing the accepted SchemaIR hash. Existing read calls
keep their storage path and do not compute that extra digest. The host uses
ordinary typed queries and conditional native mutations. Removing the host
changes no graph format; its dossier rows are ordinary application data.
No new transport authorization boundary or irreversible audit format is claimed.
Integrating those contracts needs an accepted amendment and corresponding format,
wire, recovery and downgrade/refusal evidence where applicable.

## Alternatives

Wrapping every stored mutation as an action leaves approval and audit bypasses.
An independent decision database introduces cross-store atomicity and retention
obligations. A second commit for audit loses the action's atomicity on crashes.
Universal formal verification has no defensible unbounded promise. None is an
acceptable substitute for the scoped integration contract above.

## Evidence and tests

The crate's `tests/contracts.rs` owns deterministic evaluator behavior. Extend it
for malformed contracts, parameter mismatch, missing/stale/future evidence,
same-person/different-client approval, expiry without content changes, action
version drift, opaque access refusals, delivery ambiguity and retry exhaustion.
`tests/engine.rs` additionally owns the real-engine host: atomic business/dossier
visibility, native one-time consumption, concurrent confirmations, durable refusal,
reopen/replay, invalid effects, registered append validation, exact read observation
and the native coarse service-write gate. The service identity is trusted; this
does not qualify server-side authentication or native actor views.

The existing DST cost owner covers `OntologyPropose`, `OntologyConfirm`,
`OntologyReplay`, `OntologyDelete` and `OntologyDeleteReplay`, each over a reopened handle. Setup/verification use underscore
labels and the existing golden's two deterministic passes must agree. Propose
measures request lookup, evidence capture and dossier append; confirm adds the
stored-proposal read and the atomic effect; replay reads the existing dossier
without publishing. Existing golden rows must remain unchanged.

Native integration extends existing owners: `policy_engine_chassis.rs`,
`writes.rs`, `integration/point_in_time.rs`, `integration/merge_truth_table.rs`,
`detached_commit_matrix.rs`, server `auth_policy`/`data_routes`/`mcp`, and GQT for
rows/errors. Mechanism and concurrency assertions belong in Rust. Update the DST
storage-cost golden for every changed storage footprint. A pure evaluator test
cannot establish any of those end-to-end properties.

No new Lance behavior is assumed by this crate. Native integration must follow
the complete [Lance reading protocol](../dev/lance.md), inspect the pinned source,
and retain the current substrate fences.

## Rollout

1. Executable evaluator contracts and this coverage/dependency matrix.
2. Trusted action registry and native atomic execution, including deny audit,
   durable idempotency, clock gates, replay and proposal consumption.
3. Shared HTTP/CLI/MCP surfaces and agent identity/delegation.
4. Native actor views, temporal evidence retention, external receivers and recovery.
5. Domain journeys, shadow/override feedback, verifier integration and qualification.

These are completion gates, not independent substitutes for the final objective.
The first and part of the second have implementation in this change. The fork
cannot advertise full operational-ontology support until the remaining gates
have evidence. Open work is tracked in the
[fork implementation issue](https://github.com/EnzoTironi/omnigraph/issues/1).

## Unresolved questions

- Which native authority records action releases, revocation and one-time approval
  consumption while preserving historical rule replay?
- What bounded clock contract applies between pre-effect validation and durable
  publication under contention or ambiguous storage writes?

## Decision log

- 2026-10-26: Fork owner requested implementation and validation against open
  upstream work. Added original evaluators and an explicit partial disposition.
  No upstream acceptance or full conformance claim is implied.
