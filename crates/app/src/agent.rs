//! `open-island agent`: what a coding agent tells the island, so that its turns show up like any long task.
//!
//! * `open-island agent` with a Claude Code hook's JSON on stdin (one command for every hook event): a prompt
//!   starts a turn, each tool call updates the status line, `Stop` ends the turn, a `Notification` is a card, and
//!   `SessionEnd` forgets the session. Every session is its own activity, so several agents run side by side.
//!   The card of a session that needs you stays until the agent goes on (as drip's does); with `--done` a card also
//!   says when a turn ends. Each event is announced once: where there is a card, the pill does not open for it too.
//! * `open-island agent start|tool|done|fail|end --id ID [--name Codex] [text...]`: the same for any other agent
//!   or script, by hand.
//! * `open-island agent setup claude`: the `hooks` block to put in `~/.claude/settings.json`.
//!
//! A hook must never get in the agent's way: it always exits 0, says nothing, and does not wait for the island.

use crate::services::ipc;
use serde_json::{json, Value};

/// The folder's name: what tells the sessions apart.
fn repo(cwd: &str) -> String {
    std::path::Path::new(cwd.trim_end_matches(['/', '\\'])).file_name().map_or(String::new(), |n| n.to_string_lossy().into_owned())
}

fn short(text: &str, max: usize) -> String {
    let one: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if one.chars().count() > max {
        one.chars().take(max - 1).collect::<String>() + "…"
    } else {
        one
    }
}

/// What a tool call is about: the file, the command, the pattern, the address.
fn target(input: &Value) -> String {
    for key in ["file_path", "path", "notebook_path"] {
        if let Some(p) = input.get(key).and_then(Value::as_str) {
            return std::path::Path::new(p).file_name().map_or(short(p, 30), |n| n.to_string_lossy().into_owned());
        }
    }
    for key in ["command", "pattern", "url", "query", "description", "prompt"] {
        if let Some(t) = input.get(key).and_then(Value::as_str) {
            return short(t, 36);
        }
    }
    String::new()
}

fn activity(state: &str, id: &str, label: &str, detail: &str) -> String {
    let mut v = json!({"id": id, "label": label, "icon": "bolt"});
    if !detail.is_empty() {
        v["detail"] = json!(detail);
    }
    format!("activity {state} {v}")
}

/// The colour of the agent's cards (Claude's orange).
const AGENT_COLOR: &str = "#d97757";

/// The messages for the island that one Claude Code hook event turns into. `terminal` is the program the agent runs
/// in, which a click on its card brings forward; `done` asks for a card at the end of each turn.
pub fn claude_lines(event: &Value, name: &str, terminal: &str, done: bool) -> Vec<String> {
    let s = |k: &str| event.get(k).and_then(Value::as_str).unwrap_or("");
    let id = s("session_id");
    if id.is_empty() {
        return vec![];
    }
    let repo = repo(s("cwd"));
    let label = if repo.is_empty() { name.to_string() } else { format!("{name} · {repo}") };
    // one card per session: a newer one takes its place, and going on takes it away
    let card_id = format!("agent:{id}");
    let dismiss = format!("dismiss {card_id}");
    // takes the session off the pill without a word: its card says what happened
    let clear = format!("activity clear {}", json!({"id": id}));
    let who = if repo.is_empty() { name } else { repo.as_str() };
    let card = |title: String, detail: &str, kind: &str, stay: bool| {
        let mut v = json!({"id": card_id, "title": title, "kind": kind, "stay": stay, "app": terminal});
        if !detail.is_empty() {
            v["detail"] = json!(detail);
        }
        if kind == "info" {
            v["icon"] = json!("bolt");
            v["color"] = json!(AGENT_COLOR);
        }
        format!("card {v}")
    };
    match s("hook_event_name") {
        "SessionStart" => vec![activity("run", id, &label, "thinking")],
        "UserPromptSubmit" => vec![activity("run", id, &label, "thinking"), dismiss],
        "PreToolUse" => {
            let t = s("tool_name");
            let what = target(event.get("tool_input").unwrap_or(&Value::Null));
            vec![activity("run", id, &label, &if what.is_empty() { t.to_string() } else { format!("{t} {what}") }), dismiss]
        }
        "PostToolUse" | "PostToolUseFailure" | "ElicitationResult" => vec![dismiss],
        "Stop" if done => vec![clear, card(format!("{who} is done"), &short(s("last_assistant_message"), 160), "ok", false)],
        "Stop" => vec![activity("done", id, &label, "turn finished"), dismiss],
        "StopFailure" => vec![clear, card(format!("{who} stopped"), &short(s("error"), 160), "fail", false)],
        // the agent waits for someone: a card, and nothing on the pill (a status for a session that has none would
        // open it, a second notice of the same thing). It stays when a decision is asked
        // (a permission, a question); an agent that has merely been idle gets one that goes by itself.
        "Notification" => {
            let msg = short(s("message"), 160);
            let kind = s("notification_type");
            let decides = matches!(kind, "permission_prompt" | "elicitation_dialog") || (kind.is_empty() && msg.contains("permission"));
            vec![card(format!("{who} needs you"), &msg, "info", decides)]
        }
        "SessionEnd" => vec![clear, dismiss],
        _ => vec![],
    }
}

