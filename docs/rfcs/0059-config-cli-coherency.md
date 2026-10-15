---
rfc: "0059"
title: "Named targets and CLI coherency"
track: maintainer
status: draft
implementation: not-started
authors:
  - andrew
created: 2026-09-06
updated: 2026-10-15
discussion: https://github.com/ModernRelay/omnigraph/pull/675
supersedes: []
superseded_by: []
blocked_on:
  - Review the versioned operator configuration and command migration against the complete current CLI inventory.
  - Name the introduction, last compatibility, and removal releases before shipping the transition.
---

# RFC 0059: Named targets and CLI coherency

## Summary

Introduce one named **target** for the destination of every backend operation.
Select it with `--target NAME` or an explicitly saved current target. A target
binds an access kind, scope, address and authentication references. Resolve it
once, refuse invalid or unsupported selections without fallback, and print the
resolved destination before mutations. Source directories and credentials do
not select destinations.

Keep the existing command families: `cluster validate/plan/apply/status` and
managed `cluster create/push/delete/operation` remain under `cluster`. Add
`target` only for administering connection selections. Configuration remains
input files selected with `--config DIR`; provisioning and deployment remain
operations on the selected cluster. No `workspace`, `service` or `deployment`
command group is introduced.

Keep declarative planning/application, the current v2 identity contract and
existing deployment/publication authority. This proposal changes destination
selection, not the storage, authorization or deployment protocol. All new syntax
below is proposed; the [CLI reference](../user/cli/reference.md) owns currently
supported behavior.

## Motivation and inspected baseline

The current CLI distributes destination selection among `--server`, `--store`,
`--cluster`, `--profile`, `--managed`, `--direct`, operator defaults, environment
profiles and managed folder context. Their applicability differs by command.
The existing `cluster` family already groups configuration, deployment and
managed lifecycle operations. Keep that organization and make backend selection
consistent across those operations.

Source was reviewed at `cea152566` on 2026-10-14. The existing explicit managed
control selection and ambiguity checks already prevent several earlier wrong
destination cases; this proposal must not describe those fixes as missing.
Remaining inconsistencies include:

- Supported data commands can still select managed folder context implicitly,
  while controls require `--managed`. Changing directory affects some commands.
- Profiles work for data and maintenance but not cluster controls. Cluster
  nicknames and literal roots have different applicability across commands.
- `--direct` means ordinary addressing and can still select HTTP. Its name
  does not describe the backend it selects.
- Mutation destination diagnostics are not uniform across data and deployment
  commands. Inspecting a profile does not explain every dispatch path.

The source owners are [scope](../../crates/omnigraph-cli/src/scope.rs),
[command applicability](../../crates/omnigraph-cli/src/planes.rs),
[managed data](../../crates/omnigraph-cli/src/managed/data.rs) and
[deployment dispatch](../../crates/omnigraph-cli/src/main.rs). This RFC records
source inspection and proposed evidence gates, not runtime qualification.

Expected work includes interactive switching between development and production,
scripts sharing operator homes with interactive shells, local authoring against
a remote cluster, bootstrap before a cluster exists, and recovery after a lost
submission response. Those cases require a common selection contract while
retaining operation-specific effects and recovery rules.

## User and operational behavior

### Vocabulary and selection

| Concept | Meaning |
| --- | --- |
| Target | Operator-owned destination and authentication binding. Its nickname is not backend identity. |
| Configuration bundle | Input files: `cluster.yaml` and its referenced schema, query, policy and provider files, or a captured managed source revision. It is not a separately registered resource. |
| Cluster | The existing unit of configuration, deployment and managed lifecycle operations. Target kind determines which operations are supported. |
| Deployment | Existing durable application/run identity and its observed outcome. |
| Graph and branch/snapshot | Data scope within the selected target, independent of configuration source revision. |

For backend operations, selection is exactly:

```text
explicit --target NAME > saved current target > missing-target error
```

