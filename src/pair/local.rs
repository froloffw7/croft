//! Local-model transport for the pilot: one minimal Anthropic-compatible
//! `/v1/messages` streaming call per turn (Ollama, LM Studio, llama.cpp,
//! vLLM). Deliberately NOT the claude CLI: driving the full agent at a local
//! endpoint ships ~213 KB of tool schemas per turn and 500s on every Ollama
//! tested (docs/MULTIPLAYER.md, "local models"). Everything downstream of a
//! text delta — the fence machine, the apply path, notes, cancel — is the
//! shared pair machinery; only the transport differs. The endpoint is
//! stateless, so the caller owns the conversation as a message list.
//!
//! Servers that only speak the OpenAI wire (FastFlowLM, which runs models on
//! AMD NPUs, and others) are reached through `/v1/chat/completions`: an
//! endpoint whose `/v1/messages` answers a route-level 404 is switched over
//! on the spot and remembered for the session, and `CROFT_PAIR_API=openai`
//! (or `anthropic`) pins the choice.

use std::sync::{Mutex, OnceLock};
use std::sync::mpsc::Sender;
use std::time::Duration;

use serde_json::{Value, json};

use super::{FenceMachine, PairState, TurnEnd, apply_fence_event};

/// Ceiling for one turn's reply; fenced edits are short, and local servers
/// reject nothing under their context limit.
const MAX_TOKENS: u64 = 8192;

/// The reply cap sent as `max_tokens`. `CROFT_PAIR_MAX_TOKENS` raises it for
/// reasoning models, whose hidden thinking eats the same budget as the answer
/// ("reply truncated"); clamped so a typo cannot ask for nothing or the moon.
fn max_tokens() -> u64 {
    std::env::var("CROFT_PAIR_MAX_TOKENS")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .map_or(MAX_TOKENS, |n| n.clamp(256, 200_000))
}

/// Which wire a local endpoint speaks.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Api {
    /// `POST /v1/messages` (Anthropic), `message_stop`-terminated SSE.
    Anthropic,
    /// `POST /v1/chat/completions` (OpenAI), `[DONE]`-terminated SSE.
    OpenAi,
}

/// Endpoints whose `/v1/messages` was a route-level 404 this session; their
/// turns go straight to `/v1/chat/completions`.
static OPENAI_ENDPOINTS: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// `CROFT_PAIR_API`: `openai` or `anthropic` pins the wire; anything else
/// (or unset) leaves it to the 404 probe.
fn pinned_api() -> Option<Api> {
    match std::env::var("CROFT_PAIR_API")
        .ok()?
        .to_lowercase()
        .as_str()
    {
        "openai" => Some(Api::OpenAi),
        "anthropic" => Some(Api::Anthropic),
        _ => None,
    }
}

/// Whether a 404 means "this server has no such route" rather than "the
/// route exists and the model does not" (Ollama's `model 'x' not found`,
/// whose body names the fix and must reach the user).
fn is_missing_route(status: u16, body: &str) -> bool {
    status == 404 && !body.to_lowercase().contains("model")
}

/// The auth headers for `base_url`: `(x-api-key value, Authorization
/// value)`. Keyed Anthropic-compatible gateways read the environment token
/// as `Authorization: Bearer` (Anthropic's own `ANTHROPIC_AUTH_TOKEN`
/// convention); it is also mirrored into `x-api-key` for gateways of the
/// other style. The credential only ever travels over https or to a
/// loopback host - a cleartext remote hop gets the harmless placeholder
/// instead, so the token cannot be sniffed off the wire. Environment-only
/// on purpose: a token must never land in `pair.json`.
pub(crate) fn auth_for(base_url: &str, token: Option<&str>) -> (String, Option<String>) {
    match token {
        Some(t) if credential_allowed(base_url) => (t.to_string(), Some(format!("Bearer {t}"))),
        _ => (String::from("croft"), None),
    }
}

/// Whether `base_url` is an OpenCode gateway, which refuses a request without
/// a stable `x-opencode-session` ("MissingSessionID") so it can route a
/// conversation to one backend and reuse its prompt cache. A real URL parse:
/// the host must be `opencode.ai` or a subdomain of it.
pub(crate) fn wants_session_header(base_url: &str) -> bool {
    url::Url::parse(base_url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_ascii_lowercase))
        .is_some_and(|h| h == "opencode.ai" || h.ends_with(".opencode.ai"))
}

