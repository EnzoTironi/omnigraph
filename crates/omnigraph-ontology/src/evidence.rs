use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{Result, name, require};

/// The authority of one coherent observation. The host obtains every field
/// from the accepted read view, never a later head lookup.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotPin {
    /// Host-established target binding, not a globally portable graph identity.
    /// The embedded adapter uses its configured root together with the exact
    /// accepted schema and commit; it never derives schema identities from it.
    pub graph: String,
    pub branch: String,
    pub commit: String,
    pub schema: String,
}

impl SnapshotPin {
    pub fn validate(&self) -> Result<()> {
        for value in [&self.graph, &self.branch, &self.commit, &self.schema] {
            name(value)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Provenance {
    pub source: String,
    pub source_record: String,
    pub producer_version: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum Evidence {
    Present {
        value: Value,
        observed_at_ms: i64,
        recorded_at_ms: i64,
        provenance: Provenance,
    },
    Missing,
    Unknown,
    NotApplicable {
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Requirement {
    pub max_age_ms: u64,
    pub allow_not_applicable: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Readiness {
    Ready,
    Missing,
    Unknown,
    NotApplicable,
    Stale,
    Invalid,
}

impl Requirement {
    /// Freshness is relative to the observed business fact, not ingestion.
    /// Both timestamps must be known by the evaluation clock. Future facts
    /// and missing provenance cannot silently become usable evidence.
    pub fn check(&self, evidence: &Evidence, now_ms: i64) -> Readiness {
        match evidence {
            Evidence::Missing => Readiness::Missing,
            Evidence::Unknown => Readiness::Unknown,
            Evidence::NotApplicable { reason } => {
                if self.allow_not_applicable && name(reason).is_ok() {
                    Readiness::Ready
                } else {
                    Readiness::NotApplicable
                }
            }
            Evidence::Present {
                value,
                observed_at_ms,
                recorded_at_ms,
                provenance,
            } => {
                if value.is_null()
                    || *observed_at_ms > *recorded_at_ms
                    || *recorded_at_ms > now_ms
                    || name(&provenance.source).is_err()
                    || name(&provenance.source_record).is_err()
                    || name(&provenance.producer_version).is_err()
                {
                    return Readiness::Invalid;
                }
                let age = i128::from(now_ms) - i128::from(*observed_at_ms);
                if age <= i128::from(self.max_age_ms) {
                    Readiness::Ready
                } else {
                    Readiness::Stale
                }
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceSnapshot {
    pub pin: SnapshotPin,
    pub captured_at_ms: i64,
    pub items: BTreeMap<String, Evidence>,
}

impl EvidenceSnapshot {
    pub fn validate(&self, now_ms: i64) -> Result<()> {
        self.pin.validate()?;
        require(self.items.len() <= 256, "at most 256 evidence items")?;
        require(self.captured_at_ms <= now_ms, "snapshot is from the future")?;
        for key in self.items.keys() {
            name(key)?;
        }
        for item in self.items.values() {
            if let Evidence::Present { recorded_at_ms, .. } = item {
                require(
                    *recorded_at_ms <= self.captured_at_ms,
                    "evidence was recorded after its snapshot capture",
                )?;
            }
        }
        require(
            serde_json::to_vec(self)
                .map(|b| b.len() <= 524_288)
                .unwrap_or(false),
            "evidence snapshot exceeds 512 KiB",
        )?;
        Ok(())
    }
}