An empty, unknown, malformed or unsupported explicit target fails. It never
falls through to the current target. The final design has no environment
target/profile override, directory-derived target, URL/name guessing or
credential-derived destination. Automation passes `--target`; separate
`OMNIGRAPH_HOME` directories isolate configuration where needed. `--target`
never changes the saved current target.

Proposed examples:

```sh
omnigraph target list
omnigraph target current
omnigraph target inspect production --output json
omnigraph target use staging
omnigraph query list_people --target staging --graph knowledge
omnigraph cluster validate --config ./company-brain
omnigraph cluster plan --config ./company-brain --target staging
omnigraph cluster apply --config ./company-brain --target staging
omnigraph cluster push --config ./company-brain --target production \
  --expected-revision PREVIOUS_REVISION --message "Update configuration"
omnigraph cluster plan --target production --rev REVISION
omnigraph cluster apply --target production --plan PLAN_RUN_ID
omnigraph cluster status --deployment-id DEPLOYMENT_ID --target staging
omnigraph cluster status --target production
```

Here `staging` is a server target; `production` is a managed cluster target.
Data commands retain their existing query, parameter, branch, snapshot and
conditional-write semantics. Target selection never infers a stored query from
a local filename or a graph from credentials or discovery results.

Local authoring, version/help and completion commands do not require a target.
An irrelevant explicit `--target` is rejected rather than silently discarded.
Target administration takes its named entry as the object being edited; it
does not contact that backend merely to list, inspect, select or remove it.

### Target kinds and scopes

Kind chooses the access/authentication path; scope says what is addressed.
Neither alone establishes that every command is available.

| Kind | Scope and required binding | Eligible purpose |
| --- | --- | --- |
| `direct` | `graph`: one storage URI | Currently supported direct graph operations, including initialization and maintenance. |
| `direct` | `root`: one deployment storage root | Currently supported deployment, recovery and root/graph tooling. A root is not a graph URI. |
| `server` | One server endpoint and its explicit authentication binding | Currently supported served data and deployment operations. Preserve HTTP(S) and Unix-socket support and platform refusals. |
| `managed` | `account`: control API origin and control identity binding | Cluster creation and exact provisioning-operation recovery before a cluster exists. No graph access. |
| `managed` | `cluster`: control API origin, cluster ID/incarnation and control identity binding; optional independently selected data endpoint and separate data credential reference | Qualified managed configuration/lifecycle operations; graph operations additionally require a complete data binding. |

An account target must not require a cluster ID, data endpoint or configuration
directory. Cluster creation returns the cluster identity and operation/replay identifiers
before attempting to save a local target. A local save failure cannot hide an
accepted creation or cause another create. Registering the resulting cluster
target does not automatically make it current.

A managed cluster target can be registered immediately after accepted creation,
before its data endpoint is ready. Control operations use its cluster binding; graph
operations refuse until an explicit endpoint/data binding is installed. An
unavailable data endpoint cannot block cluster status, deletion or recovery.

A graph operation requires `--graph` or the selected target's `default_graph`
when the target does not itself identify one graph. A direct graph target rejects an
additional graph selector. Registry and cluster control operations do not
consume a graph default and reject an irrelevant explicit graph. No sole-graph probe supplies a missing graph. Data branch/snapshot
selection remains explicit or uses that operation's existing default; targets
do not add hidden branch defaults.

Backend applicability stays explicit. A managed target does not enable every
server route, and a root target does not acquire direct query support merely
because it contains graphs. Preserve the current qualified managed query,
mutation, load, commit-read and graph-list paths during migration. Additional
paths need their own credential, transport, authorization and outcome tests.

### Configuration ownership and inspection

Keep one operator file under `OMNIGRAPH_HOME` (normally `~/.omnigraph`). Its
target-format revision introduces `targets` and `current_target`; operator
identity, presentation preferences and read aliases keep their distinct roles.
The following is a schema sketch, not an accepted serialized format:

