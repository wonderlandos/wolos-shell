// SPDX-License-Identifier: GPL-2.0-or-later
//! The Authenticate sheet's bookkeeping, shared by Settings and the Control Center, GTK-free
//! (QA 0.12.0 bugs 4, 6; QA hunt 1 D1). Every change that answered `NeedsAuth` waits here in
//! order; one password re-runs all of them, each with its own copy (a one-shot slot went to
//! whichever ran first); Cancel ends all of them. `W` is what the surface keeps per change
//! (Settings: the action, its callback and status line; Control Center: the action and the tile
//! it holds).
use crate::system::ActionError;
use std::collections::{BTreeSet, VecDeque};

/// After this many wrong passwords in a row the sheet stops asking: every waiting change fails
/// and its control flips back (QA 0.12.0 bug 4: the sheet looped forever).
pub(crate) const MAX_REFUSALS: u32 = 3;

/// Let-go Allows remembered: far more than can be out at once (one per typed password), so the
/// sets stay small for a session that asks all day.
const KEPT_ALLOWS: usize = 16;

/// Adds Allow `n` to `set`, dropping the oldest past [`KEPT_ALLOWS`].
fn mark(set: &mut BTreeSet<u32>, n: u32) {
    set.insert(n);
    while set.len() > KEPT_ALLOWS {
        set.pop_first();
    }
}

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
    /// The Allows let go (wrong passwords ended the sheet, [`Self::let_go`], [`Self::cancel`]):
    /// their later refusals finish at once. One set, not one slot: two Allows can be out at once
    /// and a second let-go forgot the first (QA-A10-SD-03). The last [`KEPT_ALLOWS`] only.
    gave_up: BTreeSet<u32>,
    /// The Allows the person cancelled while their retries were out ([`Self::cancel`]): a refusal
    /// for one ends the change as `Cancelled`, not as a wrong password.
    cancelled: BTreeSet<u32>,
}

