//! Embedded reference host for constructive actions. This adapter has no HTTP
//! authentication boundary: its caller supplies a previously verified principal.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use omnigraph::Session;
use omnigraph_compiler::ir::ParamMap;
use omnigraph_compiler::query::ast::{Expr, Literal, Mutation, MutationWriteMode};
use omnigraph_compiler::types::ScalarType;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::action::{Action, Decision, Mode, Outcome, Principal, PrincipalKind};
use crate::evidence::{Evidence, EvidenceSnapshot, Provenance, SnapshotPin};
use crate::{ContractError, digest, name, require};

pub const RECORD_SCHEMA: &str = r#"
node OntologyDecision {
    decision_id: String @unique
    record: String
    consumes: String? @unique
}
"#;

const RECORD_QUERY: &str = r#"
query ontology_record($og_id: String) {
    match { $d: OntologyDecision { decision_id: $og_id } }
    return { $d.record as record }
}
"#;

const RECORD_MUTATION: &str = r#"
query ontology_record($og_id: String, $og_record: String, $og_consumes: String?) {
    insert OntologyDecision { decision_id: $og_id, record: $og_record, consumes: $og_consumes }
}
"#;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Contract(#[from] ContractError),
    #[error(transparent)]
    Engine(Box<omnigraph::error::OmniError>),
}

impl From<omnigraph::error::OmniError> for Error {
    fn from(error: omnigraph::error::OmniError) -> Self {
        Self::Engine(Box::new(error))
    }
}

pub type Result<T> = std::result::Result<T, Error>;

pub trait Clock: Send + Sync {
    fn now_ms(&self) -> crate::Result<i64>;
}

pub struct SystemClock;

