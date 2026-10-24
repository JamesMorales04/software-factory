#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum State {
    Pass,
    Fail,
    Blocked,
}

impl State {
    fn label(self) -> &'static str {
        match self {
            Self::Pass => "PASS",
            Self::Fail => "FAIL",
            Self::Blocked => "BLOCKED",
        }
    }
}

#[derive(Debug)]
pub(crate) struct Check {
    state: State,
    name: &'static str,
    detail: String,
    required: bool,
}

impl Check {
    pub(crate) fn pass(name: &'static str, detail: impl Into<String>) -> Self {
        Self::new(State::Pass, name, detail)
    }

    pub(crate) fn fail(name: &'static str, detail: impl Into<String>) -> Self {
        Self::new(State::Fail, name, detail)
    }

    pub(crate) fn blocked(name: &'static str, detail: impl Into<String>) -> Self {
        Self::new(State::Blocked, name, detail)
    }

    pub(crate) fn informational(mut self) -> Self {
        self.required = false;
        self
    }

    pub(crate) fn is_required_failure(&self) -> bool {
        self.required && self.state != State::Pass
    }

    pub(crate) fn print(&self) {
        println!("{:<7} {}: {}", self.state.label(), self.name, self.detail);
    }

    fn new(state: State, name: &'static str, detail: impl Into<String>) -> Self {
        Self {
            state,
            name,
            detail: detail.into(),
            required: true,
        }
    }
}