impl<W> Default for AuthQueue<W> {
    fn default() -> Self {
        AuthQueue {
            waiting: VecDeque::new(),
            refusals: 0,
            counted: 0,
            attempt: 0,
            checking: 0,
            gave_up: BTreeSet::new(),
            cancelled: BTreeSet::new(),
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
        self.next(w, r, retry)
    }
    fn next(&mut self, w: W, r: Result<(), ActionError>, retry: Option<u32>) -> Next<W> {
        match r {
            Err(ActionError::NeedsAuth | ActionError::Denied)
                if retry.is_some_and(|n| self.gave_up.contains(&n)) =>
            {
                // The person cancelled while the password was being checked: nothing was
                // changed by them, so no "Incorrect password" (QA-A9-SHELL-01/02).
                let why = if retry.is_some_and(|n| self.cancelled.contains(&n)) {
                    ActionError::Cancelled
                } else {
                    ActionError::Denied
                };
                Next::Finish(w, Err(why))
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
                    if let Some(n) = retry {
                        mark(&mut self.gave_up, n);
                    }
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
    /// Cancel: every waiting change (the surface ends each one). Retries still out with a typed
    /// password cannot be called back, so the Allow they carry is let go: their late refusal
    /// finishes each as `Cancelled` and never re-asks (QA-A9-SHELL-01 Settings, -02 the Control
    /// Center panel hiding), the one rule for both surfaces.
    pub(crate) fn cancel(&mut self) -> Vec<W> {
        self.refusals = 0;
        if self.checking > 0 {
            mark(&mut self.gave_up, self.attempt);
            mark(&mut self.cancelled, self.attempt);
        }
        self.waiting.drain(..).collect()
    }
    /// The changes waiting, in order.
    pub(crate) fn waiting(&self) -> impl Iterator<Item = &W> {
        self.waiting.iter()
    }
    /// The sheet for Allow `n` was let go while its retries were still out (an install goes on
    /// under its status line): a refusal arriving for it later finishes the change as `Denied`
    /// instead of bringing the sheet back on whatever page is up then (QA-VIS2-35: it came back
    /// a minute later on Desktop & Dock with the old error line).
    pub(crate) fn let_go(&mut self, n: u32) {
        mark(&mut self.gave_up, n);
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

    /// QA-VIS2-35: once the sheet for an Allow was let go (the change runs on under its status
    /// line), a refusal that arrives for it later ends the change as Denied: no sheet comes back.
    /// A change asked after that still gets the sheet.
    #[test]
    fn a_late_refusal_after_the_sheet_went_does_not_bring_it_back() {
        let mut q: AuthQueue<&'static str> = AuthQueue::default();
        let _ = q.answered("remote", Err(ActionError::NeedsAuth), None);
        let (all, n) = q.allow();
        q.sent(Some(n));
        q.let_go(n);
        for w in all {
            assert!(matches!(
                q.answered(w, Err(ActionError::Denied), Some(n)),
                Next::Finish("remote", Err(ActionError::Denied))
            ));
        }
        assert_eq!((q.len(), q.checking()), (0, 0));
        assert!(matches!(
            q.answered("dock", Err(ActionError::NeedsAuth), None),
            Next::Ask("")
        ));
    }

    /// QA-A9-SHELL-01/02: Cancel (Esc, a pane change, the Control Center panel hiding) while the
    /// typed password is still being checked lets that Allow go: the refusal that arrives later
    /// ends the change as `Cancelled` (nothing changed, no "Incorrect password") and the sheet
    /// does not come back with the cancelled change waiting behind it.
    #[test]
    fn cancel_while_checking_lets_the_allow_go() {
        let mut q: AuthQueue<&'static str> = AuthQueue::default();
        let _ = q.answered("remote", Err(ActionError::NeedsAuth), None);
        let (all, n) = q.allow();
        assert_eq!(all, ["remote"]);
        q.sent(Some(n));
        assert!(q.cancel().is_empty(), "the change is out with the password");
        assert!(matches!(
            q.answered("remote", Err(ActionError::Denied), Some(n)),
            Next::Finish("remote", Err(ActionError::Cancelled))
        ));
        assert_eq!((q.len(), q.checking()), (0, 0));
        // The next change asks again, and a wrong password then counts from zero.
        assert!(matches!(
            q.answered("dock", Err(ActionError::NeedsAuth), None),
            Next::Ask("")
        ));
        assert_eq!(q.refusals, 0);
    }

    /// A cancel with no retry out is the plain one and leaves nothing to let go: the next Allow's
    /// refusal asks again as before.
    #[test]
    fn cancel_with_nothing_checking_lets_nothing_go() {
        let mut q: AuthQueue<&'static str> = AuthQueue::default();
        let _ = q.answered("a", Err(ActionError::NeedsAuth), None);
        assert_eq!(q.cancel(), ["a"]);
        let _ = q.answered("b", Err(ActionError::NeedsAuth), None);
        let (all, n) = q.allow();
        q.sent(Some(n));
        for w in all {
            assert!(matches!(
                q.answered(w, Err(ActionError::Denied), Some(n)),
                Next::Ask(m) if m == ActionError::Denied.message()
            ));
        }
    }

    /// The sheet was cancelled while checking and the password was right: the change had already
    /// been sent with it, so it ends as it ended (Ok); only a refusal becomes `Cancelled`.
    #[test]
    fn a_right_password_after_a_cancel_still_ends_the_change() {
        let mut q: AuthQueue<&'static str> = AuthQueue::default();
        let _ = q.answered("remote", Err(ActionError::NeedsAuth), None);
        let (all, n) = q.allow();
        q.sent(Some(n));
        let _ = q.cancel();
        for w in all {
            assert!(matches!(
                q.answered(w, Ok(()), Some(n)),
                Next::Finish("remote", Ok(()))
            ));
        }
    }

    /// The let-go of a slow change (AUTH_HOLD) still ends a refusal as `Denied`: the person did
    /// not cancel, the password was wrong.
    #[test]
    fn a_hold_let_go_still_reads_denied() {
        let mut q: AuthQueue<&'static str> = AuthQueue::default();
        let _ = q.answered("remote", Err(ActionError::NeedsAuth), None);
        let (all, n) = q.allow();
        q.sent(Some(n));
        q.let_go(n);
        for w in all {
            assert!(matches!(
                q.answered(w, Err(ActionError::Denied), Some(n)),
                Next::Finish("remote", Err(ActionError::Denied))
            ));
        }
    }

    /// QA-A10-SD-03: two Allows out at once. Allow 1 (a slow install) was let go by AUTH_HOLD;
    /// a second change asks, Allow 2 goes out and the person cancels while it is checked. Each
    /// Allow keeps its own let-go: 2's refusal ends as Cancelled, 1's late refusal as Denied, and
    /// neither re-asks (a single slot forgot 1 and reopened the sheet).
    #[test]
    fn a_cancel_of_a_later_allow_keeps_an_earlier_let_go() {
        let mut q: AuthQueue<&'static str> = AuthQueue::default();
        let _ = q.answered("install", Err(ActionError::NeedsAuth), None);
        let (_, one) = q.allow();
        q.sent(Some(one));
        q.let_go(one);
        let _ = q.answered("remote", Err(ActionError::NeedsAuth), None);
        let (_, two) = q.allow();
        q.sent(Some(two));
        assert!(q.cancel().is_empty());
        assert!(matches!(
            q.answered("remote", Err(ActionError::Denied), Some(two)),
            Next::Finish("remote", Err(ActionError::Cancelled))
        ));
        assert!(matches!(
            q.answered("install", Err(ActionError::Denied), Some(one)),
            Next::Finish("install", Err(ActionError::Denied))
        ));
        assert_eq!((q.len(), q.checking()), (0, 0));
    }

    /// QA-A10-SD-03, the mirror: a let-go of a later Allow keeps the cancel of an earlier one.
    #[test]
    fn a_later_let_go_keeps_an_earlier_cancel() {
        let mut q: AuthQueue<&'static str> = AuthQueue::default();
        let _ = q.answered("a", Err(ActionError::NeedsAuth), None);
        let (_, one) = q.allow();
        q.sent(Some(one));
        let _ = q.cancel();
        let _ = q.answered("b", Err(ActionError::NeedsAuth), None);
        let (_, two) = q.allow();
        q.sent(Some(two));
        q.let_go(two);
        assert!(matches!(
            q.answered("a", Err(ActionError::Denied), Some(one)),
            Next::Finish("a", Err(ActionError::Cancelled))
        ));
        assert!(matches!(
            q.answered("b", Err(ActionError::Denied), Some(two)),
            Next::Finish("b", Err(ActionError::Denied))
        ));
        assert_eq!((q.len(), q.checking()), (0, 0));
        // The sets keep the last Allows only, however long the session.
        for n in 3..100 {
            q.let_go(n);
        }
        assert_eq!(q.gave_up.len(), KEPT_ALLOWS);
        assert!(q.gave_up.contains(&99) && !q.gave_up.contains(&two));
    }
}