```yaml
target_config_version: 1
current_target: staging
targets:
  scratch:
    kind: direct
    scope: graph
    uri: file:///data/scratch
  local-deployment:
    kind: direct
    scope: root
    uri: file:///data/deployment
  staging:
    kind: server
    endpoint: https://staging.example
    credential: staging-auth
    default_graph: knowledge
  provisioning:
    kind: managed
    scope: account
    api: https://control.example
    control_credential: work-account
  production:
    kind: managed
    scope: cluster
    api: https://control.example
    cluster_id: CLUSTER_ID
    cluster_incarnation: INCARNATION
    endpoint: https://production.graphs.example
    control_credential: work-account
    data_credential: production-identity
    default_graph: knowledge
```

Credential fields denote nonsecret references within their existing namespaces,
not bearer values or permission claims. The accepted schema must define each
reference's lookup and principal binding, including supported server auth
modes. Backend identity is never inferred from the target nickname or URL.

Resolve relative stored filesystem paths against the declaring operator file,
not the invocation directory. Validate target variants strictly: duplicate
names, missing or contradictory fields, unknown routing/authentication fields,
invalid endpoints and unknown format revisions refuse. Apply existing endpoint
rules for each transport; managed canonical-origin restrictions do not erase
supported server Unix sockets. Do not merge files, splice fields from several
targets, or add independent server/auth overrides.
For Unix transport, retain the configured socket as destination/authentication
identity; its internal synthetic HTTP origin is never an operator target.

`target use NAME` validates the local entry and atomically saves the selection.
Configuration edits serialize and preserve unrelated settings; they do not
hold a lock across network work. `target current` reports the saved selection
or its absence. `target inspect [NAME]` shows the explicit entry or current
selection, its kind/scope, canonical configured address, graph default,
credential mechanism/reference, configuration path and selection provenance.
It labels configured identity as configured, not remotely verified.

Inspection is offline and redacted: no token reads/exports, issuer calls,
authentication refresh, credential plugins, backend opens or reads of bundle
files. Untrusted configuration bundles cannot install authentication behavior.
Portable secret export, if ever added, is a separate explicit operation.

### Aliases, output and discovery

Read aliases remain pure bindings: target name, graph, stored query and existing
parameter/output defaults. They name the same target objects used by ordinary
commands. An alias's declared target is explicit selection; conflicting global
destination or graph overrides refuse. It cannot inherit the current target or
have another credential resolver. Migrate existing server-bound aliases
explicitly; managed aliases require qualification before being enabled.

Use `-o/--output` consistently for supported management formats, initially
human/table and JSON. Keep existing query result envelopes, NDJSON/export and
raw Blob streams; a common option does not imply every format works everywhere.
Retain existing structured errors, effect outcomes and exit semantics.
Compatibility spellings such as `--json`/`--format` need one documented mapping
and conflict rule, not a second rendering implementation.

Before every backend mutation, print the resolved target kind, destination,
graph/branch where applicable and selection source to stderr. This includes
ordinary mutations, branch statements, loads, Blob writes, deployments and
managed lifecycle operations. `--quiet` may suppress progress but not this
destination line or unknown-outcome recovery information. Results remain on stdout.
Classify effects per operation, including durable unpublished proposals and
equivalent GQ statements. Preserve read-shaped operations' existing recovery
effects and make them explicit; a read-like verb is not proof of no writes.
Management receipts include target binding and exact operation ID using the
existing outcome representation; data receipt schemas are not silently wrapped
or changed to add targeting metadata.

Help describes backend/scope applicability without contacting a backend.
Generate static command/flag completion and local target completion from the
same declarations. Tab completion must not log in, acquire credentials, mutate
configuration or require network access. Reuse existing Clap declarations and
applicability metadata; a second exhaustive command registry is unnecessary.
Tests independently assert the observable routes and effects.

