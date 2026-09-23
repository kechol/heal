//! The semantic tasks HEAL ships. Each file is one task; the registry in
//! [`crate::semantic::task::registry`] lists them in a stable order.

// Task code mostly builds prompt text. `format!` into a `String` reads
// better there than `write!` chains, and the allocations are dwarfed by
// the network round trip the text is built for.
#![allow(clippy::format_push_string, clippy::format_collect)]

pub mod commit_intent;
pub mod common;
pub mod concept;
pub mod naming;
pub mod refactor;
pub mod verify;

#[cfg(test)]
#[allow(dead_code)] // shared by every task's tests; not all use every helper
pub(crate) mod testing {
    //! Offline helpers for task tests: plan, fake the answers, lower.
    use crate::semantic::api::Answer;
    use crate::semantic::task::{Answered, Item, Lowered, Task, TaskContext};

    /// Plan `task`, answer each item with `answer_for`, and lower.
    pub(crate) fn lower_with(
        task: &dyn Task,
        ctx: &TaskContext<'_>,
        answer_for: impl Fn(&Item) -> Option<Answer>,
    ) -> (Vec<Item>, Lowered) {
        let items: Vec<Item> = task
            .plan(ctx)
            .expect("plan")
            .into_iter()
            .flat_map(|g| g.items)
            .collect();
        let answers: Vec<Option<Answer>> = items.iter().map(&answer_for).collect();
        let answered: Vec<Answered<'_>> = items
            .iter()
            .zip(&answers)
            .map(|(item, a)| Answered {
                item,
                answer: a.as_ref(),
            })
            .collect();
        let lowered = task.lower(ctx, &answered);
        (items, lowered)
    }

    pub(crate) fn choice(label: &str, p: f64) -> Answer {
        Answer::Choice {
            choice: label.to_owned(),
            probabilities: [(label.to_owned(), p)].into_iter().collect(),
            confidence: p,
        }
    }

    pub(crate) fn noul(p: f64) -> Answer {
        Answer::Noul { noul: p }
    }

    pub(crate) fn score(level: f64, confidence: f64) -> Answer {
        Answer::Score {
            score: level,
            probabilities: std::collections::BTreeMap::new(),
            confidence,
        }
    }

    /// The text of a question's `instructions`, for matching in fakes.
    pub(crate) fn instructions(item: &Item) -> String {
        match &item.question {
            crate::semantic::api::Question::Noul { instructions, .. }
            | crate::semantic::api::Question::Choice { instructions, .. }
            | crate::semantic::api::Question::Score { instructions, .. } => instructions
                .as_str()
                .map_or_else(|| instructions.to_string(), str::to_owned),
        }
    }
}
