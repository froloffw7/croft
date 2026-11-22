//! Navigator conversation memory across `croft pair` runs.
//!
//! * local provider: the last few messages are saved to the state directory
//!   after every successful turn and reloaded when the seat starts.
//! * claude provider: a stable session id per (workspace, seat) is handed to
//!   the CLI as `--session-id` the first time and `--resume` afterwards.
//!
//! `CROFT_PAIR_HISTORY=off` disables it, `=fresh` discards what was saved and
//! starts over. A history problem never blocks a turn: every error is
//! swallowed and the conversation simply starts empty.

use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// Messages kept for a local model: 3 exchanges. Every ask embeds a buffer,
/// so older turns are both large and stale; a few are enough for "and the
/// second one?" follow-ups without eating a small model's context.
pub(crate) const KEEP_MESSAGES: usize = 6;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Mode {
    On,
    Off,
    Fresh,
}

pub(crate) fn mode() -> Mode {
    match std::env::var("CROFT_PAIR_HISTORY").as_deref() {
        Ok("off") => Mode::Off,
        Ok("fresh") => Mode::Fresh,
        _ => Mode::On,
    }
}

fn key(workspace: &Path, name: &str) -> String {
    let mut h = Sha256::new();
    h.update(workspace.to_string_lossy().as_bytes());
    h.update([0]);
    h.update(name.as_bytes());
    h.finalize()
        .iter()
        .take(8)
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn dir_in(state: &Path) -> PathBuf {
    state.join("pair")
}

pub(crate) fn local_path(workspace: &Path, name: &str) -> PathBuf {
    dir_in(&crate::paths::state_dir()).join(format!("{}.messages.json", key(workspace, name)))
}

fn session_path(workspace: &Path, name: &str) -> PathBuf {
    dir_in(&crate::paths::state_dir()).join(format!("{}.session", key(workspace, name)))
}

/// Keep the last `KEEP_MESSAGES`, starting on a user message (an API refuses
/// a conversation that opens with the assistant).
pub(crate) fn trim(mut msgs: Vec<Value>) -> Vec<Value> {
    if msgs.len() > KEEP_MESSAGES {
        msgs.drain(..msgs.len() - KEEP_MESSAGES);
    }
    while msgs
        .first()
        .is_some_and(|m| m.get("role").and_then(Value::as_str) != Some("user"))
    {
        msgs.remove(0);
    }
    msgs
}

pub(crate) fn load_local(path: &Path) -> Vec<Value> {
    match mode() {
        Mode::Off => return Vec::new(),
        Mode::Fresh => {
            let _ = std::fs::remove_file(path);
            return Vec::new();
        }
        Mode::On => {}
    }
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str::<Vec<Value>>(&s).ok())
        .map(trim)
        .unwrap_or_default()
}

pub(crate) fn save_local(path: &Path, msgs: &[Value]) {
    if mode() == Mode::Off {
        return;
    }
    let kept = trim(msgs.to_vec());
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(text) = serde_json::to_string(&kept) {
        // Write-then-rename so a crash never leaves half a file.
        let tmp = path.with_extension("tmp");
        if std::fs::write(&tmp, text).is_ok() {
            let _ = std::fs::rename(&tmp, path);
        }
    }
}

/// A fresh random-looking UUID (v4 layout) without a uuid dependency.
fn new_uuid() -> String {
    let mut h = Sha256::new();
    h.update(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
            .to_le_bytes(),
    );
    h.update(std::process::id().to_le_bytes());
    let mut b: [u8; 16] = h.finalize()[..16].try_into().unwrap();
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let hex: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

/// True when the claude CLI already has a transcript for `id` under any
/// project directory of its config dir.
fn claude_knows(config: &Path, id: &str) -> bool {
    let Ok(rd) = std::fs::read_dir(config.join("projects")) else {
        return false;
    };
    rd.flatten()
        .any(|e| e.path().join(format!("{id}.jsonl")).is_file())
}

fn claude_config_dir() -> PathBuf {
    std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_default()
                .join(".claude")
        })
}

/// The claude CLI flags for this seat: `--session-id <id>` for a brand-new
/// conversation, `--resume <id>` when the CLI still has it. Empty when
/// history is off.
pub(crate) fn claude_args(workspace: &Path, name: &str) -> Vec<String> {
    if cfg!(test) {
        // Unit tests build commands for throwaway workspaces; keep them out
        // of the real state directory.
        return Vec::new();
    }
    claude_args_in(&session_path(workspace, name), &claude_config_dir(), mode())
}

pub(crate) fn claude_args_in(path: &Path, config: &Path, mode: Mode) -> Vec<String> {
    if mode == Mode::Off {
        return Vec::new();
    }
    let saved = std::fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    if mode == Mode::On
        && let Some(id) = &saved
        && claude_knows(config, id)
    {
        return vec!["--resume".into(), id.clone()];
    }
    let id = new_uuid();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(path, &id);
    vec!["--session-id".into(), id]
}
