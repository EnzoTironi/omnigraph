use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::evidence::EvidenceSnapshot;
use crate::{Result, name, require};

/// A business version's valid interval is separate from when it was recorded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BusinessVersion {
    pub id: String,
    pub recorded_at_ms: i64,
    pub valid_from_ms: i64,
    pub valid_until_ms: Option<i64>,
    pub approved: bool,
}

impl BusinessVersion {
    pub fn applicable(&self, valid_at_ms: i64, known_at_ms: i64) -> bool {
        name(&self.id).is_ok()
            && self.approved
            && self.recorded_at_ms <= known_at_ms
            && self.valid_from_ms <= valid_at_ms
            && self
                .valid_until_ms
                .is_none_or(|end| end > self.valid_from_ms && valid_at_ms < end)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Versions {
    pub rule: String,
    pub logic: String,
    pub model: Option<String>,
}

impl Versions {
    pub fn validate(&self) -> Result<()> {
        name(&self.rule)?;
        name(&self.logic)?;
        if let Some(model) = &self.model {
            name(model)?;
        }
        Ok(())
    }
}

/// A prediction is evidence with an explicit input and applicability contract.
/// Confidence alone does not authorize any action.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Prediction {
    pub id: String,
    pub value: Value,
    pub uncertainty: BTreeMap<String, Value>,
    pub inputs: EvidenceSnapshot,
    pub model_version: String,
    pub in_scope: bool,
    pub ready: bool,
}

impl Prediction {
    pub fn validate(&self) -> Result<()> {
        name(&self.id)?;
        name(&self.model_version)?;
        require(!self.value.is_null(), "prediction must have a value")?;
        require(
            self.uncertainty.len() <= 32,
            "at most 32 uncertainty fields",
        )?;
        self.inputs.validate(self.inputs.captured_at_ms)
    }
}
