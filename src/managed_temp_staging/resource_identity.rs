//! One-time resource identity publication shared with asynchronous cleanup.

use std::future::Future;
use std::pin::Pin;
use std::sync::Mutex;
#[cfg(test)]
use std::sync::Arc;
use std::task::Context;
use std::task::Poll;
use std::task::Waker;

use qubit_operation::ResourceId;

#[cfg(test)]
#[derive(Clone, Copy, Debug)]
pub(crate) enum PublishEvent {
    CleanupPending,
    Publishing(ResourceId),
}

#[cfg(test)]
pub(crate) type PublishHook = Arc<dyn Fn(PublishEvent) + Send + Sync>;

#[derive(Default)]
struct State {
    value: Option<ResourceId>,
    next_waiter: u64,
    waiter: Option<(u64, Waker)>,
    #[cfg(test)]
    hook: Option<PublishHook>,
}

/// A resource identity that can be awaited while the owner is being retained.
#[derive(Default)]
pub(super) struct ResourceIdentity(Mutex<State>);

impl ResourceIdentity {
    #[cfg(test)]
    pub(crate) fn set_hook(&self, hook: PublishHook) {
        self.0.lock().unwrap_or_else(|poison| poison.into_inner()).hook = Some(hook);
    }
    #[cfg(test)]
    pub(super) fn published(id: ResourceId) -> Self {
        Self(Mutex::new(State {
            value: Some(id),
            ..State::default()
        }))
    }

    pub(super) fn publish(&self, id: ResourceId) {
        #[cfg(test)]
        let hook = self.0.lock().unwrap_or_else(|poison| poison.into_inner()).hook.clone();
        #[cfg(test)]
        if let Some(hook) = hook {
            hook(PublishEvent::Publishing(id));
        }
        let waiter = {
            let mut state = self.0.lock().unwrap_or_else(|poison| poison.into_inner());
            assert!(
                state.value.is_none(),
                "resource identity published more than once"
            );
            state.value = Some(id);
            state.waiter.take().map(|(_, waiter)| waiter)
        };
        if let Some(waiter) = waiter {
            waiter.wake();
        }
    }

    pub(super) fn wait(&self) -> IdentityFuture<'_> {
        let token = {
            let mut state = self.0.lock().unwrap_or_else(|poison| poison.into_inner());
            state.next_waiter = state.next_waiter.wrapping_add(1);
            state.next_waiter
        };
        IdentityFuture {
            identity: self,
            token,
        }
    }
}

pub(super) struct IdentityFuture<'a> {
    identity: &'a ResourceIdentity,
    token: u64,
}

impl Future for IdentityFuture<'_> {
    type Output = ResourceId;

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        let candidate = context.waker().clone();
        let (result, old_waiter, _hook) = {
            let mut state = self
                .identity
                .0
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            if let Some(id) = state.value {
                (Poll::Ready(id), None, None)
            } else {
                let old = state
                    .waiter
                    .replace((self.token, candidate))
                    .map(|(_, waker)| waker);
                #[cfg(test)]
                let hook = state.hook.clone();
                #[cfg(not(test))]
                let hook: Option<()> = None;
                (Poll::Pending, old, hook)
            }
        };
        drop(old_waiter);
        #[cfg(test)]
        if result.is_pending() {
            if let Some(hook) = _hook {
                hook(PublishEvent::CleanupPending);
            }
        }
        result
    }
}

impl Drop for IdentityFuture<'_> {
    fn drop(&mut self) {
        let waiter = {
            let mut state = self
                .identity
                .0
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            if state
                .waiter
                .as_ref()
                .is_some_and(|(token, _)| *token == self.token)
            {
                state.waiter.take().map(|(_, waker)| waker)
            } else {
                None
            }
        };
        drop(waiter);
    }
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::sync::Arc;
    use std::task::Context;
    use std::task::Wake;
    use std::task::Waker;

    use super::ResourceIdentity;

    struct NoopWake;

    impl Wake for NoopWake {
        fn wake(self: Arc<Self>) {}
    }

    #[test]
    fn test_cancelled_identity_wait_removes_its_waker() {
        let identity = ResourceIdentity::default();
        let waker = Waker::from(Arc::new(NoopWake));
        let mut context = Context::from_waker(&waker);
        let mut future = Box::pin(identity.wait());
        assert!(future.as_mut().poll(&mut context).is_pending());
        assert!(identity.0.lock().unwrap().waiter.is_some());
        drop(future);
        assert!(identity.0.lock().unwrap().waiter.is_none());
    }
}
