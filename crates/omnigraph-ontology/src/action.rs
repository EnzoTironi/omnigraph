use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::evidence::{Evidence, EvidenceSnapshot, Readiness, Requirement};
use crate::model::Versions;
use crate::{ContractError, Result, digest, name, require};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Principal {
    pub id: String,
    pub kind: PrincipalKind,
    pub client: Option<String>,
    pub roles: BTreeSet<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrincipalKind {
    Human,
    Agent,
    Service,
}

impl Principal {
    pub fn validate(&self) -> Result<()> {
        name(&self.id)?;
        require(self.roles.len() <= 64, "at most 64 actor roles")?;
        for role in &self.roles {
            name(role)?;
        }
        if let Some(client) = &self.client {
            name(client)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Parameter {
    String { max_bytes: usize },
    Bool,
    Integer { min: i64, max: i64 },
    Enum { values: BTreeSet<String> },
}

impl Parameter {
    fn validate(&self) -> Result<()> {
        match self {
            Self::String { max_bytes } => require(
                (1..=65_536).contains(max_bytes),
                "string parameters must be bounded to 1..65536 bytes",
            ),
            Self::Integer { min, max } => require(min <= max, "inverted integer range"),
            Self::Enum { values } => {
                require(
                    !values.is_empty() && values.len() <= 256,
                    "invalid enum size",
                )?;
                for value in values {
                    name(value)?;
                }
                Ok(())
            }
            Self::Bool => Ok(()),
        }
    }

    fn accepts(&self, value: &Value) -> bool {
        match self {
            Self::String { max_bytes } => value.as_str().is_some_and(|s| s.len() <= *max_bytes),
            Self::Bool => value.is_boolean(),
            Self::Integer { min, max } => value.as_i64().is_some_and(|n| n >= *min && n <= *max),
            Self::Enum { values } => value.as_str().is_some_and(|s| values.contains(s)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Manual,
    Automatic,
    Confirm,
    Disabled,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Predicate {
    Equals {
        evidence: String,
        value: Value,
    },
    IntegerRange {
        evidence: String,
        min: i64,
        max: i64,
    },
    EqualsParameter {
        evidence: String,
        parameter: String,
    },
}

impl Predicate {
    fn evidence(&self) -> &str {
        match self {
            Self::Equals { evidence, .. }
            | Self::IntegerRange { evidence, .. }
            | Self::EqualsParameter { evidence, .. } => evidence,
        }
    }

    fn check(&self, value: &Value, parameters: &BTreeMap<String, Value>) -> bool {
        match self {
            Self::Equals {
                value: expected, ..
            } => value == expected,
            Self::IntegerRange { min, max, .. } => {
                value.as_i64().is_some_and(|n| n >= *min && n <= *max)
            }
            Self::EqualsParameter { parameter, .. } => parameters.get(parameter) == Some(value),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Action {
    pub id: String,
    pub versions: Versions,
    pub mode: Mode,
    pub parameters: BTreeMap<String, Parameter>,
    pub requirements: BTreeMap<String, Requirement>,
    pub guards: BTreeMap<String, Predicate>,
    pub proposer_roles: BTreeSet<String>,
    pub confirmer_roles: BTreeSet<String>,
    pub proposal_ttl_ms: u64,
}

impl Action {
    pub fn validate(&self) -> Result<()> {
        name(&self.id)?;
        self.versions.validate()?;
        require(self.parameters.len() <= 64, "at most 64 parameters")?;
        require(self.requirements.len() <= 256, "at most 256 requirements")?;
        require(self.guards.len() <= 256, "at most 256 guards")?;
        require(
            !self.proposer_roles.is_empty(),
            "action requires proposer roles",
        )?;
        require(self.proposer_roles.len() <= 64, "at most 64 proposer roles")?;
        require(
            self.confirmer_roles.len() <= 64,
            "at most 64 confirmer roles",
        )?;
        require(
            !matches!(self.mode, Mode::Confirm | Mode::Manual) || !self.confirmer_roles.is_empty(),
            "human actions require confirmer roles",
        )?;
        require(
            self.proposal_ttl_ms > 0 && self.proposal_ttl_ms <= 604_800_000,
            "proposal lifetime must be in 1ms..7 days",
        )?;
        for (key, parameter) in &self.parameters {
            name(key)?;
            parameter.validate()?;
        }
        for key in self.requirements.keys() {
            name(key)?;
        }
        for role in self.proposer_roles.iter().chain(&self.confirmer_roles) {
            name(role)?;
        }
        for (id, guard) in &self.guards {
            name(id)?;
            require(
                self.requirements.contains_key(guard.evidence()),
                "guard evidence is undeclared",
            )?;
            if let Predicate::IntegerRange { min, max, .. } = guard {
                require(min <= max, "inverted guard range")?;
            }
            if let Predicate::EqualsParameter { parameter, .. } = guard {
                require(
                    self.parameters.contains_key(parameter),
                    "guard parameter is undeclared",
                )?;
            }
        }
        Ok(())
    }

    pub(crate) fn validate_parameters(&self, parameters: &BTreeMap<String, Value>) -> Result<()> {
        require(
            self.parameters.len() == parameters.len(),
            "parameter set differs from contract",
        )?;
        for (key, spec) in &self.parameters {
            require(
                parameters.get(key).is_some_and(|v| spec.accepts(v)),
                "parameter is missing or outside its declared type/domain",
            )?;
        }
        Ok(())
    }

    fn evaluate(
        &self,
        inputs: &EvidenceSnapshot,
        params: &BTreeMap<String, Value>,
        now: i64,
    ) -> BTreeMap<String, Check> {
        let mut checks = BTreeMap::new();
        for (key, requirement) in &self.requirements {
            let status =
                requirement.check(inputs.items.get(key).unwrap_or(&Evidence::Missing), now);
            checks.insert(format!("evidence:{key}"), Check::Readiness(status));
        }
        for (key, guard) in &self.guards {
            let ready = checks
                .get(&format!("evidence:{}", guard.evidence()))
                .is_some_and(Check::passed);
            let outcome = match (ready, inputs.items.get(guard.evidence())) {
                (true, Some(Evidence::Present { value, .. })) => {
                    Check::Predicate(guard.check(value, params))
                }
                _ => Check::Unavailable,
            };
            checks.insert(format!("guard:{key}"), outcome);
        }
        checks
    }

    /// Identity and role membership must come from the host's authenticated
    /// context. Evidence must come from the same accepted graph read view.
    pub fn propose(
        &self,
        id: &str,
        actor: Principal,
        parameters: BTreeMap<String, Value>,
        inputs: EvidenceSnapshot,
        now_ms: i64,
    ) -> Result<Decision> {
        self.validate()?;
        name(id)?;
        actor.validate()?;
        self.validate_parameters(&parameters)?;
        inputs.validate(now_ms)?;
        let authorized = !actor.roles.is_disjoint(&self.proposer_roles)
            && self.mode != Mode::Disabled
            && (self.mode != Mode::Manual || actor.kind == PrincipalKind::Human);
        let mut checks = if authorized {
            self.evaluate(&inputs, &parameters, now_ms)
        } else {
            BTreeMap::new()
        };
        checks.insert("policy:proposer".into(), Check::Predicate(authorized));
        checks.insert(
            "policy:enabled".into(),
            Check::Predicate(self.mode != Mode::Disabled),
        );
        let outcome = failed_checks(&checks).unwrap_or(if self.mode == Mode::Automatic {
            Outcome::Allowed
        } else {
            Outcome::Review
        });
        let decision = Decision {
            format: 1,
            id: id.into(),
            action: self.id.clone(),
            action_digest: digest(self)?,
            versions: self.versions.clone(),
            parameters,
            proposed_by: actor,
            confirmed_by: None,
            proposed_at_ms: now_ms,
            evaluated_at_ms: now_ms,
            inputs,
            checks,
            outcome,
            previous: None,
        };
        decision.validate_size()?;
        Ok(decision)
    }

    /// Reuse the evaluator at native admission boundaries. This checks the
    /// materialized evidence, not a fresh independently captured snapshot.
    #[cfg(feature = "engine")]
    pub(crate) fn revalidate_at(&self, decision: &Decision, now_ms: i64) -> Result<()> {
        require(
            now_ms >= decision.evaluated_at_ms,
            "admission clock moved backwards",
        )?;
        require(
            decision.action_digest == digest(self)?,
            "action changed before admission",
        )?;
        decision.inputs.validate(now_ms)?;
        require(
            failed_checks(&self.evaluate(&decision.inputs, &decision.parameters, now_ms)).is_none(),
            "evidence expired before admission",
        )?;
        if decision.previous.is_some() || decision.outcome == Outcome::Review {
            let age = i128::from(now_ms) - i128::from(decision.proposed_at_ms);
            require(
                age >= 0 && age < i128::from(self.proposal_ttl_ms),
                "proposal expired before admission",
            )?;
        }
        Ok(())
    }

    /// A confirmation produces a new record and re-evaluates current evidence.
    /// The host must load `proposal` from authoritative storage and atomically
    /// consume it with the decision and business effects. A client-supplied
    /// serialized proposal is never sufficient authority.
    pub fn confirm(
        &self,
        id: &str,
        proposal: &Decision,
        confirmer: Principal,
        inputs: EvidenceSnapshot,
        now_ms: i64,
    ) -> Result<Decision> {
        self.validate()?;
        proposal.validate_size()?;
        name(id)?;
        confirmer.validate()?;
        inputs.validate(now_ms)?;
        require(
            proposal.outcome == Outcome::Review,
            "proposal is not pending review",
        )?;
        require(
            matches!(self.mode, Mode::Confirm | Mode::Manual),
            "action does not accept confirmation",
        )?;
        require(
            proposal.action_digest == digest(self)? && proposal.action == self.id,
            "action changed; a new proposal is required",
        )?;
        require(
            proposal.format == 1 && proposal.versions == self.versions,
            "unsupported proposal or version drift",
        )?;
        require(
            proposal.confirmed_by.is_none() && proposal.previous.is_none(),
            "a confirmation cannot be used as another proposal",
        )?;
        proposal.proposed_by.validate()?;
        proposal.inputs.validate(proposal.proposed_at_ms)?;
        let expected = self.propose(
            &proposal.id,
            proposal.proposed_by.clone(),
            proposal.parameters.clone(),
            proposal.inputs.clone(),
            proposal.proposed_at_ms,
        )?;
        require(
            expected == *proposal,
            "stored proposal disagrees with its action contract",
        )?;
        self.validate_parameters(&proposal.parameters)?;
        require(
            proposal.inputs.pin.graph == inputs.pin.graph
                && proposal.inputs.pin.branch == inputs.pin.branch
                && proposal.inputs.pin.schema == inputs.pin.schema,
            "confirmation targets another graph, branch or schema",
        )?;
        require(
            id != proposal.id,
            "confirmation must have its own record id",
        )?;
        require(
            now_ms >= proposal.proposed_at_ms,
            "clock moved before proposal",
        )?;
        let age = i128::from(now_ms) - i128::from(proposal.proposed_at_ms);
        let authorized = !confirmer.roles.is_disjoint(&self.confirmer_roles)
            && confirmer.kind == PrincipalKind::Human;
        let mut checks = if authorized {
            self.evaluate(&inputs, &proposal.parameters, now_ms)
        } else {
            BTreeMap::new()
        };
        checks.insert("policy:confirmer".into(), Check::Predicate(authorized));
        checks.insert(
            "policy:separation".into(),
            Check::Predicate(confirmer.id != proposal.proposed_by.id),
        );
        checks.insert(
            "policy:lifetime".into(),
            Check::Predicate(age < i128::from(self.proposal_ttl_ms)),
        );
        let outcome = failed_checks(&checks).unwrap_or(Outcome::Allowed);
        let decision = Decision {
            id: id.into(),
            confirmed_by: Some(confirmer),
            inputs,
            checks,
            evaluated_at_ms: now_ms,
            outcome,
            previous: Some(proposal.id.clone()),
            ..proposal.clone()
        };
        decision.validate_size()?;
        Ok(decision)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Allowed,
    Review,
    Denied,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum Check {
    Readiness(Readiness),
    Predicate(bool),
    Unavailable,
}

fn failed_checks(checks: &BTreeMap<String, Check>) -> Option<Outcome> {
    if checks.values().any(|check| {
        matches!(
            check,
            Check::Predicate(false)
                | Check::Readiness(
                    Readiness::Missing
                        | Readiness::NotApplicable
                        | Readiness::Stale
                        | Readiness::Invalid
                )
        )
    }) {
        Some(Outcome::Denied)
    } else if checks.values().any(|check| !check.passed()) {
        Some(Outcome::Unknown)
    } else {
        None
    }
}

impl Check {
    pub fn passed(&self) -> bool {
        matches!(
            self,
            Self::Readiness(Readiness::Ready) | Self::Predicate(true)
        )
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Decision {
    pub format: u32,
    pub id: String,
    pub action: String,
    pub action_digest: String,
    pub versions: Versions,
    pub parameters: BTreeMap<String, Value>,
    pub proposed_by: Principal,
    pub confirmed_by: Option<Principal>,
    pub proposed_at_ms: i64,
    pub evaluated_at_ms: i64,
    pub inputs: EvidenceSnapshot,
    pub checks: BTreeMap<String, Check>,
    pub outcome: Outcome,
    pub previous: Option<String>,
}

impl Decision {
    pub const MAX_BYTES: usize = 1_048_576;

    pub fn validate_size(&self) -> Result<()> {
        let size = serde_json::to_vec(self)
            .map_err(|e| ContractError::Encoding(e.to_string()))?
            .len();
        require(size <= Self::MAX_BYTES, "decision exceeds 1 MiB")
    }

    /// Descriptive output only: publication must also enforce the host's
    /// current authority, action contract, and unique decision identity.
    pub fn allows_effects(&self) -> bool {
        self.outcome == Outcome::Allowed
    }
}
