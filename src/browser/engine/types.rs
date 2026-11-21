//! Engine-neutral types of the seam.
//!
//! Nothing here imports an engine. These are the shapes the interface and both
//! backends agree on; the WebKit and Servo modules translate to and from them.

/// How a page is configured when it is created.
///
/// Both fields are user settings rather than engine details, which is why they
/// travel as one value instead of as two `bool` arguments: the Servo backend
/// maps them to its own preferences, and a reader at the call site can see
/// what is being decided.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ViewConfig {
    /// Developer extras: the inspector entry in the context menu.
    pub developer_tools: bool,
    /// Render with the GPU. Off keeps the page in software, which is what
    /// machines without a working GPU driver need.
    pub hardware_acceleration: bool,
}

/// Where a load is. The four stages WebKit reports and the seam keeps, because
/// the window's history, title fallback and toolbar all key off them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoadState {
    /// A navigation started (including a redirect chain's first hop).
    Started,
    /// A redirect was followed. Only the backend that reports the stage has
    /// it: WebKit does, Servo 0.7 reports started and finished only.
    #[cfg(target_os = "linux")]
    Redirected,
    /// The first bytes of the document arrived: the URL is real now.
    Committed,
    /// The document and its subresources are done.
    Finished,
}

/// One credential the user typed for a site.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Credentials {
    pub username: String,
    pub password: String,
}

/// Where the prompt's single answer waits until the dialog decides.
type AnswerSlot = std::rc::Rc<std::cell::RefCell<Option<Box<dyn FnOnce(Option<Credentials>)>>>>;

/// An HTTP/proxy authentication request waiting for the interface.
///
/// The engine cancels a request the embedder does not claim, so the prompt
/// holds the engine's own request and answers it exactly once — either with
/// credentials or by cancelling. Cloning the prompt shares that one answer,
/// which is what the dialog needs (its buttons and its close path).
#[derive(Clone)]
pub struct AuthPrompt {
    host: String,
    realm: String,
    retry: bool,
    proposed_username: Option<String>,
    answer: AnswerSlot,
}

impl AuthPrompt {
    pub(crate) fn new(
        host: String,
        realm: String,
        retry: bool,
        proposed_username: Option<String>,
        answer: Box<dyn FnOnce(Option<Credentials>)>,
    ) -> Self {
        Self {
            host,
            realm,
            retry,
            proposed_username,
            answer: std::rc::Rc::new(std::cell::RefCell::new(Some(answer))),
        }
    }

    pub fn host(&self) -> &str {
        &self.host
    }

    /// Empty when the engine did not name one.
    pub fn realm(&self) -> &str {
        &self.realm
    }

    /// True when the same request already failed once (wrong password).
    pub fn is_retry(&self) -> bool {
        self.retry
    }

    /// A username the engine already knows for this host, if any.
    pub fn proposed_username(&self) -> Option<&str> {
        self.proposed_username.as_deref()
    }

    /// Sends credentials, or `None` to cancel. The first call wins: a dialog
    /// that authenticated and then closed must not cancel afterwards.
    pub fn answer(&self, credentials: Option<Credentials>) {
        if let Some(answer) = self.answer.borrow_mut().take() {
            answer(credentials);
        }
    }
}

/// An entry the interface wants added to the page's context menu.
///
/// The engine builds the real menu; the interface only says what to add —
/// which is why this is a list of intents, not a widget.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MenuItem {
    Separator,
    /// A window action (`win.view-source`) to trigger on the page's window.
    Action {
        label: String,
        action: String,
    },
}

/// One find action. `First` starts a search, `Next`/`Previous` move inside it,
/// so the find bar uses a single entry point for typing and for the arrows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FindRequest {
    pub query: String,
    pub step: FindStep,
    pub wrap: bool,
    pub case_sensitive: bool,
    /// Matches past this count are not reported. The bar shows a number, so
    /// the ceiling is part of the visible behaviour, not an engine detail.
    pub max_matches: u32,
}

