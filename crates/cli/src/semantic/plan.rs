//! Packing a task's groups into requests under the two token ceilings.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::Value;

use crate::semantic::api::Question;
use crate::semantic::cost::{estimate_tokens, request_budget, state_budget};
use crate::semantic::task::Item;

/// One request to send.
#[derive(Debug, Clone)]
pub struct Batch {
    pub state: Arc<Value>,
    pub questions: BTreeMap<String, Question>,
    pub est_tokens: usize,
}

/// Why a group could not be packed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackError {
    /// The shared state alone is over the state budget. Splitting the
    /// questions cannot help; the task must send a smaller state.
    StateTooLarge { tokens: usize },
}

/// Split `items` into requests sharing `state`. Duplicate keys (the same
/// subject asked twice in one plan) are sent once.
pub fn pack(state: Value, items: Vec<Item>) -> Result<Vec<Batch>, PackError> {
    let state_bytes = serde_json::to_vec(&state)
        .expect("state serialization")
        .len();
    let state_tokens = estimate_tokens(state_bytes);
    if state_tokens > state_budget() {
        return Err(PackError::StateTooLarge {
            tokens: state_tokens,
        });
    }
    let state = Arc::new(state);
    // Envelope: model id, braces, and key quoting. Generous on purpose.
    let overhead = 64;
    let budget = request_budget();
    let mut batches = Vec::new();
    let mut current: BTreeMap<String, Question> = BTreeMap::new();
    let mut used = state_tokens + overhead;
    for item in items {
        if current.contains_key(&item.key) {
            continue;
        }
        let q_bytes = serde_json::to_vec(&item.question)
            .expect("question serialization")
            .len()
            + item.key.len()
            + 8;
        let q_tokens = estimate_tokens(q_bytes);
        if !current.is_empty() && used + q_tokens > budget {
            batches.push(Batch {
                state: Arc::clone(&state),
                questions: std::mem::take(&mut current),
                est_tokens: used,
            });
            used = state_tokens + overhead;
        }
        used += q_tokens;
        current.insert(item.key, item.question);
    }
    if !current.is_empty() {
        batches.push(Batch {
            state,
            questions: current,
            est_tokens: used,
        });
    }
    Ok(batches)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn item(key: &str, text: &str) -> Item {
        Item {
            key: key.to_owned(),
            question: Question::Noul {
                instructions: json!(text),
                criteria: None,
            },
            meta: serde_json::Value::Null,
        }
    }

    #[test]
    fn small_groups_fit_one_request_and_duplicates_collapse() {
        let batches = pack(
            json!("s"),
            vec![item("a", "x"), item("b", "y"), item("a", "x")],
        )
        .unwrap();
        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0].questions.len(), 2);
    }

    #[test]
    fn many_questions_spill_into_more_requests_under_budget() {
        let long = "q".repeat(3000);
        let items: Vec<Item> = (0..200).map(|i| item(&format!("k{i:03}"), &long)).collect();
        let batches = pack(json!("state"), items).unwrap();
        assert!(batches.len() > 1);
        assert!(batches.iter().all(|b| b.est_tokens <= request_budget()));
        let total: usize = batches.iter().map(|b| b.questions.len()).sum();
        assert_eq!(total, 200);
    }

    #[test]
    fn oversized_state_is_refused() {
        let big = "x".repeat(200_000);
        assert!(matches!(
            pack(json!(big), vec![item("a", "x")]),
            Err(PackError::StateTooLarge { .. })
        ));
    }
}
