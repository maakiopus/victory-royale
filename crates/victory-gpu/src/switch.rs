//! The `VICTORY_*` on/off switches (`VICTORY_D3D12_DEBUG=1` and its
//! siblings).
//!
//! The console has nothing like these: they are our own debugging aids, read
//! once from the environment. This module holds the one reading of a switch's
//! value, so that every switch documented as `=1` in the README's table
//! answers the same way to the same value.

use std::ffi::OsStr;

/// Whether a switch's value turns it on: it is set, and is not `0`.
///
/// A switch used to be on whenever its variable existed, so
/// `VICTORY_D3D12_WARP=0` (typed to switch it off, or left over from
/// comparing two runs) still selected the software adapter. Any other value
/// keeps the meaning it had, so a run that already sets one of these to
/// something else behaves as before.
fn value_is_on(value: Option<&OsStr>) -> bool {
    value.is_some_and(|v| v != "0")
}

/// Whether the environment variable `name` switches something on.
pub(crate) fn on(name: &str) -> bool {
    value_is_on(std::env::var_os(name).as_deref())
}

#[cfg(test)]
mod tests {
    use super::value_is_on;
    use std::ffi::OsStr;

    #[test]
    fn unset_is_off() {
        assert!(!value_is_on(None));
    }

    #[test]
    fn zero_is_off() {
        assert!(!value_is_on(Some(OsStr::new("0"))));
    }

    #[test]
    fn one_is_on() {
        assert!(value_is_on(Some(OsStr::new("1"))));
    }

    #[test]
    fn other_values_keep_their_old_meaning() {
        // Documented as `=1`; anything else that was on before stays on:
        // only `0` is new.
        for value in ["", "true", "yes", "2", "00", " 0", "0 "] {
            assert!(value_is_on(Some(OsStr::new(value))), "{value:?}");
        }
    }
}