/// One opaque id for this croft run: every turn of the conversation carries
/// the same value, a new run starts a new one. Nothing about the user in it.
fn session_id() -> &'static str {
    static ID: OnceLock<String> = OnceLock::new();
    ID.get_or_init(|| {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        format!("croft-{}-{nanos:x}", std::process::id())
    })
}

/// Whether the credential may travel to `base_url`: https anywhere, http
/// only to a loopback host. A real URL parse, not a string scan - the
/// authority's userinfo made `http://localhost:pass@evil.example` read as
/// loopback under a colon-split check while the actual destination was the
/// remote host. Inline completions send their API key under the same rule
/// (#1927).
pub(crate) fn credential_allowed(base_url: &str) -> bool {
    let Ok(u) = url::Url::parse(base_url) else {
        return false;
    };
    match u.scheme() {
        "https" => true,
        "http" => match u.host() {
            Some(url::Host::Domain(d)) => d == "localhost",
            Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
            Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
            None => false,
        },
        _ => false,
    }
}

/// POST one streaming `/v1/messages` turn and drive its text deltas through
/// the fence machine into the collab seat. `messages` must already end with
/// the user message; on success the assistant's full text is appended so the
/// next turn carries the conversation. Always ends the turn: state flags
/// reset and a [`TurnEnd`] sent, error or not. A cancel that landed
/// mid-stream keeps draining the HTTP body (no mid-request abort) but
/// applies nothing — the shared `cancelled` flag gates the apply path.
pub(crate) fn stream_turn(
    base_url: &str,
    model: &str,
    system: &str,
    messages: &mut Vec<Value>,
    state: &Mutex<PairState>,
    turn_tx: &Sender<TurnEnd>,
) {
    stream_turn_until(
        base_url,
        model,
        system,
        messages,
        state,
        turn_tx,
        std::time::Instant::now() + TURN_DEADLINE,
    );
}

