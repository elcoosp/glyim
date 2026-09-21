/// TokenBudget.
pub struct TokenBudget {
    /// Struct.
    pub max_tokens: usize,
    #[allow(missing_docs)]
    pub used_tokens: usize,
}
impl TokenBudget {
    /// new.
    pub fn new(max_tokens: usize) -> Self {
        Self {
            max_tokens,
            used_tokens: 0,
        }
    }
    /// remaining.
    pub fn remaining(&self) -> usize {
        self.max_tokens.saturating_sub(self.used_tokens)
    }
    /// try_allocate.
    pub fn try_allocate(&mut self, tokens: usize) -> bool {
        if self.used_tokens + tokens <= self.max_tokens {
            self.used_tokens += tokens;
            true
        } else {
            false
        }
    }
    /// force_allocate.
    pub fn force_allocate(&mut self, tokens: usize) {
        self.used_tokens += tokens;
    }
    /// estimate_tokens.
    pub fn estimate_tokens(text: &str) -> usize {
        (text.len() * 11 + 27) / 40
    }
}