Retrieval, operational diagnosis and schema documentation have distinct
meanings. Preserve data-query `explain`; any future configuration-field help
must use a qualified noun. Exact names never fall back to prefix matches.
Diagnostics assembled from several observations disclose their revisions rather
than implying one snapshot. Live capability discovery, remote completion and
`auth can-i` are follow-ups with separate server contracts. Client support,
backend support, object existence and authorization are different facts.

## Design and authority

### One resolved invocation

Parse the command and resolve one complete target before credential acquisition
or backend effects. Check kind/scope applicability, then acquire/validate the
selected authentication binding and dispatch through the existing operation
owner. Carry the same immutable target and provenance through diagnostics,
submission, response handling and observation. A later `target use`, nickname
edit, directory change or credential-cache update cannot redirect that command.

Operation retries may recapture current authorized database state under their
existing protocol; they cannot select a different target. Protocol-owned
pre-submit renewal may replace a credential only for the same selected
identity/binding. Preserve the current bounded settlement, cancellation and
retry rules rather than inventing generic client retries.

### Credentials and policy

The [identity and applied-policy RFC](2026-09-09-identity-credentials-and-applied-policy.md)
owns authentication. Native signed data credentials and caches remain strict,
identity-only v2. Do not restore v1 compatibility, graph/action grants in
tokens, permission checks based on cached claims, or public Rust shape repairs
from the September draft. The server verifies identity and the real
root/incarnation; current applied policy authorizes operations. Candidate
policy cannot authorize its own deployment.

Resolve endpoints independently of credentials. Before sending a managed data
credential, require agreement with the selected canonical API origin, cluster
ID/incarnation and data endpoint as well as the existing v2 metadata checks.
A cache entry cannot supply or replace the destination. Endpoint rotation is an
explicit target update plus matching credential acquisition; permission refusal
never triggers another identity, target or authentication mode.

Keep storage, server, managed control and managed data credential namespaces
separate. Preserve provider-native control login/renewal and origin-bound
automation credentials. Preserve bounded self-only v2 acquisition before
submission for missing, expired or different-principal data credentials.
Malformed/unsupported caches refuse before issuer access; validate again under
the existing acquisition lock. Offline inspection never performs acquisition.
Authenticated served actors remain server-derived; direct actor selection and
engine policy checks retain their existing scope.

### Configuration input and deployment

The destination never supplies desired configuration. `--config DIR` selects
the directory containing `cluster.yaml` and its referenced input files. Keep
`cluster.yaml` and all storage/wire names unchanged; connection naming does
not rename deployed artifacts or routes.
The [current control-plane guide](../dev/control-plane.md) and
[server deployment RFC](2026-09-29-server-runtime-and-online-deployment.md)
own durable authority and activation.

| Stage | Direct root or server | Managed cluster |
| --- | --- | --- |
| Validate | Local bundle validation, without target credentials. | Same local authoring validation; does not validate a remote revision. |
| Push | No new upload step. | Explicit capture/upload of referenced files, retaining existing bounds and replay contract. |
| Plan | Bundle preview against selected destination. | Saved plan for explicit pushed revision or bound source head, including existing lease/effects. |
| Apply | Capture/revalidate source and submit through current owner. | Execute the exact saved plan; never silently replan, push or commit source. |
| Inspect/wait | Observe the original deployment ID and digest. | Observe the original run/provisioning-operation identity under its protocol. |

Keep `--config DIR` for local inputs to `cluster validate`, managed
`cluster push`, and direct/server `cluster plan` and source-based `cluster apply`. Its
existing `.` default selects source files only and cannot supply a destination.
Managed `cluster plan` retains `--rev` or the bound source head; managed
`cluster apply` retains the exact saved `--plan`. These two paths reject
`--config` because they no longer need folder context and do not execute local
files. Managed plan output identifies the exact selected revision. A bundle's
declared storage root must agree with the independently selected destination
under existing canonical-root checks; neither side silently overrides the
other. A graph branch is never a configuration branch or revision.

