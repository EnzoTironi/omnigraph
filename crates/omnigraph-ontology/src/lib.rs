//! Experimental decision contracts, independent of storage and authentication.
//!
//! These evaluators do not grant an engine capability. A host must establish
//! identity, capture evidence, enforce publication preconditions, and commit
//! effects with their decision record. See the crate README for that boundary.

pub mod access;
pub mod action;
pub mod delivery;
#[cfg(feature = "engine")]
pub mod engine;
pub mod evidence;
pub mod model;

use serde::Serialize;
use sha2::{Digest, Sha256};

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ContractError {
    #[error("invalid contract: {0}")]
    Invalid(String),
    #[error("decision refused: {0}")]
    Refused(String),
    #[error("contract encoding failed: {0}")]
    Encoding(String),
}

pub type Result<T> = std::result::Result<T, ContractError>;

pub(crate) fn require(condition: bool, reason: &str) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(ContractError::Invalid(reason.into()))
    }
}

pub(crate) fn name(value: &str) -> Result<()> {
    require(
        !value.is_empty()
            && value.len() <= 512
            && value.trim() == value
            && !value.chars().any(char::is_control),
        "identifiers must contain 1..512 bytes without control or surrounding whitespace",
    )
}

/// Digest of a typed contract. Object keys are sorted recursively; arrays retain
/// their declared order. The versioned record identifies this encoding.
pub fn digest(value: &impl Serialize) -> Result<String> {
    fn ordered(value: serde_json::Value) -> serde_json::Value {
        match value {
            serde_json::Value::Object(map) => {
                let sorted: std::collections::BTreeMap<_, _> = map.into_iter().collect();
                serde_json::Value::Object(
                    sorted.into_iter().map(|(k, v)| (k, ordered(v))).collect(),
                )
            }
            serde_json::Value::Array(values) => {
                serde_json::Value::Array(values.into_iter().map(ordered).collect())
            }
            other => other,
        }
    }
    let value =
        serde_json::to_value(value).map_err(|error| ContractError::Encoding(error.to_string()))?;
    let bytes = serde_json::to_vec(&ordered(value))
        .map_err(|error| ContractError::Encoding(error.to_string()))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}
