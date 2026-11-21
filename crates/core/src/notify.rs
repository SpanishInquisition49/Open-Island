//! Notification queue.

#[derive(Clone, Debug, Default)]
pub struct Notification {
    pub id: u32,
    pub app: String,
    pub summary: String,
    pub body: String,
    pub critical: bool,
    pub group_count: u32,
}

/// Adds `n` to the queue, or folds it into the waiting notification of the same app (the newer one
/// replaces it and the count goes up). Returns true when it was folded.
pub fn enqueue(queue: &mut Vec<Notification>, mut n: Notification, max_len: usize) -> bool {
    for q in queue.iter_mut() {
        if !q.app.is_empty() && q.app == n.app {
            n.group_count = q.group_count.max(1) + 1;
            *q = n;
            return true;
        }
    }
    queue.push(n);
    if queue.len() > max_len {
        queue.remove(0);
    }
    false
}

/// Two notifications are of the same conversation when they come from the same app and say the same thing in their title
/// (a chat's or a sender's name, a site): five messages from one person are one conversation, not five cards.
pub fn same_conversation(a_app: &str, a_summary: &str, b_app: &str, b_summary: &str) -> bool {
    !a_app.is_empty() && a_app == b_app && a_summary.trim() == b_summary.trim()
}

/// The history, newest first, as groups of consecutive entries of one conversation that came within `window_ms` of each other:
/// (position of the newest of the group, how many). `entries` are (app, summary, when in ms).
pub fn group_history(entries: &[(&str, &str, i64)], window_ms: i64) -> Vec<(usize, usize)> {
    let mut groups: Vec<(usize, usize)> = vec![];
    for (i, (app, summary, at)) in entries.iter().enumerate() {
        if let Some((start, count)) = groups.last_mut() {
            let (pa, ps, pat) = entries[*start + *count - 1];
            // newest first: the one before in the list is the later one
            if same_conversation(pa, ps, app, summary) && (pat - at).abs() <= window_ms {
                *count += 1;
                continue;
            }
        }
        groups.push((i, 1));
    }
    groups
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(app: &str, s: &str) -> Notification {
        Notification { app: app.into(), summary: s.into(), ..Default::default() }
    }

    #[test]
    fn folds_same_app() {
        let mut q = vec![];
        assert!(!enqueue(&mut q, n("chat", "a"), 8));
        assert!(enqueue(&mut q, n("chat", "b"), 8));
        assert!(enqueue(&mut q, n("chat", "c"), 8));
        assert_eq!(q.len(), 1);
        assert_eq!(q[0].summary, "c");
        assert_eq!(q[0].group_count, 3);
    }

    #[test]
    fn caps_the_queue() {
        let mut q = vec![];
        for i in 0..5 {
            enqueue(&mut q, n(&format!("app{i}"), "x"), 3);
        }
        assert_eq!(q.len(), 3);
        assert_eq!(q[0].app, "app2");
        enqueue(&mut q, n("", "no app"), 3);
        enqueue(&mut q, n("", "no app 2"), 3);
        assert!(q.iter().filter(|x| x.app.is_empty()).count() == 2, "unnamed apps never fold");
    }

    #[test]
    fn a_conversation_is_an_app_and_a_title() {
        assert!(same_conversation("Telegram", "Anna", "Telegram", " Anna "));
        assert!(!same_conversation("Telegram", "Anna", "Telegram", "Marco"));
        assert!(!same_conversation("Telegram", "Anna", "Signal", "Anna"));
        assert!(!same_conversation("", "x", "", "x"), "no app, no conversation");
    }

    #[test]
    fn the_history_is_grouped_by_conversation_within_a_window() {
        const MIN: i64 = 60_000;
        // newest first: Anna x3 (the last two a minute apart), Marco, Anna again 40 minutes before, then Anna x2 an hour before that
        let e = [("T", "Anna", 100 * MIN), ("T", "Anna", 99 * MIN), ("T", "Anna", 98 * MIN), ("T", "Marco", 97 * MIN), ("T", "Anna", 60 * MIN), ("T", "Anna", 59 * MIN), ("T", "Anna", 0)];
        assert_eq!(group_history(&e, 30 * MIN), vec![(0, 3), (3, 1), (4, 2), (6, 1)], "a different sender breaks a group, so does a long silence");
        assert_eq!(group_history(&[], MIN), vec![]);
        assert_eq!(group_history(&e[..1], MIN), vec![(0, 1)]);
    }
}

