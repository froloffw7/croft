//! Context-window accounting for local models.
//!
//! A local server has a fixed window (set when the model was loaded) and
//! typically crashes or truncates silently when it is exceeded. croft cannot
//! read the window from every server, so the user declares it
//! (`croft pair --context 131072`) and croft keeps the conversation inside
//! it: tokens are ESTIMATED from bytes (a deliberately pessimistic ~3 bytes
//! per token, which holds for code and over-counts prose), the oldest
//! exchanges are dropped first, and a turn that cannot fit even alone is
//! refused with a message that says what to shrink.

use serde_json::Value;

/// Room kept for the model's answer (matches the transport's `max_tokens`).
pub(crate) const RESERVE_TOKENS: usize = 8192;

/// Estimated tokens for `text`.
pub(crate) fn estimate_text(text: &str) -> usize {
    text.len().div_ceil(3)
}

/// Estimated tokens of a whole request: the system prompt and every message.
pub(crate) fn estimate(system: &str, messages: &[Value]) -> usize {
    let body: usize = messages
        .iter()
        .map(|m| estimate_text(m.get("content").and_then(Value::as_str).unwrap_or("")) + 4)
        .sum();
    estimate_text(system) + body
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Fit {
    /// Fits as is; `used` is the estimate.
    Ok { used: usize },
    /// Old messages were dropped to fit.
    Trimmed { dropped: usize, used: usize },
    /// Even the newest turn alone does not fit.
    TooBig { used: usize },
}

/// Drop the oldest messages (always in whole exchanges, so the conversation
/// still opens on a user message) until the request fits `limit` minus the
/// answer reserve. The LAST message (the new user turn) is never dropped.
pub(crate) fn fit(system: &str, messages: &mut Vec<Value>, limit: usize) -> Fit {
    let budget = limit.saturating_sub(RESERVE_TOKENS.min(limit / 4));
    let mut dropped = 0;
    while estimate(system, messages) > budget && messages.len() > 1 {
        let n = if messages.len() >= 3 { 2 } else { 1 };
        messages.drain(..n);
        dropped += n;
        while messages.len() > 1
            && messages
                .first()
                .is_some_and(|m| m.get("role").and_then(Value::as_str) != Some("user"))
        {
            messages.remove(0);
            dropped += 1;
        }
    }
    let used = estimate(system, messages);
    if used > budget {
        Fit::TooBig { used }
    } else if dropped > 0 {
        Fit::Trimmed { dropped, used }
    } else {
        Fit::Ok { used }
    }
}

pub(crate) fn too_big_message(used: usize, limit: usize) -> String {
    format!(
        "this turn alone is ~{used} tokens but the context window is {limit}: select a smaller \
         fragment (Ask Navigator with a selection sends only those lines) or raise \
         `croft pair --context`"
    )
}
