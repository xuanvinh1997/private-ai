//! Sự kiện có kiểu.
//!
//! Cordis có năm chế độ phát. Hai trong số đó — `serial` và `bail` — chỉ tách nhau vì
//! JavaScript phân biệt `T` với `Promise<T>` ngay tại chỗ gọi. Rust không có vấn đề đó,
//! nên ở đây còn ba:
//!
//! | Cordis            | Ở đây                                  |
//! |-------------------|----------------------------------------|
//! | `emit`, `parallel`| [`Notify`] → `Context::notify`         |
//! | `serial`, `bail`  | [`First`]  → `Context::first`          |
//! | `waterfall`       | [`Waterfall`] → `Context::waterfall`   |

use futures::future::BoxFuture;

/// Sự kiện chỉ để quan sát. Listener không trả lời được.
pub trait Notify: Send + Sync + 'static {
    const NAME: &'static str;
    type Payload: Send + Sync + 'static;
}

/// Sự kiện dừng ở listener đầu tiên trả lời. Thay cho `serial` + `bail`.
pub trait First: Send + Sync + 'static {
    const NAME: &'static str;
    type Payload: Send + Sync + 'static;
    type Out: Send + 'static;
}

/// Middleware bao quanh — bản Rust của `waterfall`.
///
/// `Req` là yêu cầu dùng chung mà listener được phép sửa trên đường xuôi; `Out` là kết
/// quả chảy ngược lên. Không gọi `next` nghĩa là phủ quyết, đúng như Cordis.
pub trait Waterfall: Send + Sync + 'static {
    const NAME: &'static str;
    type Req: Send + 'static;
    type Out: Send + 'static;
}

/// Hành vi trong cùng của một chuỗi waterfall — cái chạy khi mọi middleware đã uỷ quyền.
pub type Tail<'t, E> = &'t (
        dyn for<'r> Fn(&'r mut <E as Waterfall>::Req) -> BoxFuture<'r, <E as Waterfall>::Out>
            + Send
            + Sync
    );

/// Con trỏ tới phần còn lại của chuỗi.
///
/// Nó tiêu thụ chính mình khi chạy, nên **không thể uỷ quyền hai lần**. Cordis cho phép
/// gọi `next()` nhiều lần và đó là nguồn lỗi; ở đây trình biên dịch chặn.
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

/// Một tầng của chuỗi.
///
/// Trả về `BoxFuture` chứ không dùng `async fn`: trait này luôn được dùng dưới dạng
/// `dyn`, mà `async fn` trong trait thì không dyn-safe.
pub trait Middleware<E: Waterfall>: Send + Sync + 'static {
    fn call<'a>(&'a self, req: &'a mut E::Req, next: Next<'a, E>) -> BoxFuture<'a, E::Out>;
}