/// `agent <event> --id ID [--name N] [text...]` by hand: one line, or an error to print.
pub fn manual_line(args: &[String]) -> Result<String, String> {
    let event = args.first().map(String::as_str).unwrap_or("");
    let (mut id, mut name, mut text) = (String::new(), "Agent".to_string(), vec![]);
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--id" => id = args.get(i + 1).ok_or("--id needs a value")?.clone(),
            "--name" => name = args.get(i + 1).ok_or("--name needs a value")?.clone(),
            f if f.starts_with("--") => return Err(format!("unknown option {f}")),
            t => {
                text.push(t.to_string());
                i += 1;
                continue;
            }
        }
        i += 2;
    }
    if id.is_empty() {
        id = name.to_lowercase();
    }
    let text = text.join(" ");
    Ok(match event {
        "start" => activity("run", &id, &name, if text.is_empty() { "working" } else { &text }),
        "tool" | "update" => activity("run", &id, &name, &text),
        "done" => activity("done", &id, &name, if text.is_empty() { "finished" } else { &text }),
        "fail" => activity("fail", &id, &name, if text.is_empty() { "failed" } else { &text }),
        "end" => format!("activity clear {}", json!({"id": id})),
        _ => return Err("usage: open-island agent start|tool|done|fail|end --id ID [--name Codex] [text...]   (or Claude Code's hook JSON on stdin)".into()),
    })
}

/// The `hooks` block for Claude Code.
pub fn claude_settings() -> Value {
    let hook = json!([{"hooks": [{"type": "command", "command": "open-island agent"}]}]);
    let tools = json!([{"matcher": "*", "hooks": [{"type": "command", "command": "open-island agent"}]}]);
    json!({"hooks": {"UserPromptSubmit": hook, "PreToolUse": tools, "PostToolUse": tools, "PostToolUseFailure": tools, "Stop": hook, "StopFailure": hook, "Notification": hook, "SessionEnd": hook}})
}