impl Clock for SystemClock {
    fn now_ms(&self) -> crate::Result<i64> {
        let elapsed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| ContractError::Invalid("clock precedes Unix epoch".into()))?;
        i64::try_from(elapsed.as_millis())
            .map_err(|_| ContractError::Invalid("clock exceeds millisecond range".into()))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Query {
    pub source: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceColumn {
    pub value: String,
    pub observed_at_ms: String,
    pub recorded_at_ms: String,
    pub source_record: String,
    pub source: String,
    pub producer_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub evidence: Query,
    pub effect: Query,
    pub columns: BTreeMap<String, EvidenceColumn>,
}

/// One immutable registered action. The session is deliberately private.
/// A deployment must keep its writer credential private and give callers no
/// alternate path to mutate these business or audit rows.
pub struct EngineAction {
    session: Session,
    action: Action,
    binding: Binding,
    branch: String,
    writer: String,
    schema_hash: String,
    clock: Arc<dyn Clock>,
}

#[derive(Debug, Clone)]
pub struct Execution {
    pub decision: Decision,
    pub commit_id: Option<String>,
    pub replayed: bool,
}

impl EngineAction {
    pub fn new(
        session: Session,
        mut action: Action,
        binding: Binding,
        branch: &str,
        writer: &str,
        clock: Arc<dyn Clock>,
    ) -> Result<Self> {
        action.validate()?;
        name(branch)?;
        name(writer)?;
        require(
            binding.columns.len() == action.requirements.len()
                && binding.columns.keys().eq(action.requirements.keys()),
            "evidence bindings must match requirements exactly",
        )?;
        require(
            !action.parameters.keys().any(|key| key.starts_with("og_")),
            "og_ parameters are reserved for the host",
        )?;
        require(
            binding.evidence.source.len() <= 65_536 && binding.effect.source.len() <= 65_536,
            "registered query exceeds 64 KiB",
        )?;
        for column in binding.columns.values() {
            for value in [
                &column.value,
                &column.observed_at_ms,
                &column.recorded_at_ms,
                &column.source_record,
                &column.source,
                &column.producer_version,
            ] {
                name(value)?;
            }
        }
        validate_effect(&binding.effect)?;
        let catalog = session.catalog();
        validate_record_schema(&catalog)?;
        let accepted = catalog.bound_schema_ir().ok_or_else(|| {
            ContractError::Invalid("registered catalog lacks accepted schema identity".into())
        })?;
        let schema_hash = omnigraph_compiler::schema_ir_hash(accepted)
            .map_err(|error| ContractError::Invalid(error.to_string()))?;
        let read = omnigraph_compiler::find_read_statement(
            &binding.evidence.source,
            &binding.evidence.name,
        )
        .map_err(|error| ContractError::Invalid(error.to_string()))?;
        require(
            !read.is_explain(),
            "action evidence must be a read, not an explain",
        )?;
        let checked = omnigraph_compiler::CheckedRead::check(&catalog, read.decl())
            .map_err(|error| ContractError::Invalid(error.to_string()))?;
        let ir = omnigraph_compiler::lower_query(&catalog, checked.query())
            .map_err(|error| ContractError::Invalid(error.to_string()))?;
        require(
            !ir.has_wildcard_traversal(),
            "confirmation evidence needs explicit edge types",
        )?;
        // Pin the actual registered sources, not just a human-authored version label.
        action.versions.logic = digest(&(action.versions.logic.as_str(), &binding))?;
        Ok(Self {
            session,
            action,
            binding,
            branch: branch.into(),
            writer: writer.into(),
            schema_hash,
            clock,
        })
    }

    pub fn action(&self) -> &Action {
        &self.action
    }

    /// Retries with the same id resolve the stored decision and never execute
    /// its effect twice. A conflict reusing an id with different inputs refuses.
    pub async fn submit(
        &self,
        id: &str,
        actor: Principal,
        parameters: BTreeMap<String, Value>,
    ) -> Result<Execution> {
        name(id)?;
        actor.validate()?;
        if actor.roles.is_disjoint(&self.action.proposer_roles)
            || self.action.mode == Mode::Disabled
            || (self.action.mode == Mode::Manual && actor.kind != PrincipalKind::Human)
        {
            return Err(ContractError::Refused("actor cannot propose this action".into()).into());
        }
        self.action.validate_parameters(&parameters)?;
        if let Some(stored) = self.lookup(id).await? {
            require(
                stored.action_digest == digest(&self.action)?
                    && stored.proposed_by == actor
                    && stored.parameters == parameters
                    && stored.previous.is_none(),
                "request id reused with different inputs",
            )?;
            return Ok(Execution {
                decision: stored,
                commit_id: None,
                replayed: true,
            });
        }
        let inputs = self.capture(&parameters).await?;
        let decision = self
            .action
            .propose(id, actor, parameters, inputs, self.clock.now_ms()?)?;
        self.publish(decision).await
    }

    /// The proposal is read from the graph, never accepted from the client.
    pub async fn confirm(
        &self,
        id: &str,
        proposal_id: &str,
        actor: Principal,
    ) -> Result<Execution> {
        name(id)?;
        name(proposal_id)?;
        actor.validate()?;
        if actor.roles.is_disjoint(&self.action.confirmer_roles)
            || actor.kind != PrincipalKind::Human
            || !matches!(self.action.mode, Mode::Confirm | Mode::Manual)
        {
            return Err(ContractError::Refused("actor cannot confirm this action".into()).into());
        }
        if let Some(stored) = self.lookup(id).await? {
            require(
                stored.action_digest == digest(&self.action)?
                    && stored.confirmed_by.as_ref() == Some(&actor)
                    && stored.previous.as_deref() == Some(proposal_id),
                "request id reused with another confirmation",
            )?;
            return Ok(Execution {
                decision: stored,
                commit_id: None,
                replayed: true,
            });
        }
        let (proposal, head) = self
            .lookup_pinned(proposal_id)
            .await?
            .ok_or_else(|| ContractError::Refused("proposal not found".into()))?;
        let inputs = self.capture_at(&proposal.parameters, Some(&head)).await?;
        let decision = self
            .action
            .confirm(id, &proposal, actor, inputs, self.clock.now_ms()?)?;
        self.publish(decision).await
    }

    async fn lookup(&self, id: &str) -> Result<Option<Decision>> {
        Ok(self.lookup_pinned(id).await?.map(|(decision, _)| decision))
    }

    async fn lookup_pinned(&self, id: &str) -> Result<Option<(Decision, String)>> {
        let parameters = ParamMap::from([("og_id".into(), Literal::String(id.into()))]);
        let read = self
            .session
            .query_observed(
                self.branch.as_str(),
                RECORD_QUERY,
                "ontology_record",
                &parameters,
            )
            .await?;
        require(
            read.schema_ir_hash == self.schema_hash,
            "registered schema changed",
        )?;
        require(
            read.result.num_rows() <= 1,
            "decision identity is not unique",
        )?;
        let rows = read
            .result
            .to_rust_json()
            .map_err(omnigraph::error::OmniError::from)?;
        let rows = rows
            .as_array()
            .ok_or_else(|| ContractError::Invalid("record result is not rows".into()))?;
        require(rows.len() <= 1, "decision identity is not unique")?;
        rows.first()
            .map(|row| {
                let record = row.get("record").and_then(Value::as_str).ok_or_else(|| {
                    ContractError::Invalid("stored dossier is not a string".into())
                })?;
                require(
                    record.len() <= Decision::MAX_BYTES,
                    "stored dossier exceeds limit",
                )?;
                let decision: Decision = serde_json::from_str(record)
                    .map_err(|e| ContractError::Encoding(e.to_string()))?;
                require(
                    decision.format == 1 && decision.id == id,
                    "invalid stored decision identity",
                )?;
                let head = read.graph_commit_id.clone().ok_or_else(|| {
                    ContractError::Invalid("stored proposal has no graph authority".into())
                })?;
                Ok((decision, head))
            })
            .transpose()
    }

    async fn capture(&self, parameters: &BTreeMap<String, Value>) -> Result<EvidenceSnapshot> {
        self.capture_at(parameters, None).await
    }

    async fn capture_at(
        &self,
        parameters: &BTreeMap<String, Value>,
        head: Option<&str>,
    ) -> Result<EvidenceSnapshot> {
        let params = literals(parameters)?;
        let target = match head {
            Some(head) => omnigraph::db::ReadTarget::Snapshot(omnigraph::db::SnapshotId::new(head)),
            None => omnigraph::db::ReadTarget::branch(&self.branch),
        };
        let read = self
            .session
            .query_observed(
                target,
                &self.binding.evidence.source,
                &self.binding.evidence.name,
                &params,
            )
            .await?;
        require(
            read.schema_ir_hash == self.schema_hash,
            "registered schema changed",
        )?;
        require(
            read.result.num_rows() <= 1,
            "evidence query must return at most one row",
        )?;
        let rows = read
            .result
            .to_rust_json()
            .map_err(omnigraph::error::OmniError::from)?;
        let rows = rows
            .as_array()
            .ok_or_else(|| ContractError::Invalid("evidence result is not rows".into()))?;
        require(
            rows.len() <= 1,
            "evidence query must return at most one row",
        )?;
        let mut items = BTreeMap::new();
        for (key, column) in &self.binding.columns {
            let evidence = match rows.first() {
                None => Evidence::Missing,
                Some(row) => match row.get(&column.value) {
                    None | Some(Value::Null) => Evidence::Missing,
                    Some(value) => Evidence::Present {
                        value: value.clone(),
                        observed_at_ms: row
                            .get(&column.observed_at_ms)
                            .and_then(Value::as_i64)
                            .ok_or_else(|| {
                                ContractError::Invalid("invalid observed time".into())
                            })?,
                        recorded_at_ms: row
                            .get(&column.recorded_at_ms)
                            .and_then(Value::as_i64)
                            .ok_or_else(|| {
                                ContractError::Invalid("invalid recorded time".into())
                            })?,
                        provenance: Provenance {
                            source: column.source.clone(),
                            source_record: row
                                .get(&column.source_record)
                                .and_then(Value::as_str)
                                .ok_or_else(|| {
                                    ContractError::Invalid("invalid source identity".into())
                                })?
                                .into(),
                            producer_version: column.producer_version.clone(),
                        },
                    },
                },
            };
            items.insert(key.clone(), evidence);
        }
        Ok(EvidenceSnapshot {
            pin: SnapshotPin {
                graph: self.session.uri().into(),
                branch: self.branch.clone(),
                commit: read.graph_commit_id.ok_or_else(|| {
                    ContractError::Invalid("action needs an initialized graph commit".into())
                })?,
                schema: read.schema_ir_hash,
            },
            captured_at_ms: self.clock.now_ms()?,
            items,
        })
    }

    async fn publish(&self, decision: Decision) -> Result<Execution> {
        // The complete dossier is retained as ordinary graph data. Uniqueness
        // of consumes makes an allowed confirmation a one-time transition.
        let mut params = if decision.allows_effects() {
            literals(&decision.parameters)?
        } else {
            ParamMap::new()
        };
        params.insert("og_id".into(), Literal::String(decision.id.clone()));
        params.insert(
            "og_record".into(),
            Literal::String(
                serde_json::to_string(&decision)
                    .map_err(|e| ContractError::Encoding(e.to_string()))?,
            ),
        );
        params.insert(
            "og_consumes".into(),
            match (&decision.previous, decision.outcome) {
                (Some(id), Outcome::Allowed) => Literal::String(id.clone()),
                _ => Literal::Null,
            },
        );
        let (source, query) = if decision.allows_effects() {
            (
                self.binding.effect.source.as_str(),
                self.binding.effect.name.as_str(),
            )
        } else {
            (RECORD_MUTATION, "ontology_record")
        };
        let receipt = self
            .session
            .mutate_as_with_expected_head_receipt(
                &self.branch,
                source,
                query,
                &params,
                Some(&self.writer),
                Some(&decision.inputs.pin.commit),
            )
            .await?;
        let commit = receipt
            .commit
            .ok_or_else(|| ContractError::Invalid("decision append produced no commit".into()))?;
        Ok(Execution {
            decision,
            commit_id: Some(commit.graph_commit_id),
            replayed: false,
        })
    }
}

fn literals(parameters: &BTreeMap<String, Value>) -> crate::Result<ParamMap> {
    parameters
        .iter()
        .map(|(key, value)| {
            let literal = match value {
                Value::String(v) => Literal::String(v.clone()),
                Value::Bool(v) => Literal::Bool(*v),
                Value::Number(v) => Literal::Integer(v.as_i64().ok_or_else(|| {
                    ContractError::Invalid("integer parameter outside I64".into())
                })?),
                _ => {
                    return Err(ContractError::Invalid(
                        "unsupported action parameter value".into(),
                    ));
                }
            };
            Ok((key.clone(), literal))
        })
        .collect()
}

fn validate_record_schema(catalog: &omnigraph_compiler::catalog::Catalog) -> crate::Result<()> {
    require(
        !catalog.edge_types.contains_key("OntologyDecision"),
        "the dossier type cannot share an edge namespace",
    )?;
    let node = catalog
        .node_types
        .get("OntologyDecision")
        .ok_or_else(|| ContractError::Invalid("OntologyDecision schema is required".into()))?;
    require(
        node.key.is_none(),
        "decision records must be strict inserts, not upserts",
    )?;
    for field in ["decision_id", "consumes"] {
        require(
            node.unique_constraints.iter().any(|c| c == &[field]),
            "decision ids and consumed proposals require native uniqueness",
        )?;
    }
    for (field, nullable) in [
        ("decision_id", false),
        ("record", false),
        ("consumes", true),
    ] {
        require(
            node.properties.get(field).is_some_and(|property| {
                property.scalar == ScalarType::String
                    && property.nullable == nullable
                    && !property.list
            }),
            "decision schema has incompatible field types",
        )?;
    }
    Ok(())
}

fn validate_effect(query: &Query) -> crate::Result<()> {
    let decl = omnigraph_compiler::find_named_query(&query.source, &query.name)
        .map_err(|error| ContractError::Invalid(error.to_string()))?;
    require(
        decl.write_mode == MutationWriteMode::Publish,
        "action effects must publish directly",
    )?;
    let mut record_count = 0;
    for mutation in &decl.mutations {
        match mutation {
            Mutation::Insert(insert) if insert.type_name == "OntologyDecision" => {
                record_count += 1;
                require(
                    insert.assignments.len() == 3,
                    "decision append needs exact host fields",
                )?;
                for (field, param) in [
                    ("decision_id", "og_id"),
                    ("record", "og_record"),
                    ("consumes", "og_consumes"),
                ] {
                    require(
                        insert
                            .assignments
                            .iter()
                            .filter(|a| {
                                a.property == field
                                    && matches!(&a.value, Expr::Variable(name) if name == param)
                            })
                            .count()
                            == 1,
                        "decision fields must come from reserved host parameters",
                    )?;
                }
            }
            Mutation::Update(update) => require(
                update.type_name != "OntologyDecision",
                "an action cannot rewrite audit history",
            )?,
            Mutation::Delete(_) => {
                return Err(ContractError::Invalid(
                    "destructive actions need an atomic changeset extension".into(),
                ));
            }
            _ => (),
        }
    }
    require(
        record_count == 1,
        "effect must append exactly one decision record",
    )
}
