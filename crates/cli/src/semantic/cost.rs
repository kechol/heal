//! Token estimates and prices for `heal semantic ask --dry-run`.
//!
//! The estimate is deliberately conservative (it over-counts): jev-lint
//! measured its own estimator at ~14% above the billed count, and an
//! under-estimate would let a request cross the 32Ki state ceiling, which
//! the server answers by dropping verdicts rather than by splitting. Every
//! provider plans under Jev's ceilings, so a task selects the same subjects
//! whichever model answers.

use crate::core::config::SemanticProvider;

/// What one model costs, and how a plan for it is priced.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pricing {
    pub usd_per_input_token: f64,
    pub usd_per_output_token: f64,
    /// Planning estimate. Zero when output is free.
    pub est_output_tokens_per_question: usize,
    /// Prompt text the provider's wire format adds to every request.
    pub request_overhead_tokens: usize,
    pub max_questions_per_request: usize,
}

/// `$42` per billion input tokens; output tokens are free. Source:
/// <https://docs.typesafe.ai/models.md>.
pub const JEV_PRICING: Pricing = Pricing {
    usd_per_input_token: 42.0 / 1_000_000_000.0,
    usd_per_output_token: 0.0,
    est_output_tokens_per_question: 0,
    request_overhead_tokens: 0,
    max_questions_per_request: usize::MAX,
};

/// Claude Haiku 5.5 at its rate for prompts of 100K tokens or fewer, which
/// [`request_budget`] guarantees. Every answer is generated text, so the
/// question cap keeps a response short enough for the request timeout.
pub const CLAUDE_HAIKU_PRICING: Pricing = Pricing {
    usd_per_input_token: 0.10 / 1_000_000.0,
    usd_per_output_token: 0.50 / 1_000_000.0,
    est_output_tokens_per_question: 100,
    request_overhead_tokens: 400,
    max_questions_per_request: 25,
};

#[must_use]
pub fn pricing(model: &str) -> Pricing {
    match SemanticProvider::of_model(model) {
        SemanticProvider::Jev => JEV_PRICING,
        SemanticProvider::Claude => CLAUDE_HAIKU_PRICING,
    }
}

impl Pricing {
    #[must_use]
    pub fn usd(&self, input_tokens: u64, output_tokens: u64) -> f64 {
        #[allow(clippy::cast_precision_loss)]
        let (input, output) = (input_tokens as f64, output_tokens as f64);
        input * self.usd_per_input_token + output * self.usd_per_output_token
    }

    /// Estimated price of one request carrying `questions` questions.
    #[must_use]
    pub fn estimate_usd(&self, est_input_tokens: usize, questions: usize) -> f64 {
        self.usd(
            (est_input_tokens + self.request_overhead_tokens) as u64,
            (questions * self.est_output_tokens_per_question) as u64,
        )
    }
}

/// Ceiling on the whole request, in input tokens.
pub const REQUEST_TOKEN_LIMIT: usize = 65_536;
/// Independent ceiling on `state` alone. Fills first in practice.
pub const STATE_TOKEN_LIMIT: usize = 32_768;
/// Planning margin applied to both ceilings so estimation error does not
/// push a batch over.
pub const PLANNING_MARGIN: f64 = 0.85;

/// Approximate token count of a serialized JSON payload. Three bytes per
/// token over-counts English prose and source code, which is the safe
/// direction for budgeting.
#[must_use]
pub fn estimate_tokens(bytes: usize) -> usize {
    bytes.div_ceil(3)
}

/// Planning budget for a whole request.
#[must_use]
pub fn request_budget() -> usize {
    budget(REQUEST_TOKEN_LIMIT)
}

/// Planning budget for the state alone.
#[must_use]
pub fn state_budget() -> usize {
    budget(STATE_TOKEN_LIMIT)
}

fn budget(limit: usize) -> usize {
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    let b = (limit as f64 * PLANNING_MARGIN) as usize;
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_billion_tokens_cost_forty_two_dollars() {
        assert!((JEV_PRICING.usd(1_000_000_000, 1_000_000) - 42.0).abs() < 1e-9);
    }

    #[test]
    fn claude_haiku_charges_for_output_and_estimates_it() {
        assert_eq!(pricing("claude-haiku-5-5"), CLAUDE_HAIKU_PRICING);
        assert_eq!(pricing("jev-1.13.0"), JEV_PRICING);
        assert!((CLAUDE_HAIKU_PRICING.usd(1_000_000, 1_000_000) - 0.60).abs() < 1e-9);
        let expected = CLAUDE_HAIKU_PRICING.usd(1400, 200);
        assert!((CLAUDE_HAIKU_PRICING.estimate_usd(1000, 2) - expected).abs() < 1e-12);
    }

    #[test]
    fn estimate_rounds_up() {
        assert_eq!(estimate_tokens(0), 0);
        assert_eq!(estimate_tokens(1), 1);
        assert_eq!(estimate_tokens(3), 1);
        assert_eq!(estimate_tokens(4), 2);
    }

    #[test]
    fn budgets_sit_under_the_ceilings() {
        assert!(state_budget() < STATE_TOKEN_LIMIT);
        assert!(request_budget() < REQUEST_TOKEN_LIMIT);
    }
}