The target determines command availability. For example, `cluster create` needs
an account-scoped managed target, `cluster push/delete` need a managed cluster
target, and `cluster force-unlock` needs supported direct root access. An
unsupported combination refuses rather than changing backend or turning one
operation into another. `cluster validate` stays local and needs no target.

Applying new configuration through direct storage still requires stopped serving
and the existing writer/admission conditions. Server-owned apply retains live activation. Root-only reconciliation
of an existing deployment remains possible without source files, using its
exact identity and existing quiescence requirements. Routing never adds a
ledger writer, a per-graph apply loop, a last-applied cache or field-ownership
database. Removed import, refresh and approve commands stay removed.

### Submission, outcomes and waiting

Preserve operation-specific submission and replay contracts. Server deployment
submits once with its original ID; a lost reply leads to exact-ID observation.
Managed run/lifecycle idempotency keys retain only their existing replay scope.
Do not generalize that scope to token issuance or data mutations.

Pin waits to target identity/incarnation, operation/deployment ID and source
revision/digest where the protocol supplies them. Never follow a newer rollout
or reread the current target while waiting. Keep accepted, applied, active and
ready distinct; old healthy serving state does not prove the requested revision
is active. State which condition a wait promises.

Keep finite connection/request/body limits and separate bounded operation-wait
budgets. Preserve the current per-operation defaults and documented ranges;
do not import kubectl's differing zero-as-infinite meanings. A timeout stops
local observation, not accepted work. Return the original IDs and recovery
instructions; nonzero exit is not proof that no effects occurred. No silent
write replay, credential downgrade or target fallback is allowed.

## Compatibility and reversibility

This intentionally changes CLI/configuration contracts from
[RFC 0007](0007-operator-config.md), [RFC 0011](0011-cli-addressing-and-config.md),
[RFC 0052](0052-managed-control-plane-cli.md) and
[RFC 0061](0061-managed-cluster-lifecycle.md). Acceptance must identify their
affected sections and add disposition notes. They remain authoritative until
then; this draft does not mark them superseded.

The migration inventory must map every current command and target source:

| Existing surface | Final destination |
| --- | --- |
| Server, store/root entries, profiles and flat target defaults | Complete named targets; presentation defaults stay separate. |
| `--server`, `--store`, `--cluster`, `--profile`, positional backend addresses | `--target NAME`; registration accepts explicit endpoints/URIs. No permanent second targeting syntax. |
| `--managed` and folder-based mode selection | Managed target kind/scope, with the existing `cluster` verbs. |
| `--direct` | Retired after command-aware migration; never translate it to storage because ordinary addressing may select HTTP. |
| `OMNIGRAPH_PROFILE`, implicit folder context | Retired as routing sources; explicit import may propose target entries but never selects one automatically. |
| `cluster validate/plan/apply/push` | Keep their names. `--config DIR` selects local input; `--target NAME` selects the backend. Managed plan/apply retain `--rev`/`--plan` and no longer read folder context. |
| Deployment status/observe/reconciliation, lock and ledger maintenance | Keep `cluster status/observe/apply/force-unlock/upgrade-ledger` and their exact-ID/recovery contracts; resolve the destination through `--target`. |
| Managed create/delete/undo-delete/status and provisioning-operation recovery | Keep `cluster create/delete/undo-delete/status/operation`, including account-scoped creation/recovery. Managed history/cancel/token also remain under `cluster`. |
| Folder `use`, `profile list/show`, credential commands and aliases | Target administration/inspection and target-bound authentication/aliases, with explicit migration for each spelling. |

Do not infer equivalence from equal nicknames. Existing cluster-name resolution
differs between maintenance and deployment commands. Flatten each profile's
complete address and graph binding; do not inherit fields from unused profiles
or defaults. Importing a folder binding requires independently verified cluster
endpoint/identity information, not a cached token's choice of destination.
Colliding server/cluster/profile names require an explicit mapping. Two old
names sharing an endpoint retain their chosen credential owners; URL-prefix
matching cannot establish equivalent identity or merge them automatically.

