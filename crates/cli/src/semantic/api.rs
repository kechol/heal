//! Wire types for `TypeSafe`'s System One endpoint (`POST /v1/systemone`).
//!
//! One request carries a single `state` plus a map of independent typed
//! questions; the response answers every question under the same key.
//! Field names follow the published API reference verbatim
//! (<https://docs.typesafe.ai/api.md>).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// One typed question. `instructions` and `criteria` accept strings or
/// structured JSON, which is why they are carried as [`Value`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Question {
    /// Independent yes/no predicate. Returns a bare probability, no
    /// confidence. The criteria **must** be nested under `criteria`; a
    /// flat `{true, false}` at the top level is accepted by the server
    /// and silently discarded (measured by jev-lint).
    Noul {
        instructions: Value,
        #[serde(skip_serializing_if = "Option::is_none")]
        criteria: Option<NoulCriteria>,
    },
    /// Pick one of at most 255 labelled options.
    Choice {
        instructions: Value,
        criteria: BTreeMap<String, Value>,
    },
    /// Ordered rubric of 2–10 levels, clean to worst.
    Score {
        instructions: Value,
        criteria: Vec<Value>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NoulCriteria {
    #[serde(rename = "true")]
    pub yes: Value,
    #[serde(rename = "false")]
    pub no: Value,
}

impl Question {
    /// The server's cap on the number of options in one `choice`.
    pub const MAX_CHOICE_OPTIONS: usize = 255;
    /// Allowed range of levels in one `score`.
    pub const SCORE_LEVELS: std::ops::RangeInclusive<usize> = 2..=10;

    /// Reject shapes the server would refuse (or, for `noul`, accept
    /// and silently mangle) before a paid request is built.
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Self::Noul { .. } => Ok(()),
            Self::Choice { criteria, .. } => {
                if criteria.len() < 2 || criteria.len() > Self::MAX_CHOICE_OPTIONS {
                    return Err(format!(
                        "choice needs 2..={} options, got {}",
                        Self::MAX_CHOICE_OPTIONS,
                        criteria.len()
                    ));
                }
                Ok(())
            }
            Self::Score { criteria, .. } => {
                if !Self::SCORE_LEVELS.contains(&criteria.len()) {
                    return Err(format!("score needs 2..=10 levels, got {}", criteria.len()));
                }
                Ok(())
            }
        }
    }
}

/// Request body. `questions` is a `BTreeMap` so the serialized bytes are
/// deterministic, which keeps token estimates and request logs stable.
#[derive(Debug, Clone, Serialize)]
pub struct Request<'a> {
    pub model: &'a str,
    pub state: &'a Value,
    pub questions: &'a BTreeMap<String, Question>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Response {
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub answers: BTreeMap<String, Answer>,
    #[serde(default)]
    pub usage: Usage,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
pub struct Usage {
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
}

/// One answer. Persisted verbatim in the verdict store, so the serde
/// shape is part of the `.heal/semantic/` contract.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Answer {
    Noul {
        noul: f64,
    },
    Choice {
        choice: String,
        #[serde(default)]
        probabilities: BTreeMap<String, f64>,
        #[serde(default)]
        confidence: f64,
    },
    Score {
        score: f64,
        #[serde(default)]
        probabilities: BTreeMap<String, f64>,
        #[serde(default)]
        confidence: f64,
    },
}

impl Answer {
    /// Probability of the positive outcome for a `noul`, the selected
    /// option's probability for a `choice`, and the score normalised to
    /// `0..=1` for a `score` given its level count.
    #[must_use]
    pub fn strength(&self, score_levels: usize) -> f64 {
        match self {
            Self::Noul { noul } => *noul,
            Self::Choice {
                choice,
                probabilities,
                ..
            } => probabilities.get(choice).copied().unwrap_or(0.0),
            Self::Score { score, .. } => {
                let top = score_levels.saturating_sub(1).max(1);
                #[allow(clippy::cast_precision_loss)]
                let top = top as f64;
                (score / top).clamp(0.0, 1.0)
            }
        }
    }

    /// Provider confidence. `noul` has none; its distance from 0.5 is
    /// used instead so every answer can be routed on one axis.
    #[must_use]
    pub fn confidence(&self) -> f64 {
        match self {
            Self::Noul { noul } => (noul - 0.5).abs() * 2.0,
            Self::Choice { confidence, .. } | Self::Score { confidence, .. } => *confidence,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn noul_criteria_serialize_nested() {
        let q = Question::Noul {
            instructions: json!("Is it?"),
            criteria: Some(NoulCriteria {
                yes: json!("yes"),
                no: json!("no"),
            }),
        };
        let v = serde_json::to_value(&q).unwrap();
        assert_eq!(v["type"], "noul");
        assert_eq!(v["criteria"]["true"], "yes");
        assert!(v.get("true").is_none());
    }

    #[test]
    fn choice_rejects_too_few_or_too_many_options() {
        let mut criteria = BTreeMap::new();
        criteria.insert("a".to_owned(), json!("A"));
        let q = Question::Choice {
            instructions: json!("pick"),
            criteria: criteria.clone(),
        };
        assert!(q.validate().is_err());
        for i in 0..300 {
            criteria.insert(format!("o{i}"), json!("x"));
        }
        let q = Question::Choice {
            instructions: json!("pick"),
            criteria,
        };
        assert!(q.validate().is_err());
    }

    #[test]
    fn parses_every_answer_type() {
        let body = json!({
            "model": "jev-1.13.0",
            "answers": {
                "a": {"type": "noul", "noul": 0.8},
                "b": {"type": "choice", "choice": "x", "probabilities": {"x": 0.7, "y": 0.3}, "confidence": 0.6},
                "c": {"type": "score", "score": 2.0, "legend": {"0": "n/a"}, "probabilities": {"2": 0.9}, "confidence": 0.9}
            },
            "usage": {"input_tokens": 120, "output_tokens": 4}
        });
        let r: Response = serde_json::from_value(body).unwrap();
        assert_eq!(r.usage.input_tokens, 120);
        assert!((r.answers["a"].strength(0) - 0.8).abs() < 1e-9);
        assert!((r.answers["b"].strength(0) - 0.7).abs() < 1e-9);
        assert!((r.answers["c"].strength(4) - 2.0 / 3.0).abs() < 1e-9);
        assert!((r.answers["a"].confidence() - 0.6).abs() < 1e-9);
    }
}
