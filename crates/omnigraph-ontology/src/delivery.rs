use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{Result, digest, name, require};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryState {
    Pending,
    InFlight,
    Retryable,
    Ambiguous,
    Confirmed,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Intent {
    pub decision: String,
    pub effect: String,
    pub destination: String,
    pub payload: Value,
    pub max_attempts: u32,
    pub deduplicates: bool,
    pub compensation_action: Option<String>,
}

impl Intent {
    pub fn validate(&self) -> Result<()> {
        for value in [&self.decision, &self.effect, &self.destination] {
            name(value)?;
        }
        require(
            (1..=32).contains(&self.max_attempts),
            "delivery attempts must be in 1..32",
        )?;
        require(
            serde_json::to_vec(&self.payload)
                .map(|b| b.len() <= 65_536)
                .unwrap_or(false),
            "delivery payload exceeds 64 KiB",
        )?;
        if let Some(action) = &self.compensation_action {
            name(action)?;
        }
        Ok(())
    }

    /// Stable across retries, bound to immutable destination and payload.
    pub fn key(&self) -> Result<String> {
        self.validate()?;
        digest(&(
            1_u32,
            &self.decision,
            &self.effect,
            &self.destination,
            &self.payload,
        ))
    }
}

/// A value to persist with compare-and-swap on the same authoritative intent.
/// This type performs no I/O, scheduling, or delivery on its own.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Delivery {
    pub intent: Intent,
    pub state: DeliveryState,
    pub attempts: u32,
    pub revision: u64,
    pub next_attempt_ms: i64,
    pub receipt: Option<String>,
}

impl Delivery {
    pub fn pending(intent: Intent, now_ms: i64) -> Result<Self> {
        intent.validate()?;
        Ok(Self {
            intent,
            state: DeliveryState::Pending,
            attempts: 0,
            revision: 0,
            next_attempt_ms: now_ms,
            receipt: None,
        })
    }

    fn validate(&self) -> Result<()> {
        self.intent.validate()?;
        require(
            self.attempts <= self.intent.max_attempts,
            "attempt count exceeds policy",
        )?;
        require(
            self.state != DeliveryState::Pending || self.attempts == 0,
            "pending delivery already has attempts",
        )?;
        require(
            self.state == DeliveryState::Pending || self.attempts > 0,
            "non-pending delivery has no attempt",
        )?;
        require(
            (self.state == DeliveryState::Confirmed) == self.receipt.is_some(),
            "confirmation and receipt disagree",
        )?;
        if let Some(receipt) = &self.receipt {
            name(receipt)?;
        }
        Ok(())
    }

    fn next(&self) -> Result<Self> {
        self.validate()?;
        let mut next = self.clone();
        next.revision = self
            .revision
            .checked_add(1)
            .ok_or_else(|| crate::ContractError::Invalid("delivery revision exhausted".into()))?;
        Ok(next)
    }

    /// Persist this transition before sending. A host crash after it leaves
    /// InFlight, which must be reconciled just like an ambiguous response.
    pub fn start(&self, now_ms: i64) -> Result<Self> {
        let mut next = self.next()?;
        require(
            matches!(
                self.state,
                DeliveryState::Pending | DeliveryState::Retryable
            ),
            "delivery must be pending or safely retryable",
        )?;
        require(
            now_ms >= self.next_attempt_ms,
            "delivery backoff has not elapsed",
        )?;
        require(
            self.attempts < self.intent.max_attempts,
            "delivery budget exhausted",
        )?;
        next.attempts += 1;
        next.state = DeliveryState::InFlight;
        Ok(next)
    }

    pub fn confirmed(&self, receipt: &str) -> Result<Self> {
        name(receipt)?;
        let mut next = self.next()?;
        require(
            matches!(
                self.state,
                DeliveryState::InFlight | DeliveryState::Ambiguous
            ),
            "only a sent or ambiguous delivery can be confirmed",
        )?;
        next.state = DeliveryState::Confirmed;
        next.receipt = Some(receipt.into());
        Ok(next)
    }

    pub fn response_lost(&self) -> Result<Self> {
        let mut next = self.next()?;
        require(
            self.state == DeliveryState::InFlight,
            "delivery is not in flight",
        )?;
        next.state = DeliveryState::Ambiguous;
        Ok(next)
    }

    /// The receiver proved no effect, or guarantees deduplication of this key.
    /// An ambiguous non-idempotent effect cannot become retryable by timeout.
    pub fn retry(&self, now_ms: i64, receiver_proved_absent: bool) -> Result<Self> {
        let mut next = self.next()?;
        require(
            matches!(
                self.state,
                DeliveryState::InFlight | DeliveryState::Ambiguous
            ),
            "delivery is not awaiting a receiver outcome",
        )?;
        require(
            receiver_proved_absent || self.intent.deduplicates,
            "ambiguous effect requires reconciliation before replay",
        )?;
        if self.attempts == self.intent.max_attempts {
            next.state = DeliveryState::Failed;
        } else {
            let delay = 1000_i64 * (1_i64 << self.attempts.min(10));
            next.next_attempt_ms = now_ms
                .checked_add(delay)
                .ok_or_else(|| crate::ContractError::Invalid("backoff clock overflow".into()))?;
            next.state = DeliveryState::Retryable;
        }
        Ok(next)
    }
}
