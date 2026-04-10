use std::sync::atomic::{AtomicU32, Ordering};

/// Tracks the iteration budget for an agent loop.
///
/// The budget limits the number of LLM round-trips the agent may perform
/// within a single `chat()` invocation.
pub struct IterationBudget {
    max_total: u32,
    used: AtomicU32,
}

impl IterationBudget {
    /// Create a new budget with the given maximum iteration count.
    pub fn new(max_total: u32) -> Self {
        Self {
            max_total,
            used: AtomicU32::new(0),
        }
    }

    /// Consume one iteration. Returns `false` if the budget is exhausted.
    pub fn consume(&self) -> bool {
        let prev = self.used.fetch_add(1, Ordering::SeqCst);
        if prev >= self.max_total {
            // Roll back — budget was already exhausted.
            self.used.fetch_sub(1, Ordering::SeqCst);
            false
        } else {
            true
        }
    }

    /// Refund one iteration (e.g. for execute_code calls that shouldn't count).
    pub fn refund(&self) {
        let prev = self.used.fetch_sub(1, Ordering::SeqCst);
        if prev == 0 {
            // Prevent underflow — restore to 0.
            self.used.store(0, Ordering::SeqCst);
        }
    }

    /// Return the number of remaining iterations.
    pub fn remaining(&self) -> u32 {
        let used = self.used.load(Ordering::SeqCst);
        self.max_total.saturating_sub(used)
    }

    /// Return the number of iterations consumed so far.
    pub fn used(&self) -> u32 {
        self.used.load(Ordering::SeqCst)
    }

    /// Return a warning string when the budget is running low.
    ///
    /// - At ≥90% usage: a strong warning
    /// - At ≥70% usage: a caution
    /// - Otherwise: `None`
    pub fn budget_warning(&self) -> Option<String> {
        let used = self.used.load(Ordering::SeqCst);
        let remaining = self.remaining();
        let pct = (used as f64 / self.max_total as f64) * 100.0;

        if pct >= 90.0 {
            Some(format!(
                "[WARNING] Iteration budget critically low: {remaining}/{} remaining. Wrap up immediately.",
                self.max_total
            ))
        } else if pct >= 70.0 {
            Some(format!(
                "[CAUTION] Iteration budget running low: {remaining}/{} remaining. Start wrapping up.",
                self.max_total
            ))
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_budget_consume_and_exhaust() {
        let budget = IterationBudget::new(3);
        assert!(budget.consume());
        assert!(budget.consume());
        assert!(budget.consume());
        assert!(!budget.consume());
        assert_eq!(budget.remaining(), 0);
        assert_eq!(budget.used(), 3);
    }

    #[test]
    fn test_budget_refund() {
        let budget = IterationBudget::new(3);
        assert!(budget.consume());
        assert_eq!(budget.remaining(), 2);
        budget.refund();
        assert_eq!(budget.remaining(), 3);
        assert_eq!(budget.used(), 0);
    }

    #[test]
    fn test_budget_warning_levels() {
        // max=10: 70% at 7, 90% at 9
        let budget = IterationBudget::new(10);

        // Under 70% — no warning
        for _ in 0..6 {
            budget.consume();
        }
        assert!(budget.budget_warning().is_none());

        // At 70% — caution
        budget.consume(); // 7 used
        let warning = budget.budget_warning();
        assert!(warning.is_some());
        assert!(warning.unwrap().contains("CAUTION"));

        // At 90% — warning
        budget.consume(); // 8
        budget.consume(); // 9
        let warning = budget.budget_warning();
        assert!(warning.is_some());
        assert!(warning.unwrap().contains("WARNING"));
    }

    #[test]
    fn test_budget_refund_at_zero() {
        let budget = IterationBudget::new(5);
        // Refund when nothing consumed should not underflow
        budget.refund();
        assert_eq!(budget.used(), 0);
        assert_eq!(budget.remaining(), 5);
    }
}
