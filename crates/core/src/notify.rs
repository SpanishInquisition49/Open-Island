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
}
