use gtk::Stack;
use std::rc::Rc;
use webkit6::WebView;

/// Opens a tab; `None` = blank new tab, `Some(uri)` = starting page.
/// Returns the new tab id.
pub type NewTabCallback = Rc<dyn Fn(Option<&str>) -> usize>;

pub struct Tab {
    pub id: usize,
    pub title: String,
    pub uri: String,
    pub content: Stack,
    pub web_view: WebView,
    pub history: Vec<String>,
    pub history_index: usize,
    /// Incremented on every web navigation started from the UI; lets the
    /// load watchdog tell whether it still refers to the current load.
    pub nav_serial: u64,
    /// True once the current load has committed (the page is being shown).
    pub committed: bool,
}

impl Tab {
    pub fn new(id: usize, content: Stack, web_view: WebView) -> Self {
        Self {
            id,
            title: rust_i18n::t!("app.new_tab").to_string(),
            uri: "axys://newtab".to_string(),
            content,
            web_view,
            history: vec!["axys://newtab".to_string()],
            history_index: 0,
            nav_serial: 0,
            committed: false,
        }
    }

    pub fn can_go_back(&self) -> bool {
        self.history_index > 0
    }

    pub fn can_go_forward(&self) -> bool {
        self.history_index + 1 < self.history.len()
    }

    pub fn push_history(&mut self, uri: String) {
        push_history_in(&mut self.history, &mut self.history_index, uri);
    }

    pub fn go_back(&mut self) -> Option<String> {
        if !self.can_go_back() {
            return None;
        }

        self.history_index -= 1;

        self.history.get(self.history_index).cloned()
    }

    pub fn go_forward(&mut self) -> Option<String> {
        if !self.can_go_forward() {
            return None;
        }

        self.history_index += 1;

        self.history.get(self.history_index).cloned()
    }
}

/// Bounds per-tab memory on long sessions / SPA navigation storms.
const MAX_TAB_HISTORY: usize = 100;

/// Pushes a visited URI onto a back/forward stack, trimming the forward tail
/// and bounding the total size. Split out from [`Tab::push_history`] so the
/// cursor semantics can be unit-tested without constructing GTK widgets.
fn push_history_in(history: &mut Vec<String>, history_index: &mut usize, uri: String) {
    // Compare against the entry under the cursor, not just the last one:
    // after going back, re-entering the *current* entry must not add a
    // duplicate, but the forward tail must still be discarded otherwise.
    if history.get(*history_index) == Some(&uri) {
        return;
    }

    if *history_index + 1 < history.len() {
        history.truncate(*history_index + 1);
    }

    history.push(uri);

    *history_index = history.len() - 1;

    if history.len() > MAX_TAB_HISTORY {
        let excess = history.len() - MAX_TAB_HISTORY;

        history.drain(0..excess);

        *history_index = history_index.saturating_sub(excess);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(history: &[&str], index: usize) -> (Vec<String>, usize) {
        (
            history.iter().map(|entry| entry.to_string()).collect(),
            index,
        )
    }

    #[test]
    fn push_appends_and_moves_cursor() {
        let (mut history, mut index) = state(&["a"], 0);

        push_history_in(&mut history, &mut index, "b".to_string());

        assert_eq!(history, vec!["a", "b"]);
        assert_eq!(index, 1);
    }

    #[test]
    fn push_ignores_reentry_of_current_entry() {
        // Reproduces the old bug: [a, b] with the cursor on `a` re-entering
        // `a` produced [a, a] and a broken back target.
        let (mut history, mut index) = state(&["a", "b"], 0);

        push_history_in(&mut history, &mut index, "a".to_string());

        assert_eq!(history, vec!["a", "b"]);
        assert_eq!(index, 0);
    }

    #[test]
    fn push_trims_forward_tail() {
        let (mut history, mut index) = state(&["a", "b", "c"], 0);

        push_history_in(&mut history, &mut index, "d".to_string());

        assert_eq!(history, vec!["a", "d"]);
        assert_eq!(index, 1);
    }

    #[test]
    fn push_bounds_history_and_keeps_cursor_valid() {
        let mut history: Vec<String> = (0..MAX_TAB_HISTORY).map(|i| i.to_string()).collect();

        let mut index = history.len() - 1;

        push_history_in(&mut history, &mut index, "new".to_string());

        assert_eq!(history.len(), MAX_TAB_HISTORY);
        assert_eq!(index, MAX_TAB_HISTORY - 1);
        assert_eq!(history.last().map(String::as_str), Some("new"));
    }
}
