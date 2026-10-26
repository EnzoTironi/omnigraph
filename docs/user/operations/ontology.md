# Embedded governed actions

The fork includes an experimental Rust library, `omnigraph-ontology`, for
evaluating action contracts. With its `engine` feature, an application can store
proposals, approve or refuse them, and publish constructive business changes with
their decision dossier in one graph commit.

This library is not mounted in HTTP, CLI or MCP. It does not provide native
entity/property authorization, general workflow execution, or full coverage of
the operational-ontology book. The
[governed decisions proposal](../../rfcs/2026-10-26-governed-decisions.md)
identifies those remaining boundaries.

## Run the reference journey

From the checkout, run the deterministic contracts and the real engine journey:

```bash
cargo test -p omnigraph-ontology --locked
cargo test -p omnigraph-ontology --features engine --locked --test engine
```

The second command uses local temporary graphs. Its purchase example proposes
a change, confirms with a different actor, checks the business result and audit,
reopens the graph and resolves the same request without executing it again. Other
cases exercise refused approvals, duplicate confirmation, concurrent confirmation,
invalid effects and ordinary native write policy.

## Define the action

An `Action` declares parameter domains, required evidence and freshness budgets,
typed predicates, permitted proposer/confirmer roles, an execution mode, version
identities and a proposal lifetime. The current predicate fragment supports
equality, bounded integer ranges and equality to a declared parameter. It does
not interpret arbitrary expressions, execute models or prove global invariants.

The application resolves the `Principal`, its human/agent/service classification
and its roles through a trusted identity boundary. These are not action
parameters. Another client acting as the same person cannot satisfy separation
of duties, and an agent cannot supply human confirmation merely by holding a
confirmer role.

Modes are `Manual`, `Automatic`, `Confirm` and `Disabled`. The first and third
produce proposals for confirmation; automatic mode may permit an effect at
submission. An unmet guard refuses; unknown evidence has a distinct outcome.
Unknown evidence stays unknown on confirmation too; a known denial takes
precedence over unrelated unknown evidence. Unavailable evidence does not create
a fabricated false predicate result. Confidence from a prediction never
authorizes an action.

## Bind it to a graph

Add `engine::RECORD_SCHEMA` to the application's initial schema. It declares
`OntologyDecision` with strict inserts, a unique `decision_id`, the serialized
`record`, and a nullable unique `consumes` field. Existing graphs need the
ordinary supported schema-deployment procedure.

Register an `EngineAction` with a private `Session`, one evidence read query,
one constructive effect query, the evidence column mapping, the target branch,
a service writer identity and a clock. The accepted schema and registered query
sources are bound into the runtime's identity.

The effect query includes this append alongside its business inserts/updates:

```gq
insert OntologyDecision {
  decision_id: $og_id,
  record: $og_record,
  consumes: $og_consumes
}
```

Declare those parameters as `String`, `String` and `String?` respectively. The
host reserves every `og_` action parameter. Registration checks that the append
uses exactly those fields and parameters, and refuses audit updates, destructive
actions and unpublished effect modes. The native compiler still validates the
complete query and assignments before execution.

The evidence query returns at most one row. Each evidence binding names a value
column, integer millisecond observed/recorded times and a source-record column,
plus the source and producer version. A missing row or null value is missing
evidence. A future recording time is invalid.

`Session::query_observed` returns rows, their graph commit and the accepted schema
IR digest from the same read capture. The action writer uses that exact commit
as its native mutation precondition. An intervening publication refuses the
write; the application must observe and reconsider rather than drop the condition.
Confirmation reads its proposal and evidence at the proposal lookup's same
captured head. Evidence reads therefore require explicit edge selections;
wildcard traversals and explain statements are refused at registration.

## Submit and confirm

Call `EngineAction::submit` with a unique request id, verified principal and
parameter map. For a review result, call `confirm` with a new request id, the
stored proposal's id and a verified confirmer. The host loads the proposal from
the graph; it does not accept a client-supplied proposal as authority.

An allowed confirmation appends its dossier and business effects together. A
unique `consumes` value prevents another successful confirmation of the same
proposal. A business refusal appends only its dossier. Repeating a request id
returns its stored decision; reusing it with different inputs refuses. Replays
have `replayed: true` and no new `commit_id`.

Keep the writer identity private. Applied native policy must deny direct
business and audit writes to action callers. The embedded host itself is trusted;
it can still use its private session and identity. This library does not make
storage holders subject to an unbypassable sandbox.

## Current operating boundaries

- Freshness is evaluated at decision evaluation time. Time spent waiting for
  native staging or publication is not covered by a commit-time clock fence.
- Authentication/role failures refuse before evidence capture. Validation,
  authorization and publication failures report an error without a separate
  denied-attempt dossier; only evaluated business refusals are appended.
- A changed accepted schema requires re-registration. Historical replay retains
  the materialized input dossier; arbitrary historical rule execution is absent.
- The embedded host binds proposals to its configured root. This is an operation
  target, not a portable graph identity; moving a graph requires new proposals.
- Dossiers are ordinary graph data. This library adds no retention lock,
  cryptographic signature, immutable audit privilege or native row/column policy.
- Destructive actions, verifier services, external delivery and compensation are
  not executed by the adapter. Delivery types describe bounded transitions only.
- Existing graph concurrency, storage, admission and recovery restrictions remain
  in force. No distributed fencing or external exactly-once guarantee is added.