The release introducing the new contract may retain old **explicit** spellings
as bounded adapters only where their whole destination can be resolved without
ambiguity. Reject mixtures with `--target`; adapters must enter the same resolved
invocation and print deprecation plus destination diagnostics. Context/profile/
default-dependent legacy invocations are not adapters: ambient
`OMNIGRAPH_PROFILE`, flat routing defaults and folder context never select a
destination, even when a current target exists. An explicit `--profile NAME`
may adapt only as a complete binding and wins over current selection. Without
an explicit binding or a deliberately saved current target, refuse with
migration instructions. A versioned migration is explicit and leaves
`current_target` unset until `target use` is invoked. Selector-free new commands
then use that current target; final target-format configuration rejects legacy
routing sections and environment profile selection.

Before shipping, name the introduction release, last compatibility release and
removal release, and test that interval. After removal all legacy destination
spellings refuse. The complete command map and exact new format/refusal behavior
are acceptance gates, not permission for indefinite adapters. Preserve explicit
direct access through direct targets and all existing supported transport types.

Older binaries ignore unknown operator keys today; a new format marker alone
cannot make them refuse safely. Use isolated operator homes and matched
binary/config/script upgrades, with a nonsecret backup and an explicit rollback
procedure. Never run old binaries on migrated configuration and assume equivalent
selection. Routing rollback changes no graph format, cluster identity or ledger;
independent storage/server downgrade restrictions still apply.

## Scope and alternatives

