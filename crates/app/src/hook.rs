//! `open-island hook <zsh|bash|fish>` prints the lines to put in the shell's startup file; they call
//! `open-island hook start/end` around every command, and a command that outlasts a threshold shows up as a
//! progress pill (green when it ends well, red with its exit code when not).

use crate::services::ipc;

/// Programs that are interactive or never end: a pill for them would be noise.
const IGNORED: &[&str] = &[
    "vim", "vi", "nvim", "nano", "emacs", "micro", "helix", "hx", "less", "more", "man", "most", "bat", "ssh", "mosh", "top", "htop", "btop", "btm", "tmux", "screen", "zellij", "fzf", "watch", "tail", "journalctl",
    "ranger", "yazi", "lf", "nnn", "mc", "claude", "irb", "ipython", "psql", "mysql", "sqlite3", "gdb", "lldb", "nmtui", "cmus", "ncmpcpp", "mpv", "vlc", "ytop", "glances", "lazygit", "tig", "k9s",
];

/// The words that only run another command.
const WRAPPERS: &[&str] = &["sudo", "doas", "env", "time", "nice", "command", "builtin", "exec", "nohup", "noglob", "stdbuf", "ionice"];

/// What the pill calls the command, or None when it should not get one.
pub fn label(cmdline: &str, extra_ignored: &str) -> Option<String> {
    let words: Vec<&str> = cmdline.split_whitespace().collect();
    let mut at = 0;
    while at < words.len() && (WRAPPERS.contains(&words[at]) || words[at].contains('=') && !words[at].starts_with('-') || words[at].starts_with('-') && at > 0 && WRAPPERS.contains(&words[at - 1])) {
        at += 1;
    }
    let first = words.get(at)?;
    let name = first.rsplit('/').next().unwrap_or(first);
    if IGNORED.contains(&name) || extra_ignored.split([',', ' ', ':']).any(|x| !x.is_empty() && x == name) {
        return None;
    }
    // a bare `python script.py` is a job worth showing; the interactive `python` alone is not
    if matches!(name, "python" | "python3" | "node") && words.len() == at + 1 {
        return None;
    }
    let mut text = words[at..].join(" ");
    if text.chars().count() > 40 {
        text = text.chars().take(39).collect::<String>() + "…";
    }
    Some(text)
}

pub fn script(shell: &str) -> Option<&'static str> {
    Some(match shell {
        "zsh" => {
            r#"# open-island: a pill for long commands
autoload -Uz add-zsh-hook
__open_island_pre() { __open_island_on=1; command open-island hook start "$$" "$1" 2>/dev/null; }
__open_island_post() { local c=$?; [[ -n $__open_island_on ]] && { unset __open_island_on; command open-island hook end "$$" "$c" 2>/dev/null; }; }
add-zsh-hook preexec __open_island_pre
add-zsh-hook precmd __open_island_post
"#
        }
        "bash" => {
            r#"# open-island: a pill for long commands (needs bash-preexec for the start; this form works on its own)
__open_island_pre() { [[ -n $__open_island_on || -n $COMP_LINE || $BASH_COMMAND == __open_island* || $BASH_COMMAND == *PROMPT_COMMAND* ]] && return; __open_island_on=1; command open-island hook start "$$" "$BASH_COMMAND" 2>/dev/null; }
__open_island_post() { local c=$?; [[ -n $__open_island_on ]] && { unset __open_island_on; command open-island hook end "$$" "$c" 2>/dev/null; }; return $c; }
trap '__open_island_pre' DEBUG
PROMPT_COMMAND="__open_island_post${PROMPT_COMMAND:+; $PROMPT_COMMAND}"
"#
        }
        "fish" => {
            r#"# open-island: a pill for long commands
function __open_island_pre --on-event fish_preexec
    command open-island hook start $fish_pid "$argv" 2>/dev/null
end
function __open_island_post --on-event fish_postexec
    command open-island hook end $fish_pid $status 2>/dev/null
end
"#
        }
        _ => return None,
    })
}

/// `hook <zsh|bash|fish>` | `hook start <id> <command line>` | `hook end <id> <code>`; returns the exit code.
pub fn run(args: &[String]) -> i32 {
    match args.first().map(String::as_str) {
        Some("start") if args.len() >= 3 => {
            let secs = std::env::var("OPEN_ISLAND_HOOK_SECONDS").ok().and_then(|v| v.parse::<u32>().ok()).unwrap_or(10);
            let ignored = std::env::var("OPEN_ISLAND_HOOK_IGNORE").unwrap_or_default();
            if let Some(l) = label(&args[2..].join(" "), &ignored) {
                ipc::send(&format!("cmd-start {} {secs} {l}", args[1]));
            }
            0
        }
        Some("end") if args.len() >= 3 => {
            ipc::send(&format!("cmd-end {} {}", args[1], args[2]));
            0
        }
        Some(shell) if script(shell).is_some() => {
            print!("{}", script(shell).unwrap());
            0
        }
        _ => {
            eprintln!("usage: open-island hook <zsh|bash|fish>   (then: eval \"$(open-island hook zsh)\" in the shell's startup file)");
            2
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_the_command_and_skips_the_interactive_ones() {
        assert_eq!(label("cargo build --release", "").as_deref(), Some("cargo build --release"));
        assert_eq!(label("sudo pacman -Syu", "").as_deref(), Some("pacman -Syu"), "a wrapper is looked through");
        assert_eq!(label("FOO=1 BAR=2 make -j8", "").as_deref(), Some("make -j8"), "so are variable assignments");
        assert_eq!(label("/usr/bin/rsync -a a b", "").as_deref(), Some("/usr/bin/rsync -a a b"));
        assert!(label("/usr/bin/vim x", "").is_none(), "the path does not hide an editor");
        assert!(label("vim notes.md", "").is_none() && label("sudo htop", "").is_none() && label("ssh host", "").is_none());
        assert!(label("python", "").is_none() && label("python3", "").is_none(), "a bare interpreter is a REPL");
        assert_eq!(label("python train.py", "").as_deref(), Some("python train.py"));
        assert!(label("", "").is_none() && label("   ", "").is_none() && label("sudo", "").is_none());
        assert!(label("make", "make, go").is_none() && label("go test", "make go").is_none(), "the user's own list");
    }

    #[test]
    fn a_long_command_line_is_cut() {
        let l = label(&format!("echo {}", "x".repeat(100)), "").unwrap();
        assert_eq!(l.chars().count(), 40);
        assert!(l.ends_with('…'));
    }

    #[test]
    fn every_shell_has_a_script_that_calls_start_and_end() {
        for sh in ["zsh", "bash", "fish"] {
            let s = script(sh).unwrap();
            assert!(s.contains("hook start") && s.contains("hook end"), "{sh}");
        }
        assert!(script("powershell").is_none() && script("").is_none());
    }
}