/// [`stream_turn`] with an explicit wall-clock deadline, so tests can prove
/// the ceiling interrupts a trickling stream without waiting ten minutes.
#[allow(clippy::too_many_arguments)]
pub(crate) fn stream_turn_until(
    base_url: &str,
    model: &str,
    system: &str,
    messages: &mut Vec<Value>,
    state: &Mutex<PairState>,
    turn_tx: &Sender<TurnEnd>,
    deadline: std::time::Instant,
) {
    // ureq 3 here, not the tree-wide ureq 2: version 3's phased timeouts
    // check the remaining budget before EVERY socket operation, so the
    // per-request global timeout below enforces the turn deadline at the
    // socket layer. A read can neither outlive the deadline nor keep the
    // socket alive after it: the cut drops the response, which closes the
    // connection. Version 2's only knob was a fixed per-read timeout, which
    // parked a blocked read (plus a helper thread and the socket) for up to
    // that timeout past the deadline on a peer that just went silent.
    let agent = ureq3::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(5)))
        // Never follow redirects: the x-api-key header would be replayed to
        // the redirect target, so a loopback endpoint answering 302 could
        // bounce the credential to an arbitrary host (and a 301 would
        // downgrade the POST to GET anyway). With 0 the 3xx comes back as a
        // plain response and is refused below.
        .max_redirects(0)
        // Non-2xx arrives as a response, not an error: the error BODY
        // carries the fix ("model 'x' not found, try pulling it first").
        .http_status_as_error(false)
        .build()
        .new_agent();
    let token = std::env::var("ANTHROPIC_AUTH_TOKEN").ok();
    let (api_key, bearer) = auth_for(base_url, token.as_deref());
    let send = |api: Api| {
        let (path, req) = match api {
            Api::Anthropic => (
                "/v1/messages",
                json!({
                    "model": model,
                    "max_tokens": max_tokens(),
                    "stream": true,
                    "system": system,
                    "messages": messages,
                }),
            ),
            Api::OpenAi => {
                let mut all = vec![json!({ "role": "system", "content": system })];
                all.extend(messages.iter().cloned());
                (
                    "/v1/chat/completions",
                    json!({
                        "model": model,
                        "max_tokens": max_tokens(),
                        "stream": true,
                        "messages": all,
                    }),
                )
            }
        };
        let mut post = agent
            .post(format!("{base_url}{path}"))
            .config()
            // The whole exchange (headers, cold-model wait, every body read)
            // shares the turn's remaining wall-clock budget.
            .timeout_global(Some(
                deadline.saturating_duration_since(std::time::Instant::now()),
            ))
            .build()
            .header("content-type", "application/json");
        if api == Api::Anthropic {
            post = post
                .header("anthropic-version", "2023-06-01")
                .header("x-api-key", &api_key);
        }
        if let Some(bearer) = &bearer {
            post = post.header("authorization", bearer);
        }
        if wants_session_header(base_url) {
            post = post.header("x-opencode-session", session_id());
        }
        post.send(req.to_string())
    };
    let pinned = pinned_api();
    let mut api = pinned.unwrap_or_else(|| {
        if OPENAI_ENDPOINTS
            .lock()
            .unwrap()
            .iter()
            .any(|u| u == base_url)
        {
            Api::OpenAi
        } else {
            Api::Anthropic
        }
    });
    let mut result = send(api);
    // An endpoint with no `/v1/messages` route: try the OpenAI wire once and
    // remember the answer, unless the wire was pinned.
    if pinned.is_none() && api == Api::Anthropic {
        result = match result {
            Ok(mut r) if r.status().as_u16() == 404 => {
                let body = r.body_mut().read_to_string().unwrap_or_default();
                if is_missing_route(404, &body) {
                    OPENAI_ENDPOINTS.lock().unwrap().push(base_url.to_string());
                    api = Api::OpenAi;
                    send(api)
                } else {
                    messages.pop();
                    end_turn(
                        state,
                        turn_tx,
                        true,
                        format!("local endpoint {base_url}: {}", status_detail(404, &body)),
                    );
                    return;
                }
            }
            other => other,
        };
    }
    let resp = match result {
        Ok(r) => r,
        Err(e) => {
            // The endpoint never answered the turn: pop the user message so
            // the conversation stays balanced. Leaving it made the NEXT turn
            // send [user, user], a hard 400 on strict gateways for the rest
            // of the seat's life, or a silent re-submit of the unanswered
            // ask on permissive local servers.
            messages.pop();
            end_turn(
                state,
                turn_tx,
                true,
                format!("local endpoint {base_url}: {}", error_detail(&e)),
            );
            return;
        }
    };

    let status = resp.status().as_u16();
    // With redirects disabled a 3xx arrives as a plain response; name it
    // instead of reading an empty stream (following it would have replayed
    // the credential header to wherever Location pointed).
    if (300..400).contains(&status) {
        messages.pop();
        end_turn(
            state,
            turn_tx,
            true,
            format!("local endpoint {base_url}: redirect refused (status {status})"),
        );
        return;
    }
    if status >= 400 {
        let body = resp.into_body().read_to_string().unwrap_or_default();
        messages.pop();
        end_turn(
            state,
            turn_tx,
            true,
            format!(
                "local endpoint {base_url}: {}",
                status_detail(status, &body)
            ),
        );
        return;
    }

    let mut fence = FenceMachine::new();
    let mut assistant = String::new();
    // Outcome honesty: the turn only succeeded if the stream ENDED cleanly
    // (message_stop / [DONE]) with untruncated text. A mid-stream drop, the
    // read timeout, an SSE error frame, or a token-limit cut all used to be
    // reported as a finished turn while the partial edit had been reverted.
    let mut clean_end = false;
    let mut truncated = false;
    let mut stream_error: Option<String> = None;
    // Total wall-clock ceiling: the transport's global timeout above bounds
    // every socket read by the remaining budget, so a silent or trickling
    // peer is cut mid-read at the deadline (and dropping the reader then
    // closes the socket). The deadline is enforced per CHUNK, not per line;
    // `lines()` blocks until a newline arrives, so a stream of non-newline
    // bytes would have dodged a per-line check indefinitely.
    let mut reader = resp.into_body().into_reader();
    let mut pending: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 4096];
    'stream: loop {
        // Checked even while data flows: a peer flooding bytes faster than
        // they drain must not starve the deadline.
        if std::time::Instant::now() > deadline {
            stream_error = Some(ceiling_msg());
            break;
        }
        // A single SSE line has no business being megabytes long; a cap
        // keeps a hostile or broken endpoint from ballooning memory.
        if pending.len() > 1 << 20 {
            stream_error = Some(String::from("endpoint sent an over-long SSE line"));
            break;
        }
        let n = match std::io::Read::read(&mut reader, &mut chunk) {
            Ok(0) => break, // hung up; finish() reverts open fences
            Ok(n) => n,
            Err(e) if is_timeout(&e) => {
                stream_error = Some(ceiling_msg());
                break;
            }
            Err(_) => break, // hung up; finish() reverts open fences
        };
        pending.extend_from_slice(&chunk[..n]);
        while let Some(pos) = pending.iter().position(|&b| b == b'\n') {
            let raw: Vec<u8> = pending.drain(..=pos).collect();
            let line = String::from_utf8_lossy(&raw[..raw.len() - 1]);
            let line = line.trim_end_matches('\r');
            let Some(data) = line.strip_prefix("data: ") else {
                continue;
            };
            if data.trim() == "[DONE]" {
                clean_end = true;
                break 'stream;
            }
            let Ok(v) = serde_json::from_str::<Value>(data) else {
                // Skipping silently would drop a chunk of the reply and
                // still report success; the endpoint is broken, say so.
                stream_error = Some(String::from("endpoint sent malformed SSE data"));
                break 'stream;
            };
            if api == Api::OpenAi {
                if let Some(err) = v["error"].as_object() {
                    let msg = err.get("message").and_then(Value::as_str);
                    stream_error = Some(msg.unwrap_or("stream error").to_string());
                    break 'stream;
                }
                let choice = &v["choices"][0];
                // Reasoning models stream `reasoning_content` apart from the
                // answer; only the answer reaches the fence machine.
                if let Some(text) = choice["delta"]["content"].as_str()
                    && !text.is_empty()
                {
                    assistant.push_str(text);
                    for event in fence.push(text) {
                        apply_fence_event(state, event);
                    }
                }
                match choice["finish_reason"].as_str() {
                    Some("length") => {
                        truncated = true;
                        clean_end = true;
                        break 'stream;
                    }
                    // Some servers close without `[DONE]`: a finish reason
                    // is the end of the reply.
                    Some(_) => {
                        clean_end = true;
                        break 'stream;
                    }
                    None => {}
                }
                continue;
            }
            match v["type"].as_str().unwrap_or("") {
                "content_block_delta" => {
                    if v["delta"]["type"] == "text_delta"
                        && let Some(text) = v["delta"]["text"].as_str()
                    {
                        assistant.push_str(text);
                        for event in fence.push(text) {
                            apply_fence_event(state, event);
                        }
                    }
                }
                "message_delta" if v["delta"]["stop_reason"] == "max_tokens" => {
                    truncated = true;
                }
                "message_stop" => {
                    clean_end = true;
                    break 'stream;
                }
                "error" => {
                    let msg = v["error"]["message"].as_str().unwrap_or("stream error");
                    stream_error = Some(msg.to_string());
                    break 'stream;
                }
                _ => {}
            }
        }
    }
    for event in fence.finish() {
        apply_fence_event(state, event);
    }
    let failure = if let Some(err) = stream_error {
        Some(err)
    } else if truncated {
        Some(format!(
            "reply truncated at the {}-token limit (raise it with CROFT_PAIR_MAX_TOKENS)",
            max_tokens()
        ))
    } else if !clean_end {
        Some(String::from(match api {
            Api::Anthropic => "stream ended before message_stop",
            Api::OpenAi => "stream ended before [DONE]",
        }))
    } else if assistant.is_empty() {
        // An empty assistant message is itself a 400 on the next turn.
        Some(String::from("endpoint streamed no text"))
    } else {
        None
    };
    match failure {
        Some(reason) => {
            messages.pop(); // balanced: the next ask starts a clean exchange
            end_turn(
                state,
                turn_tx,
                true,
                format!("local endpoint {base_url}: {reason}"),
            );
        }
        None => {
            messages.push(json!({ "role": "assistant", "content": assistant }));
            end_turn(state, turn_tx, false, assistant);
        }
    }
}

