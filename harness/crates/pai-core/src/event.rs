//! Typed events, in three modes: [`Notify`] for observation, [`First`] for the first
//! listener that answers, and [`Waterfall`] for surrounding middleware.

use futures::future::BoxFuture;

/// An event for observation only. Listeners cannot answer.
pub trait Notify: Send + Sync + 'static {
    const NAME: &'static str;
    type Payload: Send + Sync + 'static;
}

/// An event that stops at the first listener to answer.
pub trait First: Send + Sync + 'static {
    const NAME: &'static str;
    type Payload: Send + Sync + 'static;
    type Out: Send + 'static;
}

/// Surrounding middleware: `Req` is edited on the way down, `Out` flows back up, no `next` vetoes.
pub trait Waterfall: Send + Sync + 'static {
    const NAME: &'static str;
    type Req: Send + 'static;
    type Out: Send + 'static;
}

/// The innermost behaviour of a waterfall chain, run once every middleware has delegated.
pub type Tail<'t, E> = &'t (
        dyn for<'r> Fn(&'r mut <E as Waterfall>::Req) -> BoxFuture<'r, <E as Waterfall>::Out>
            + Send
            + Sync
    );

/// A pointer to the rest of the chain; it consumes itself, so it cannot delegate twice.
pub struct Next<'a, E: Waterfall> {
    pub(crate) rest: &'a [std::sync::Arc<dyn Middleware<E>>],
    pub(crate) tail: Tail<'a, E>,
}

impl<'a, E: Waterfall> Next<'a, E> {
    pub async fn run(self, req: &mut E::Req) -> E::Out {
        match self.rest.split_first() {
            Some((head, rest)) => {
                head.call(
                    req,
                    Next {
                        rest,
                        tail: self.tail,
                    },
                )
                .await
            }
            None => (self.tail)(req).await,
        }
    }
}

/// One layer of the chain; returns `BoxFuture` because this trait is used as `dyn`.
pub trait Middleware<E: Waterfall>: Send + Sync + 'static {
    fn call<'a>(&'a self, req: &'a mut E::Req, next: Next<'a, E>) -> BoxFuture<'a, E::Out>;
}
