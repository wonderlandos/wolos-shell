// SPDX-License-Identifier: GPL-2.0-or-later
//! Where the shell's own diagnostics go. A library does not print (RR-004); every line the shell
//! says about itself (a socket that failed, a file not applied, a part that stopped) passes
//! through [`emit`], so the one place that decides where it goes is here: the session's stderr
//! file, which the supervisor keeps small (`supervise::cap_log`).
use std::fmt::Arguments;

/// One diagnostic line on the session's stderr.
pub fn emit(line: Arguments<'_>) {
    eprintln!("{line}");
}

/// `say!("wol-shell: {what}: {e}")`: [`emit`] with format arguments.
macro_rules! say {
    ($($t:tt)*) => {
        $crate::diag::emit(format_args!($($t)*))
    };
}
pub(crate) use say;
