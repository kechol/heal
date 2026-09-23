//! Token estimates and prices for `heal semantic ask --dry-run`.
//!
//! The estimate is deliberately conservative (it over-counts): jev-lint
//! measured its own estimator at ~14% above the billed count, and an
//! under-estimate would let a request cross the 32Ki state ceiling, which
//! the server answers by dropping verdicts rather than by splitting.

/// Input price, USD per token (`$42` per billion input tokens; output
/// tokens are free). Source: <https://docs.typesafe.ai/models.md>.
pub const USD_PER_INPUT_TOKEN: f64 = 42.0 / 1_000_000_000.0;

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

#[must_use]
pub fn usd_for(input_tokens: u64) -> f64 {
    #[allow(clippy::cast_precision_loss)]
    let t = input_tokens as f64;
    t * USD_PER_INPUT_TOKEN
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
        assert!((usd_for(1_000_000_000) - 42.0).abs() < 1e-9);
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
