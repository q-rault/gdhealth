use std::panic::{self, AssertUnwindSafe};

/// A report type that can represent "this subsystem could not run".
///
/// Every new, best-effort subsystem (git intelligence, duplication detection,
/// refactoring suggestions) implements this so `isolate` can degrade to a
/// safe value instead of ever crashing the whole command.
pub trait Diagnosable {
    fn unavailable(reason: String) -> Self;
}

/// Runs `f`, catching any panic so one broken subsystem never aborts the
/// whole run. On panic, prints a short notice to stderr and returns
/// `T::unavailable(reason)` instead of propagating.
pub fn isolate<T, F>(name: &str, f: F) -> T
where
    T: Diagnosable,
    F: FnOnce() -> T,
{
    match panic::catch_unwind(AssertUnwindSafe(f)) {
        Ok(value) => value,
        Err(payload) => {
            let reason = panic_message(payload);
            eprintln!("gdhealth: {name} failed and was disabled for this run: {reason}");
            T::unavailable(reason)
        }
    }
}

/// Same fault isolation as `isolate`, for callers whose result type has no
/// natural "unavailable" variant (e.g. `Vec<PolicyFinding>`) — falls back
/// to a caller-supplied `default` on panic instead of `T::unavailable`.
pub fn isolate_or<T, F>(name: &str, default: T, f: F) -> T
where
    F: FnOnce() -> T,
{
    match panic::catch_unwind(AssertUnwindSafe(f)) {
        Ok(value) => value,
        Err(payload) => {
            let reason = panic_message(payload);
            eprintln!("gdhealth: {name} failed and was disabled for this run: {reason}");
            default
        }
    }
}

fn panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        message.to_string()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "unknown panic".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::{Diagnosable, isolate};

    #[derive(Debug, PartialEq)]
    enum Probe {
        Ok(i32),
        Unavailable(String),
    }

    impl Diagnosable for Probe {
        fn unavailable(reason: String) -> Self {
            Probe::Unavailable(reason)
        }
    }

    #[test]
    fn returns_value_when_closure_succeeds() {
        let result = isolate("probe", || Probe::Ok(42));
        assert_eq!(result, Probe::Ok(42));
    }

    #[test]
    fn catches_panics_and_returns_unavailable() {
        let result = isolate::<Probe, _>("probe", || panic!("boom"));
        assert!(matches!(result, Probe::Unavailable(reason) if reason.contains("boom")));
    }
}
