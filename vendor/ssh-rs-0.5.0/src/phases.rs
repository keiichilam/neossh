//! ssh4 addition: scoped, metadata-only startup progress on the calling thread.
use std::{cell::RefCell, sync::Arc};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnectionPhase {
    Version,
    KeyExchange,
    Authentication,
    PtyRequest,
    ShellRequest,
    ShellRequestSent,
}
type Observer = Arc<dyn Fn(ConnectionPhase) + Send + Sync>;
thread_local! {
    static OBSERVER: RefCell<Option<Observer>> = const { RefCell::new(None) };
}
struct Restore(Option<Observer>);
impl Drop for Restore {
    fn drop(&mut self) {
        OBSERVER.with(|slot| *slot.borrow_mut() = self.0.take());
    }
}
/// Observe only operations inside this scope, restoring the previous observer
/// even on panic. Events contain no credentials or environment values.
pub fn with_phase_observer<T>(observer: Observer, operation: impl FnOnce() -> T) -> T {
    let previous = OBSERVER.with(|slot| slot.borrow_mut().replace(observer));
    let _restore = Restore(previous);
    operation()
}
pub(crate) fn notify(phase: ConnectionPhase) {
    let observer = OBSERVER.with(|slot| slot.borrow().clone());
    if let Some(observer) = observer {
        observer(phase);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    #[test]
    fn nested_panic_and_other_threads_do_not_leak_observers() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let copy = seen.clone();
        with_phase_observer(Arc::new(move |p| copy.lock().unwrap().push(p)), || {
            notify(ConnectionPhase::Version);
            std::thread::spawn(|| notify(ConnectionPhase::ShellRequest))
                .join()
                .unwrap();
            let _ = std::panic::catch_unwind(|| {
                with_phase_observer(Arc::new(|_| {}), || {
                    notify(ConnectionPhase::PtyRequest);
                    panic!("fixture");
                });
            });
            notify(ConnectionPhase::KeyExchange);
        });
        notify(ConnectionPhase::Authentication);
        assert_eq!(
            *seen.lock().unwrap(),
            vec![ConnectionPhase::Version, ConnectionPhase::KeyExchange]
        );
    }
}