pub fn run(args: &[String]) -> i32 {
    if args.first().map(String::as_str) == Some("setup") {
        if args.get(1).map(String::as_str) == Some("claude") {
            println!("{}", serde_json::to_string_pretty(&claude_settings()).unwrap_or_default());
            eprintln!("Merge this into the \"hooks\" of ~/.claude/settings.json (keep your other hooks).");
            return 0;
        }
        eprintln!("usage: open-island agent setup claude");
        return 2;
    }
    let done = args.len() == 1 && args[0] == "--done";
    if args.is_empty() || done {
        // a hook: JSON on stdin; never fail
        let mut text = String::new();
        let _ = std::io::Read::read_to_string(&mut std::io::stdin(), &mut text);
        if let Ok(event) = serde_json::from_str::<Value>(&text) {
            let terminal = std::env::var("TERM_PROGRAM").unwrap_or_default();
            for line in claude_lines(&event, "Claude", &terminal, done) {
                ipc::send(&line);
            }
        }
        return 0;
    }
    match manual_line(args) {
        Ok(line) => {
            if !ipc::send(&line) {
                eprintln!("open-island is not running");
                return 1;
            }
            0
        }
        Err(e) => {
            eprintln!("open-island agent: {e}");
            2
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(v: Value) -> Vec<String> {
        claude_lines(&v, "Claude", "ghostty", false)
    }

    #[test]
    fn a_turn_is_an_activity_of_its_own_session() {
        let l = ev(json!({"hook_event_name": "UserPromptSubmit", "session_id": "s1", "cwd": "/home/me/code/site", "prompt": "fix it"}));
        assert_eq!(l[1], "dismiss agent:s1", "answering takes the card away");
        assert!(l[0].starts_with("activity run {") && l[0].contains(r#""id":"s1""#) && l[0].contains("Claude · site") && l[0].contains("thinking"));
        let l = ev(json!({"hook_event_name": "PreToolUse", "session_id": "s1", "cwd": "/home/me/code/site", "tool_name": "Edit", "tool_input": {"file_path": "/home/me/code/site/src/lib.rs"}}));
        assert!(l[0].contains("Edit lib.rs"), "{}", l[0]);
        let l = ev(json!({"hook_event_name": "PreToolUse", "session_id": "s1", "tool_name": "Bash", "tool_input": {"command": "cargo   test --workspace --all-targets -- --nocapture"}}));
        assert!(l[0].contains("Bash cargo test --workspace --all-target…"), "spaces squeezed, long commands cut: {}", l[0]);
        assert!(l[0].contains(r#""label":"Claude""#), "no folder, no suffix: {}", l[0]);
        let l = ev(json!({"hook_event_name": "Stop", "session_id": "s1", "cwd": "/x/site"}));
        assert!(l[0].starts_with("activity done") && l[0].contains("turn finished"));
        let l = ev(json!({"hook_event_name": "SessionEnd", "session_id": "s1"}));
        assert_eq!(l, vec![r#"activity clear {"id":"s1"}"#.to_string(), "dismiss agent:s1".to_string()]);
    }

    #[test]
    fn a_waiting_agent_gets_a_card_and_the_rest_is_ignored() {
        let l = ev(json!({"hook_event_name": "Notification", "session_id": "s1", "cwd": "/x/api", "message": "Claude needs your permission to use Bash"}));
        assert!(l.len() == 1 && l[0].starts_with("card {"), "one notice, not the pill's too: {l:?}");
        let card = |line: &str| serde_json::from_str::<Value>(line.strip_prefix("card ").unwrap()).unwrap();
        let c = card(&l[0]);
        assert_eq!((c["title"].as_str(), c["id"].as_str(), c["stay"].as_bool(), c["app"].as_str()), (Some("api needs you"), Some("agent:s1"), Some(true), Some("ghostty")));
        let idle = ev(json!({"hook_event_name": "Notification", "session_id": "s1", "notification_type": "idle_prompt", "message": "Claude is waiting for your input"}));
        assert_eq!(card(&idle[0])["stay"], false, "nothing to decide: it goes by itself");
        assert_eq!(ev(json!({"hook_event_name": "PostToolUse", "session_id": "s1"})), ["dismiss agent:s1"]);
        assert!(ev(json!({"hook_event_name": "SubagentStop", "session_id": "s1"})).is_empty());
        assert!(ev(json!({"hook_event_name": "Stop"})).is_empty(), "no session, nothing to say");
        assert_eq!(ev(json!({"hook_event_name": "Stop", "session_id": "s1"}))[1], "dismiss agent:s1", "no card at the end unless asked");
        let failed = ev(json!({"hook_event_name": "StopFailure", "session_id": "s1", "error": "rate_limit"}));
        assert!(failed[0].starts_with("activity clear") && failed[1].starts_with("card {"), "{failed:?}");
        let done = claude_lines(&json!({"hook_event_name": "Stop", "session_id": "s1", "cwd": "/x/api", "last_assistant_message": "All tests pass."}), "Claude", "", true);
        assert!(done.len() == 2 && done[0].starts_with("activity clear"), "{done:?}");
        let c = card(&done[1]);
        assert_eq!((c["title"].as_str(), c["detail"].as_str(), c["kind"].as_str()), (Some("api is done"), Some("All tests pass."), Some("ok")));
    }

    #[test]
    fn the_messages_are_understood_by_the_island() {
        for line in ev(json!({"hook_event_name": "PreToolUse", "session_id": "s", "cwd": "/a/b", "tool_name": "Read", "tool_input": {"file_path": "/a/b/c.rs"}})).into_iter().chain(ev(json!({"hook_event_name": "SessionEnd", "session_id": "s"}))) {
            // (a name no card has is nothing to take away: no input)
            assert!(line.starts_with("dismiss ") || crate::services::ipc::parse(&line).is_some(), "{line}");
        }
        for line in ev(json!({"hook_event_name": "Notification", "session_id": "s", "message": "a permission"})) {
            assert!(crate::services::ipc::parse(&line).is_some(), "{line}");
        }
        assert!(matches!(crate::services::ipc::parse(r#"activity clear {"id":"s"}"#), Some(island_ui::model::Input::ActivityClear(id)) if id == "s"));
    }

    #[test]
    fn by_hand_for_any_other_agent() {
        let a = |s: &[&str]| manual_line(&s.iter().map(|x| x.to_string()).collect::<Vec<_>>());
        let l = a(&["start", "--id", "cx1", "--name", "Codex", "reading", "the", "repo"]).unwrap();
        assert!(l.starts_with("activity run") && l.contains(r#""id":"cx1""#) && l.contains(r#""detail":"reading the repo""#) && l.contains(r#""label":"Codex""#), "{l}");
        assert!(a(&["done"]).unwrap().contains(r#""id":"agent""#), "the id defaults to the name");
        assert_eq!(a(&["end", "--id", "cx1"]).unwrap(), r#"activity clear {"id":"cx1"}"#);
        assert!(a(&["wobble"]).is_err() && a(&["start", "--bogus"]).is_err() && a(&[]).is_err());
    }

    #[test]
    fn the_settings_block_names_every_event() {
        let s = claude_settings();
        for e in ["UserPromptSubmit", "PreToolUse", "PostToolUse", "PostToolUseFailure", "Stop", "StopFailure", "Notification", "SessionEnd"] {
            assert_eq!(s["hooks"][e][0]["hooks"][0]["command"], "open-island agent", "{e}");
        }
    }
}
