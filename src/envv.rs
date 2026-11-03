// SPDX-License-Identifier: GPL-2.0-or-later
//! The shell's one door to the process environment (RR-027): what the session, a test or a VM
//! journey sets for the shell is read here and nowhere else in the library, so the places that
//! depend on it are found by searching for `envv::`. Values are text; a missing or non-UTF-8
//! variable is `None`.

/// The value of variable `key`.
pub fn var(key: &str) -> Option<String> {
    std::env::var(key).ok()
}

/// Whether variable `key` is set to anything at all (even text that is not UTF-8).
pub fn is_set(key: &str) -> bool {
    std::env::var_os(key).is_some()
}