The existing served query-catalog CLI gap
[#653](https://github.com/ModernRelay/omnigraph/issues/653) remains separately
tracked. Its implementation should use the common target resolver and existing
active `GET /graphs/{graph}/queries` authority, preserve direct listing, and keep
query validation separate. This RFC does not implement or close that issue,
invent compiled catalog metadata, or make catalog visibility an invocation grant.

Live capability discovery, authorization preview, remote completion, semantic
diff exit modes, resumable watch UI and broader client/server compatibility
windows are follow-ups. No new server endpoint is necessary just to name and
resolve existing supported targets.

| Alternative | Decision |
| --- | --- |
| Keep overlapping flags and document precedence | Rejected: every new command must still explain multiple competing selection models. |
| Add managed profiles only | Rejected: leaves controls and folder routing with different selection rules. |
| Require an explicit target on every invocation | Supported for automation; allow explicit persistent selection for interactive work, with mutation diagnostics. |
| Copy kubeconfig file merging and independent connection overrides | Rejected: expands provenance/write-back rules and permits destination/authentication field splicing. |
| Copy Kubernetes field ownership or per-object apply | Rejected: creates incompatible configuration/publication authority. Existing deployment owners remain responsible for partial progress and recovery. |

## Invariants and substrate alignment

The [architectural invariants](../dev/invariants.md) are unchanged. One immutable
target does not replace an engine snapshot: each attempt still captures and
revalidates its own coherent accepted authority. Graph changes retain their
existing publication boundary, explicit unpublished-result contracts and durable
receipts. Deployment reconciliation retains its existing owner across partial
effects. Names remain distinct from root, incarnation and accepted schema identity.

Current policy, server-verified identity and direct-engine gates remain mandatory.
No cloud-only correctness path, shadow configuration authority, custom storage
primitive, process-local distributed fence or generic retry queue is introduced.
No deny-list exception is requested.

Lance remains the pinned 11.0.0 substrate. This design adds no Lance API or
storage semantics. The full upstream [object-store](https://lance.org/guide/object_store/)
and [observability](https://lance.org/guide/observability/) pages were reviewed;
storage credentials and instrumentation remain with their current owners.
Implementation that changes a substrate boundary must repeat the relevant
[Lance qualification](../dev/lance.md); this documentation review is not one.

## Evidence and tests

Extend existing owners in [the test map](../dev/testing.md), with independent
assertions of destination, credentials, requests and durable effects. Generated
help snapshots or tests derived only from the routing table cannot prove routing.
Use hermetic operator homes and fixture credentials; do not log secrets.

| Gate | Required observation | Existing owner |
| --- | --- | --- |
| Selection | Explicit target overrides current; empty/unknown/malformed/unsupported selection never falls back. Missing selection errors before backend access. | `operator.rs`, `scope.rs`, `planes.rs`, `cli_schema_config` |
| Directory independence | Valid, malformed and parent-only managed context cannot affect any new target-selected command or local tool. | `cli_cluster`, `cli_data`, managed data tests |
| Frozen invocation | Change current target/config during acquisition, submission and waiting; all traffic and diagnostics retain original target identity. | Managed data tests, `cli_cluster`, `system_remote` |
| Scope matrix | Graph/root/server/managed-account/managed-cluster; missing/explicit/default graph; unsupported operations; server Unix sockets and unsupported platforms. | `planes.rs`, `cli_schema_config`, `cli_data`, `cli_cluster` |
| Bootstrap | Create without an existing cluster; control-only managed cluster target before endpoint readiness; lost reply and local target-save failure preserve original replay/operation identifiers without duplicate creation. | Managed lifecycle tests, `cli_cluster` |
| Credential binding | Wrong API, endpoint, cluster/incarnation or principal never sends the credential to a substituted destination; v1/malformed cache refuses. Missing/expired valid v2 paths acquire only under existing bounds. | Managed data/auth tests, server `auth_policy` |
| Inspection/completion | Offline, redacted, correct provenance; no token reads, issuer/backend calls, config writes or executable auth. Relative paths stay tied to the declaring file. | Operator/config and CLI invocation tests |
| Output | Every mutation has the same resolved target diagnostic on stderr; JSON, query rows, NDJSON and Blob bytes remain parseable; exact receipts and unknown outcomes survive quiet mode. | `cli_data`, `cli_cluster`, existing outcome tests |
| Source/deployment binding | `--config` and its default select only source files; managed plan/apply reject it; bundle-root mismatch refuses; managed plan uses pushed revision and apply exact plan; direct recovery needs no source; no v1 executor or apply loop. | `cli_cluster`, `cli_cluster_e2e`, cluster deployment tests |
| Observation | Concurrent successor cannot satisfy original wait; lost submission reply/timeout observes original ID without resubmission, with bounded polls. | `system_remote`, `cli_cluster`, server deployment tests |
| Aliases and migration | Bound aliases cannot retarget; name collisions and credential owners; complete old-command inventory, conflict/refusal behavior, finite adapter removal and isolated old/new binary homes. | `operator.rs`, `planes.rs`, `cli_queries`, `cli_schema_config` |

Run clean focused baselines before implementation, extend those fixtures, and
run the owning runtime gates after changes. This RFC update runs documentation,
link and spelling checks only; it makes no claim that these proposed runtime
gates pass. The CLI migration must update supported user docs, help/completion
and release notes in the same implementation changes.

## Rollout and unresolved questions

1. Review the target schema, credential-reference lookup and complete command
   migration map against the baseline. Set the finite release boundary and
   amend affected accepted RFC sections when this decision is accepted.
2. Capture baseline routes/outcomes in existing tests. Implement local target
   storage, inspection and explicit migration without silently selecting a
   target or changing existing consumers.
3. Integrate the shared resolved invocation across currently supported command
   paths, then enable the new command contract only with destination/auth/effect
   evidence. Keep unsupported combinations explicit.
4. Migrate consumers, aliases and documentation through the announced interval;
   remove old selectors and ambient folder/profile routing at its end.
5. Qualify managed aliases and other new operation support individually. Track
   the served catalog and discovery follow-ups independently.

Before acceptance, settle the exact serialized target-format schema and
credential-reference mapping, full verb/flag migration inventory (including
authentication and recovery), and named release interval. The kind/scope model,
selection order, source separation, no-fallback rule and pinned observation
are proposed decisions here, not optional implementation choices.

## Upstream design evidence

Kubernetes documentation and implementation were reviewed on 2026-10-14:

- [Contexts and kubeconfig rules](https://kubernetes.io/docs/concepts/configuration/organize-cluster-access-kubeconfig/)
  inform complete bindings, explicit current selection and configuration path
  ownership. Multi-file merging and independent field overrides are excluded.
- [Configuration inspection](https://github.com/kubernetes/kubectl/blob/7795eab7acb6f8653133eb3f8b7743623ecc0b12/pkg/cmd/config/view.go)
  informs effective-target inspection. OmniGraph's diagnostic view never acts
  as a credential export or executes a credential plugin.
- [Usage conventions](https://kubernetes.io/docs/reference/kubectl/conventions)
  inform explicit automation selection and machine output.
- [Shared configuration interfaces](https://github.com/kubernetes/cli-runtime/blob/master/pkg/genericclioptions/config_flags.go)
  and [command construction](https://github.com/kubernetes/kubectl/blob/7795eab7acb6f8653133eb3f8b7743623ecc0b12/pkg/cmd/cmd.go)
  inform common client construction; immutable once-per-invocation resolution
  is this proposal's stronger requirement.
- [API dry-run](https://kubernetes.io/docs/reference/using-api/api-concepts/#dry-run)
  motivates explicit validation/planning effects. Managed saved plans are not
  labeled side-effect-free dry runs.
- [Rollout status](https://kubernetes.io/docs/reference/kubectl/generated/kubectl_rollout/kubectl_rollout_status/)
  can follow a successor by default. OmniGraph instead requires exact-operation
  observation and preserves finite operation-specific timeouts.
- [Apply's object loop](https://github.com/kubernetes/kubectl/blob/7795eab7acb6f8653133eb3f8b7743623ecc0b12/pkg/cmd/apply/apply.go#L530-L543)
  and [server-side apply](https://kubernetes.io/docs/reference/using-api/server-side-apply/)
  are explicitly not adopted as graph publication or deployment mechanisms.

## Decision log

- **2026-09-06:** Initial draft proposed managed connection variants in the
  existing profile/cluster resolver, with requirements CC-01 through CC-18.
  It did not receive acceptance or implementation status.
- **2026-09-08:** Narrow query/mutation deadline agreement proposed a 30-second
  complete-response deadline within the existing bounded transport design.
  That agreement did not accept the wider resolver or credential proposal.
- **2026-10-14:** Replaced the September body and CC requirements with the named
  target proposal after reviewing current CLI behavior and kubectl. Introduced
  workspace/service vocabulary, explicit configuration migration, offline
  inspection, shared invocation, machine-output and exact-operation observation
  contracts. Preserved current v2 identity/acquisition and deployment authority;
  removed v1 grants, retired commands, restart-only assumptions and unrelated
  public-Rust repairs. Removed the duplicate developer framework. The original
  draft remains in Git history; this revision remains a proposal.
- **2026-10-15:** Removed the proposed `workspace` command group and resource
  vocabulary from the body. Top-level `validate`, `plan`, `apply` and `push`
  operate on configuration input; `--config DIR` replaces the earlier positional
  directory examples. This supersedes the 2026-10-14 workspace naming decision.
  Target selection, managed revision/saved-plan semantics and service lifecycle
  separation are unchanged.
- **2026-10-15 — Retain existing command families:** Supersedes the earlier
  workspace/service/deployment grouping and top-level configuration verbs.
  Keep configuration, lifecycle and recovery commands under `cluster`, including
  `--rev`, exact operation identifiers and the local-source `--config` default.
  Add `target` administration and unify destination selection with `--target`.
  No new resource command groups are needed to establish that contract.