/// Total wall-clock ceiling for one turn, enforced by the transport's
/// global timeout.
const TURN_DEADLINE: Duration = Duration::from_secs(600);

/// The ceiling failure text, derived from [`TURN_DEADLINE`] so the message
/// can never drift from the enforced value. (A test-supplied shorter
/// deadline reuses it; those tests assert the outcome, not the wording.)
fn ceiling_msg() -> String {
    format!(
        "turn exceeded the {} minute ceiling",
        TURN_DEADLINE.as_secs() / 60
    )
}

/// The failure text for a transport error. HTTP statuses never land here
/// (`http_status_as_error(false)` returns them as responses, handled by
/// [`status_detail`]). Only the GLOBAL timeout is the turn ceiling; the 5
/// second connect timeout also arrives as `Error::Timeout` and must keep
/// its own name ("timeout: connect") instead of claiming ten minutes
/// passed after five seconds.
fn error_detail(e: &ureq3::Error) -> String {
    match e {
        ureq3::Error::Timeout(ureq3::Timeout::Global | ureq3::Timeout::PerCall) => ceiling_msg(),
        other => other.to_string(),
    }
}

/// The failure text for an HTTP error status, with the error body included:
/// the body carries the fix ("model 'x' not found, try pulling it first").
fn status_detail(status: u16, body: &str) -> String {
    let body = body.trim();
    if body.is_empty() {
        format!("status {status}")
    } else {
        let short: String = body.chars().take(200).collect();
        format!("status {status}: {short}")
    }
}

