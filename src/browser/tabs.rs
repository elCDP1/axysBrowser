use gtk::Stack;
use webkit6::WebView;

pub struct Tab {
    pub id: usize,
    pub title: String,
    pub uri: String,
    pub content: Stack,
    pub web_view: WebView,
    pub history: Vec<String>,
    pub history_index: usize,
}

impl Tab {
    pub fn new(id: usize, content: Stack, web_view: WebView) -> Self {
        Self {
            id,
            title: "New Tab".to_string(),
            uri: "axys://newtab".to_string(),
            content,
            web_view,
            history: vec!["axys://newtab".to_string()],
            history_index: 0,
        }
    }

    pub fn can_go_back(&self) -> bool {
        self.history_index > 0
    }

    pub fn can_go_forward(&self) -> bool {
        self.history_index + 1 < self.history.len()
    }

    pub fn push_history(&mut self, uri: String) {
        if self.history.last() == Some(&uri) {
            return;
        }

        if self.history_index + 1 < self.history.len() {
            self.history.truncate(self.history_index + 1);
        }

        self.history.push(uri);
        self.history_index = self.history.len() - 1;
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
