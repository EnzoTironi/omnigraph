# Operational ontology contracts

This experimental crate evaluates action contracts, evidence readiness,
confirmation, application access rules, business versions, predictions, and
external-delivery transitions. Its optional `engine` adapter publishes
constructive or destructive effects with decision dossiers in real OmniGraph commits. It is
not installed in HTTP, CLI or MCP and does not enforce native actor views.

Run its executable contract tests from the checkout:

```bash
cargo test -p omnigraph-ontology --locked
cargo test -p omnigraph-ontology --features engine --locked --test engine
```

## Host obligations

A host must resolve identities and memberships independently of request bodies,
load proposals from authoritative storage, and collect all evidence from one
accepted view. A digest detects contract changes; it does not authenticate a
proposal or its author. Never accept a client-supplied decision as permission.

Commit business effects, proposal consumption, the decision dossier, and any
external intent through one graph publication. Revalidate the complete accepted
authority and time-sensitive conditions before effects. A decision evaluated
earlier is not a transferable write capability. Repeated confirmation requires
durable uniqueness, not an in-memory flag. Refusals need durable audit too.

The `access` evaluator is an application ACL oracle. Native reads, aggregates,
search, history, exports, Blob access, merges, and writes do not call it. Do not
claim native row/column security from its tests. The actor-view RFC remains the
owner of that integration, including hidden-data non-interference.

Persist each delivery transition by compare-and-swap before using it. Only send
after the intent and its decision are durably visible. An in-flight delivery
after a crash is ambiguous; elapsed time alone never proves absence. Receiver
deduplication must be a qualified receiver contract, not a client assertion.

The kernel has no mutable store, clock, network, queue, or scheduler. These
values are domain records to store in the graph, not another source of truth.
Predictions carry applicability and readiness; they never authorize a write.

## Specification and outstanding work

The [embedded user guide](../../docs/user/operations/ontology.md) describes the
engine adapter and its current limits. The
[governed decisions RFC](../../docs/rfcs/2026-10-26-governed-decisions.md)
owns the proposed integration, book coverage, upstream coordination, and evidence
gates. Its matrix deliberately distinguishes implemented evaluators from missing
end-to-end guarantees. This crate is not full book conformance.