/// Whether a body-read error is the transport enforcing the turn deadline
/// (the global timeout travels inside `io::Error::other`). Only the global
/// and per-call reasons count: no other timeout is configured for the body
/// phase, and a raw socket-timeout kind can only mean the same budget.
fn is_timeout(e: &std::io::Error) -> bool {
    matches!(
        e.kind(),
        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
    ) || e
        .get_ref()
        .and_then(|inner| inner.downcast_ref::<ureq3::Error>())
        .is_some_and(|u| {
            matches!(
                u,
                ureq3::Error::Timeout(ureq3::Timeout::Global | ureq3::Timeout::PerCall)
            )
        })
}

/// End a turn that was never sent (it cannot fit the context window).
pub(crate) fn refuse_turn(state: &Mutex<PairState>, turn_tx: &Sender<TurnEnd>, text: String) {
    end_turn(state, turn_tx, true, text);
}

/// The local twin of the claude reader's `result` handling: reset the
/// per-turn flags and report the turn's end.
fn end_turn(state: &Mutex<PairState>, turn_tx: &Sender<TurnEnd>, is_error: bool, text: String) {
    let mut st = state.lock().unwrap();
    let cancelled = st.cancelled;
    st.turn_active = false;
    st.cancelled = false;
    st.discarding = false;
    st.comment_only = false;
    drop(st);
    let _ = turn_tx.send(TurnEnd {
        is_error,
        cancelled,
        text,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Only the GLOBAL timeout is the turn ceiling. The 5 second connect
    /// timeout also surfaces as `Error::Timeout`; reporting it as "exceeded
    /// the 10 minute ceiling" after 5 seconds misnames the failure.
    #[test]
    fn a_connect_timeout_is_not_reported_as_the_turn_ceiling() {
        let connect = error_detail(&ureq3::Error::Timeout(ureq3::Timeout::Connect));
        assert!(
            !connect.contains("ceiling"),
            "a connect timeout is not the turn ceiling: {connect}"
        );
        let global = error_detail(&ureq3::Error::Timeout(ureq3::Timeout::Global));
        assert!(
            global.contains("ceiling"),
            "the global timeout IS the ceiling: {global}"
        );
        assert!(
            global.contains("10 minute"),
            "the ceiling text names the enforced duration: {global}"
        );
    }
}
