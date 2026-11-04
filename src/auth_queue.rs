// SPDX-License-Identifier: GPL-2.0-or-later
//! The Authenticate sheet's bookkeeping, shared by Settings and the Control Center, GTK-free
//! (QA 0.12.0 bugs 4, 6; QA hunt 1 D1). Every change that answered `NeedsAuth` waits here in
//! order; one password re-runs all of them, each with its own copy (a one-shot slot went to
//! whichever ran first); Cancel ends all of them. `W` is what the surface keeps per change
//! (Settings: the action, its callback and status line; Control Center: the action and the tile
//! it holds).
use crate::system::ActionError;
use std::collections::VecDeque;

/// After this many wrong passwords in a row the sheet stops asking: every waiting change fails
/// and its control flips back (QA 0.12.0 bug 4: the sheet looped forever).
pub(crate) const MAX_REFUSALS: u32 = 3;

/// What the Authenticate sheet does with one answer.
pub(crate) enum Next<W> {
    /// The change ended: its `done` gets this.
    Finish(W, Result<(), ActionError>),
    /// It waits for the password: the sheet shows, with this sentence (may be empty).
    Ask(&'static str),
    /// Too many wrong passwords: every waiting change fails with `Denied`.
    GiveUp(Vec<W>),
}

/// The changes waiting on the Authenticate sheet. A single slot lost the first change and left
/// its control grey.
pub(crate) struct AuthQueue<W> {
    waiting: VecDeque<W>,
    /// Wrong passwords in a row (one per Allow, however many changes it re-ran).
    refusals: u32,
    /// The Allow that was last counted as a wrong password.
    counted: u32,
    /// Allows so far: numbers the retries.
    attempt: u32,
    /// Retries sent with a typed password and not answered yet.
    checking: usize,
    /// The Allow whose wrong password ended the sheet: its later answers fail at once too.
    gave_up: u32,
}

impl<W> Default for AuthQueue<W> {
    fn default() -> Self {
        AuthQueue {
            waiting: VecDeque::new(),
            refusals: 0,
            counted: 0,
            attempt: 0,
            checking: 0,
            gave_up: 0,
        }
    }
}

impl<W> AuthQueue<W> {
    /// A change was sent; `retry` = the Allow it carries the password of.
    pub(crate) fn sent(&mut self, retry: Option<u32>) {
        if retry.is_some() {
            self.checking += 1;
        }
    }
    /// The answer `r` to `w` (sent with the password of Allow `retry`, if any).
    pub(crate) fn answered(
        &mut self,
        w: W,
        r: Result<(), ActionError>,
        retry: Option<u32>,
    ) -> Next<W> {
        if retry.is_some() {
            self.checking = self.checking.saturating_sub(1);
        }
        match r {
            Err(ActionError::NeedsAuth | ActionError::Denied)
                if retry.is_some() && retry == Some(self.gave_up) =>
            {
                Next::Finish(w, Err(ActionError::Denied))
            }
            Err(e @ (ActionError::NeedsAuth | ActionError::Denied)) => {
                if let Some(n) = retry
                    && n != self.counted
                {
                    self.counted = n;
                    self.refusals += 1;
                }
                self.waiting.push_back(w);
                if self.refusals >= MAX_REFUSALS {
                    self.refusals = 0;
                    self.gave_up = retry.unwrap_or_default();
                    return Next::GiveUp(self.waiting.drain(..).collect());
                }
                Next::Ask(if retry.is_some() || e == ActionError::Denied {
                    ActionError::Denied.message()
                } else {
                    ""
                })
            }
            r => {
                if retry.is_some() && r.is_ok() {
                    self.refusals = 0;
                }
                Next::Finish(w, r)
            }
        }
    }
    /// Allow: every waiting change, in order, and the number of this Allow.
    pub(crate) fn allow(&mut self) -> (Vec<W>, u32) {
        self.attempt = self.attempt.wrapping_add(1).max(1);
        (self.waiting.drain(..).collect(), self.attempt)
    }
    /// Cancel: every waiting change (the surface ends each one).
    pub(crate) fn cancel(&mut self) -> Vec<W> {
        self.refusals = 0;
        self.waiting.drain(..).collect()
    }
    pub(crate) fn len(&self) -> usize {
        self.waiting.len()
    }
    pub(crate) fn checking(&self) -> usize {
        self.checking
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// QA hunt 1 D1: the queue is not Settings'; any surface keeps its own payload in it, and
    /// one queue serves round after round: a wrong password, a right one, then a new pair of
    /// requests starts with no refusal counted and a fresh Allow number.
    #[test]
    fn the_queue_takes_any_payload_and_is_reused_round_after_round() {
        let mut q: AuthQueue<&'static str> = AuthQueue::default();
        for round in 0..3 {
            for name in ["power", "output"] {
                assert!(matches!(
                    q.answered(name, Err(ActionError::NeedsAuth), None),
                    Next::Ask("")
                ));
            }
            assert_eq!(q.len(), 2, "round {round}");
            // A wrong password: both wait again, once counted.
            let (all, n) = q.allow();
            for w in all {
                q.sent(Some(n));
                assert!(matches!(
                    q.answered(w, Err(ActionError::Denied), Some(n)),
                    Next::Ask(m) if m == ActionError::Denied.message()
                ));
            }
            assert_eq!(q.refusals, 1, "round {round}");
            // The right one: both end, in order, and the count starts over.
            let (all, m) = q.allow();
            assert_ne!(m, n);
            let mut ended = vec![];
            for w in all {
                q.sent(Some(m));
                match q.answered(w, Ok(()), Some(m)) {
                    Next::Finish(w, Ok(())) => ended.push(w),
                    _ => panic!("round {round}: not finished"),
                }
            }
            assert_eq!(ended, ["power", "output"]);
            assert_eq!((q.len(), q.checking(), q.refusals), (0, 0, 0));
        }
        // Cancel leaves it empty and ready too.
        let _ = q.answered("night", Err(ActionError::NeedsAuth), None);
        assert_eq!(q.cancel(), ["night"]);
        assert_eq!(q.len(), 0);
    }
}
