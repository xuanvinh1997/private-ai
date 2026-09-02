//! Typed events.
//!
//! Cordis has five emit modes. Two of them — `serial` and `bail` — are separate only
//! because JavaScript distinguishes `T` from `Promise<T>` at the call site. Rust does not
//! have that problem, so three remain:
//!
//! | Cordis            | Here                                   |
//! |-------------------|----------------------------------------|
//! | `emit`, `parallel`| [`Notify`] → `Context::notify`         |
//! | `serial`, `bail`  | [`First`]  → `Context::first`          |
//! | `waterfall`       | [`Waterfall`] → `Context::waterfall`   |

use futures::future::BoxFuture;

/// An event for observation only. Listeners cannot answer.
pub trait Notify: Send + Sync + 'static {
    const NAME: &'static str;
    type Payload: Send + Sync + 'static;
}

/// An event that stops at the first listener to answer. Replaces `serial` + `bail`.
pub trait First: Send + Sync + 'static {
    const NAME: &'static str;
    type Payload: Send + Sync + 'static;
    type Out: Send + 'static;
}

/// Surrounding middleware — the Rust version of `waterfall`.
///
/// `Req` is the shared request listeners may edit on the way down; `Out` is the result
/// flowing back up. Not calling `next` is a veto, exactly as in Cordis.
pub trait Waterfall: Send + Sync + 'static {
    const NAME: &'static str;
    type Req: Send + 'static;
    type Out: Send + 'static;
}

/// The innermost behaviour of a waterfall chain — what runs once every middleware has
/// delegated.
pub type Tail<'t, E> = &'t (
        dyn for<'r> Fn(&'r mut <E as Waterfall>::Req) -> BoxFuture<'r, <E as Waterfall>::Out>
            + Send
            + Sync
    );

/// A pointer to the rest of the chain.
///
/// It consumes itself when run, so it **cannot delegate twice**. Cordis allows calling
/// `next()` more than once and that is a source of bugs; here the compiler stops it.
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

/// One layer of the chain.
///
/// Returns a `BoxFuture` rather than using `async fn`: this trait is always used as `dyn`,
/// and `async fn` in a trait is not dyn-safe.
pub trait Middleware<E: Waterfall>: Send + Sync + 'static {
    fn call<'a>(&'a self, req: &'a mut E::Req, next: Next<'a, E>) -> BoxFuture<'a, E::Out>;
}
