//! The detector output contract, from Maplesyrup: a value or the reason
//! there is none, how sure, which detector, and what kind of evidence.

use serde::{Deserialize, Serialize};

use crate::confidence::{Confidence, Reliability};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Detection<T> {
    pub value: Option<T>,
    pub confidence: Confidence,
    /// Frame time the detection belongs to, in milliseconds.
    pub timestamp_ms: u64,
    /// Which detector (or plugin) produced it.
    pub source: String,
    pub reliability: Reliability,
    pub failure_reason: Option<String>,
}

impl<T> Detection<T> {
    pub fn found(
        value: T,
        confidence: Confidence,
        timestamp_ms: u64,
        source: impl Into<String>,
        reliability: Reliability,
    ) -> Self {
        Detection {
            value: Some(value),
            confidence,
            timestamp_ms,
            source: source.into(),
            reliability,
            failure_reason: None,
        }
    }

    pub fn missing(timestamp_ms: u64, source: impl Into<String>, reason: impl Into<String>) -> Self {
        Detection {
            value: None,
            confidence: Confidence::NONE,
            timestamp_ms,
            source: source.into(),
            reliability: Reliability::Unreliable,
            failure_reason: Some(reason.into()),
        }
    }

    pub fn is_present(&self) -> bool {
        self.value.is_some()
    }

    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Detection<U> {
        Detection {
            value: self.value.map(f),
            confidence: self.confidence,
            timestamp_ms: self.timestamp_ms,
            source: self.source,
            reliability: self.reliability,
            failure_reason: self.failure_reason,
        }
    }

    /// The same value, carried to a later frame with its confidence decayed.
    pub fn carried(mut self, timestamp_ms: u64, decay: f32) -> Self {
        self.timestamp_ms = timestamp_ms;
        self.confidence = self.confidence.decay(decay);
        if self.value.is_some() {
            self.reliability = Reliability::Predicted;
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_detection_says_why() {
        let d: Detection<f32> = Detection::missing(40, "bars", "no saturated run");
        assert!(!d.is_present());
        assert_eq!(d.failure_reason.as_deref(), Some("no saturated run"));
        let f = Detection::found(0.5f32, Confidence::new(0.8), 40, "bars", Reliability::Heuristic).carried(80, 0.5);
        assert_eq!(f.reliability, Reliability::Predicted);
        assert!((f.confidence.value() - 0.4).abs() < 1e-6);
    }
}
