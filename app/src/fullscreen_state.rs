//! Dependency-free request state for fullscreen and its page-fill fallback.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Action {
    None,
    ExitNative,
    ClearFallback,
    Start(u64),
    CancelPending,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct NativeSettlement {
    pub current: bool,
    pub alive: bool,
    pub cancelled: bool,
}

#[derive(Default)]
pub(crate) struct State {
    generation: u64,
    pending: Option<u64>,
    unresolved: Option<u64>,
    cancelled: Option<u64>,
    alive: bool,
}

impl State {
    pub(crate) fn new() -> Self {
        Self {
            alive: true,
            ..Self::default()
        }
    }

    fn advance(&mut self) -> u64 {
        self.generation = self.generation.wrapping_add(1);
        self.generation
    }

    pub(crate) fn toggle(&mut self, native: bool, expanded: bool) -> Action {
        if !self.alive {
            return Action::None;
        }
        if native {
            if !self.cancel() {
                self.advance();
            }
            return Action::ExitNative;
        }
        if self.pending.is_some() {
            self.cancel();
            return Action::CancelPending;
        }
        if expanded {
            if !self.cancel() {
                self.advance();
            }
            return Action::ClearFallback;
        }

        let token = self.advance();
        self.cancelled = None;
        self.pending = Some(token);
        self.unresolved = Some(token);
        Action::Start(token)
    }

    /// A fullscreenchange in either direction supersedes the delayed fallback.
    pub(crate) fn fullscreen_changed(&mut self, token: u64) -> bool {
        if !self.alive || self.pending != Some(token) {
            return false;
        }
        self.pending = None;
        self.advance();
        true
    }

    /// Whether a late request Promise may have entered fullscreen after its caller cancelled.
    pub(crate) fn should_exit_late_native(&self, token: u64) -> bool {
        self.cancelled == Some(token)
    }

    pub(crate) fn cancelled_token(&self) -> Option<u64> {
        self.cancelled
    }

    /// The native request Promise settled; its token no longer needs late-entry cleanup.
    pub(crate) fn settle_native(&mut self, token: u64) -> NativeSettlement {
        let current = self.unresolved == Some(token);
        let cancelled = current && self.should_exit_late_native(token);
        if current {
            self.unresolved = None;
            if cancelled {
                self.cancelled = None;
            }
        }
        NativeSettlement {
            current,
            alive: current && self.alive,
            cancelled,
        }
    }

    pub(crate) fn cancel(&mut self) -> bool {
        if !self.alive {
            return false;
        }
        let Some(token) = self.unresolved.or(self.pending) else {
            return false;
        };
        self.pending = None;
        self.cancelled = Some(token);
        self.advance();
        true
    }

    pub(crate) fn dispose(&mut self) {
        self.alive = false;
        if let Some(token) = self.unresolved.or(self.pending) {
            self.cancelled = Some(token);
        }
        self.pending = None;
        self.advance();
    }

    /// Complete the one active request. A denied/no-op API may use the fallback, but only while
    /// the request remains current and its original player element is still mounted.
    pub(crate) fn finish(
        &mut self,
        token: u64,
        no_native_fullscreen: bool,
        same_player: bool,
    ) -> bool {
        if !self.alive || self.pending != Some(token) {
            return false;
        }
        self.pending = None;
        self.advance();
        no_native_fullscreen && same_player
    }
}