impl FindRequest {
    /// A fresh, case-insensitive, wrap-around search with the bar's ceiling.
    pub fn first(query: impl Into<String>) -> Self {
        Self {
            query: query.into(),
            step: FindStep::First,
            wrap: true,
            case_sensitive: false,
            max_matches: 500,
        }
    }

    /// Advance to the next match of the same query.
    pub fn next(&self) -> Self {
        Self {
            step: FindStep::Next,
            ..self.clone()
        }
    }

    /// Go back to the previous match of the same query.
    pub fn previous(&self) -> Self {
        Self {
            step: FindStep::Previous,
            ..self.clone()
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FindStep {
    First,
    Next,
    Previous,
}

/// What a search reports. There is deliberately no "current index": WebKit
/// reports the match count only, so a field for it could not be honoured by
/// the Linux backend.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FindOutcome {
    pub matches: u32,
    pub found: bool,
}

/// Disconnects a signal or hook when dropped, or eagerly via [`disconnect`].
///
/// The find bar needs the eager form: it stops a search as soon as the user
/// closes the bar or switches tab, not whenever the handle finally drops.
///
/// [`disconnect`]: Subscription::disconnect
pub struct Subscription {
    disconnect: Option<Box<dyn FnOnce()>>,
}

impl Subscription {
    pub fn new(disconnect: Box<dyn FnOnce()>) -> Self {
        Self {
            disconnect: Some(disconnect),
        }
    }

    fn run(&mut self) {
        if let Some(disconnect) = self.disconnect.take() {
            disconnect();
        }
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        self.run();
    }
}

impl std::fmt::Debug for Subscription {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Subscription")
            .field("connected", &self.disconnect.is_some())
            .finish()
    }
}

/// A feature the running backend does not implement.
///
/// The interface asks [`Capabilities`] before offering the action, so this is
/// the backstop for a call that got through anyway; it never means "silently
/// did nothing".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Unsupported;

impl std::fmt::Display for Unsupported {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("not supported by this engine")
    }
}

impl std::error::Error for Unsupported {}

#[cfg(test)]
mod tests {
    use super::{AuthPrompt, Credentials, FindRequest, FindStep, Subscription};
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    #[test]
    fn a_find_request_keeps_its_query_when_stepping() {
        let first = FindRequest::first("axys");

        assert_eq!(first.step, FindStep::First);
        assert!(first.wrap);
        assert!(!first.case_sensitive);
        assert_eq!(first.max_matches, 500);

        let next = first.next();

        assert_eq!(next.step, FindStep::Next);
        assert_eq!(next.query, "axys");
        assert_eq!(next.max_matches, first.max_matches);
        assert_eq!(first.previous().step, FindStep::Previous);
    }

    #[test]
    fn a_subscription_disconnects_exactly_once() {
        let calls = Rc::new(Cell::new(0));

        {
            let calls = calls.clone();

            Subscription::new(Box::new(move || calls.set(calls.get() + 1)));
        }

        assert_eq!(calls.get(), 1, "dropping must disconnect");
    }

    #[test]
    fn an_auth_prompt_answers_exactly_once() {
        let answers = Rc::new(RefCell::new(Vec::new()));
        let sink = answers.clone();

        let prompt = AuthPrompt::new(
            "example.test".to_string(),
            "restricted".to_string(),
            true,
            Some("ana".to_string()),
            Box::new(move |credentials| sink.borrow_mut().push(credentials)),
        );

        assert_eq!(prompt.host(), "example.test");
        assert_eq!(prompt.realm(), "restricted");
        assert!(prompt.is_retry());
        assert_eq!(prompt.proposed_username(), Some("ana"));

        prompt.answer(Some(Credentials {
            username: "ana".to_string(),
            password: "s3cret".to_string(),
        }));
        prompt.answer(None);

        let answers = answers.borrow();

        assert_eq!(answers.len(), 1, "a second answer must be ignored");
        assert_eq!(answers[0].as_ref().unwrap().username, "ana");
    }
}
