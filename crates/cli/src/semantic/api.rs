//! Wire types for `TypeSafe`'s System One endpoint (`POST /v1/systemone`).
//!
//! One request carries a single `state` plus a map of independent typed
//! questions; the response answers every question under the same key.
//! Field names follow the published API reference verbatim
//! (<https://docs.typesafe.ai/api.md>); the response shape below was
//! confirmed against the live API (`jev-1.13.0`, 2026-09-23).

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
    /// `score` is the probability-weighted mean of the 0-based levels,
    /// so it is fractional: measured 2.38 for level probabilities
    /// `{0: 0.01, 1: 0.03, 2: 0.54, 3: 0.42}`. Thresholds compare it as
    /// a real number (`>= 2.5` for "level 3"), never as an index. The
    /// server's `legend` (level → criterion text) is not kept.
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

    /// A response body recorded verbatim from the live API (2026-09-23),
    /// one question of each type.
    const LIVE_RESPONSE: &str = r#"{"model":"jev-1.13.0","answers":{"n1":{"type":"noul","noul":0.53},"c1":{"type":"choice","choice":"misleading","confidence":1.0,"probabilities":{"misleading":1.0,"accurate":0.0,"vague":0.0}},"s1":{"type":"score","score":2.38,"confidence":0.53,"legend":{"0":"All three agree.","1":"Minor wording mismatch.","2":"One of the three disagrees with the others.","3":"Name and body disagree in meaning."},"probabilities":{"0":0.01,"1":0.03,"2":0.54,"3":0.42}}},"usage":{"input_tokens":508,"output_tokens":78}}"#;

    #[test]
    fn parses_a_live_response_of_every_answer_type() {
        let r: Response = serde_json::from_str(LIVE_RESPONSE).unwrap();
        assert_eq!(r.model.as_deref(), Some("jev-1.13.0"));
        assert_eq!(r.usage.input_tokens, 508);
        assert_eq!(r.usage.output_tokens, 78);
        assert!((r.answers["n1"].strength(0) - 0.53).abs() < 1e-9);
        assert!((r.answers["n1"].confidence() - 0.06).abs() < 1e-9);
        assert!((r.answers["c1"].strength(0) - 1.0).abs() < 1e-9);
        let Answer::Score {
            score,
            probabilities,
            confidence,
        } = &r.answers["s1"]
        else {
            panic!("s1 is a score");
        };
        // The score is the expected level, not an index.
        let expected: f64 = probabilities
            .iter()
            .map(|(level, p)| level.parse::<f64>().unwrap() * p)
            .sum();
        assert!((score - expected).abs() < 0.02);
        assert!((confidence - 0.53).abs() < 1e-9);
        assert!((r.answers["s1"].strength(4) - 2.38 / 3.0).abs() < 1e-9);
    }
}
