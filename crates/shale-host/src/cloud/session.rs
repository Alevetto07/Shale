//! The two values a sign-in produces, and what a sign-in can answer.
//!
//! Kept apart from [`super::Cloud`] so that a token is a type rather than two
//! strings passed around: the `Debug` here is the one place that decides a
//! token is never printed.

/// What a successful sign-in leaves behind.
#[derive(Clone)]
pub struct Tokens {
    pub access: String,
    pub refresh: String,
    /// Unix seconds. Bambu's `expiresIn` is a duration, so the deadline is
    /// worked out once, at the moment the answer arrives.
    pub expires_at: i64,
}

impl Tokens {
    pub fn new(access: &str, refresh: &str, expires_at: i64) -> Self {
        Self {
            access: access.to_string(),
            refresh: refresh.to_string(),
            expires_at,
        }
    }

    /// Whether the access token still has more than `slack` seconds on it.
    pub fn valid_for(&self, slack: i64) -> bool {
        self.expires_at - slack > super::now_unix()
    }
}

impl std::fmt::Debug for Tokens {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tokens")
            .field("access", &"set")
            .field("refresh", &"set")
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

/// The two ordinary outcomes of a sign-in. Bambu asking for a code is not a
/// failure, so it is not an error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignIn {
    /// Tokens arrived; the account is signed in.
    SignedIn,
    /// Bambu sent (or will send) a code to the address, and this key ties the
    /// answer to the attempt.
    NeedsCode { tfa_key: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_token_is_valid_while_it_has_time_left() {
        let tokens = Tokens::new("a", "r", super::super::now_unix() + 3600);
        assert!(tokens.valid_for(300));
    }

    #[test]
    fn a_token_within_the_slack_is_not_valid() {
        let tokens = Tokens::new("a", "r", super::super::now_unix() + 60);
        assert!(!tokens.valid_for(300));
    }

    #[test]
    fn debug_says_set_and_nothing_else() {
        let text = format!("{:?}", Tokens::new("aaa", "rrr", 5));
        assert!(text.contains("access: \"set\""), "{text}");
        assert!(!text.contains("aaa") && !text.contains("rrr"), "{text}");
    }
}
