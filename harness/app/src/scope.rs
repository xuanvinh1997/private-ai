//! Phạm vi tool của **một lượt**, dịch sang hạn chế của sổ đăng ký.
//!
//! Người dùng chọn phạm vi ngay trong ô soạn tin, và nó chỉ có nghĩa với đúng lượt sắp
//! gửi. Vì vậy cách cài đặt là một **phạm vi con dựng riêng cho lượt**: hạn chế cắm vào
//! đó sống bằng scope ấy, và dọn scope là quyền trở lại như cũ. Không có trạng thái nào
//! dính lại giữa hai lượt, và hai phiên chạy song song không siết lẫn nhau — chuyện sẽ
//! xảy ra nếu tất cả dùng chung một phạm vi.
//!
//! Việc siết dùng [`ToolRegistry::restrict`] chứ không phải một bộ lọc thứ hai viết
//! riêng: sổ đăng ký đã kiểm quyền ở **cả hai tầng** — lúc liệt kê schema và lúc tra cứu
//! tên mô hình gửi — bằng cùng một hàm `permits`. Dựng một đường song song nghĩa là chỉ
//! có tầng liệt kê được siết, mà tầng liệt kê là tầng mô hình đi vòng qua được: bản ghi
//! phiên còn nguyên các lượt cũ, nên nó nhớ `bash` tồn tại kể cả khi lượt này không
//! quảng cáo `bash` nữa.

use std::sync::Arc;

use futures::FutureExt;
use futures::future::BoxFuture;
use pai_agent::AgentRequest;
use pai_core::{Context, Middleware, Next, ScopeKey};
use pai_llm::{ChatRequest, Message};
use pai_tools::{ToolRegistry, ToolRestriction, Tools};

use crate::protocol::ToolScope;

/// Nhóm tool thi hành lệnh trên máy này — ranh giới giữa `write` và `shell`.
///
/// **Đây là một khoản nợ, và đây là chỗ duy nhất nó tồn tại.** `ToolMeta` phân biệt được
/// "có đổi trạng thái không" (`mutating`) nhưng **không** phân biệt được "có chạy được
/// lệnh tuỳ ý không": `edit` và `bash` đều `mutating: true`. Vì thiếu trường ấy, danh
/// sách dưới đây phải viết tay, và nó *fail open* theo đúng cái chiều nguy hiểm — một
/// tool chạy lệnh thêm vào sau này mà không ai nhớ sửa danh sách sẽ lọt vào phạm vi
/// `write`. Cách sửa đúng là một trường trong `ToolMeta` để mỗi tool tự khai, mặc định
/// giả định xấu nhất giống `mutating`; xem báo cáo bàn giao.
///
/// `task` nằm trong danh sách vì một lý do khác và nó không phải nợ: hạn chế **không di
/// truyền xuống phạm vi con** (`ToolRegistry::permits` so khớp phạm vi đúng bằng nhau),
/// nên một agent con do `task` sinh ra chạy với bộ tool đầy đủ. Để `task` lọt vào `write`
/// là để nguyên một cửa sau đi thẳng tới `bash`.
pub const TOOL_THI_HANH: &[&str] = &[
    "bash",
    "job_kill",
    "task",
    "terminal_open",
    "terminal_send",
    "terminal_signal",
    "terminal_close",
];

/// Hạn chế tương ứng với một phạm vi. `None` nghĩa là không siết gì.
///
/// `at` là phạm vi sắp bị siết: tập tool đọc ra từ đó, chứ không phải từ một danh sách
/// tên viết cứng — một tool mới cắm vào sau này phải tự rơi đúng nhóm của nó.
pub fn han_che(
    registry: &ToolRegistry,
    at: Option<ScopeKey>,
    scope: ToolScope,
) -> Option<ToolRestriction> {
    match scope {
        // Không hạn chế nào cả. Đây là chỗ duy nhất trong tệp không siết gì, và người
        // dùng phải chọn nó một cách tường minh.
        ToolScope::Shell => None,
        // Danh sách trắng dựng từ `ToolMeta::mutating`, thứ mặc định là `true`. Nghĩa là
        // một tool quên khai sẽ **rơi ra ngoài** phạm vi chỉ đọc — fail closed, đúng
        // chiều an toàn, và không cần ai bảo trì một danh sách tên.
        ToolScope::Read => Some(ToolRestriction::allow_only(
            registry
                .visible(at)
                .into_iter()
                .filter(|tool| !tool.meta().mutating)
                .map(|tool| tool.schema().name),
        )),
        // Danh sách đen, vì chiều còn lại không suy ra được từ `ToolMeta` — xem
        // [`TOOL_THI_HANH`].
        ToolScope::Write => Some(ToolRestriction::deny_only(TOOL_THI_HANH.iter().copied())),
    }
}

/// Câu nói cho **mô hình** biết nó đang bị siết, hoặc `None` khi không siết gì.
///
/// Quyết định là **nói ra**, và lý do là kiểu hỏng ở chiều ngược lại. Không nói thì tín
/// hiệu duy nhất mô hình nhận được là câu từ chối lúc gọi — "Tool `bash` không khả dụng
/// với agent này" — một câu đọc lên như một tính chất vĩnh viễn của agent. Mô hình đọc
/// nó rồi hoặc thử lại bằng một tool khác cho tới hết vòng, hoặc kết luận là mình không
/// bao giờ chạy được lệnh và **báo cáo như thể đã làm xong**. Cả hai đều tệ hơn một dòng
/// nói thẳng rằng đây là giới hạn của lượt này và người dùng nâng lên được.
///
/// Câu chỉ nói **mức**, không liệt kê tên tool bị giấu. Liệt kê là biến chỗ này thành
/// một máy dò: sổ đăng ký cố tình trả cùng một câu cho "bị cấm" và "không tồn tại" để
/// mô hình không đoán ra được bộ tool đang bị che, và một danh sách tên ở đây sẽ phá
/// đúng tính chất ấy — kể cả khi nó lỡ chứa tên tool MCP của người dùng.
pub fn loi_nhac(scope: ToolScope) -> Option<String> {
    match scope {
        ToolScope::Shell => None,
        ToolScope::Read => Some(
            "Người dùng đặt lượt này ở phạm vi **chỉ đọc**: chỉ những tool không thay đổi \
             gì mới được cắm. Tool sửa tệp và tool chạy lệnh không có ở lượt này — đừng \
             thử gọi chúng. Cần tới chúng thì mô tả việc phải làm và nói người dùng nâng \
             phạm vi lên, thay vì báo là đã làm xong."
                .into(),
        ),
        ToolScope::Write => Some(
            "Người dùng đặt lượt này ở phạm vi **đọc và ghi**: đọc và sửa tệp thì được, \
             chạy lệnh và giao việc cho agent con thì không. Cần chạy lệnh thì nói ra \
             lệnh cần chạy và để người dùng nâng phạm vi lên, thay vì báo là đã chạy."
                .into(),
        ),
    }
}

/// Nối [`loi_nhac`] vào message hệ thống của mọi request trong lượt.
///
/// Đi qua `agent/request` chứ không qua `SystemPrompt`: sổ prompt là **của cả ứng dụng**,
/// nên một khối cắm vào đó sẽ hiện luôn trong lượt của phiên khác đang chạy song song với
/// phạm vi khác. Middleware này gắn vào phạm vi của lượt, nên nó chỉ chạm đúng lượt ấy —
/// và nó **không** đi vào sổ phiên, nên lượt sau không thừa hưởng một câu nói về một hạn
/// chế đã hết hiệu lực.
struct NhacPhamVi(String);

impl Middleware<AgentRequest> for NhacPhamVi {
    fn call<'a>(
        &'a self,
        req: &'a mut ChatRequest,
        next: Next<'a, AgentRequest>,
    ) -> BoxFuture<'a, ChatRequest> {
        match req.messages.first_mut() {
            // Nối vào cuối khối hệ thống đã có: mô hình đọc luật của lượt ngay cạnh luật
            // chung, thay vì ở một message rời mà tầng nén ngữ cảnh có thể cắt đi.
            Some(Message::System { content }) => {
                content.push_str("\n\n");
                content.push_str(&self.0);
            }
            _ => req.messages.insert(0, Message::system(self.0.clone())),
        }
        next.run(req).boxed()
    }
}

/// Mở một phạm vi con cho đúng một lượt, đã cắm hạn chế tương ứng.
///
/// Người gọi **phải** `dispose()` cái scope hiệu ứng của ngữ cảnh trả về khi lượt xong;
/// đó chính là lúc quyền được trả lại. Giữ nó lâu hơn một lượt là biến một lựa chọn nhất
/// thời thành một thiết lập dính, mà một thiết lập dính là thứ người dùng quên mất mình
/// đã đặt.
pub fn mo_pham_vi(
    ctx: &Context,
    scope: ToolScope,
    approver: Arc<dyn pai_tools::Approver>,
) -> Result<Context, String> {
    // Lấy sổ đăng ký **trước** khi dựng scope: hỏng ở đây thì chưa có gì phải dọn.
    let registry: Arc<ToolRegistry> = ctx.require::<Tools>().map_err(|err| err.to_string())?;
    // `isolate` **trước** `scoped`: người duyệt của mỗi lượt phải nằm trong một cõi riêng.
    // `scoped` chỉ tạo một nhánh phạm vi mới nhưng giữ nguyên cõi, và hai provider cho
    // cùng một seam trong cùng một cõi là lỗi — nên hai lượt chạy song song ở hai phiên
    // sẽ va nhau ngay ở lượt thứ hai, và lượt ấy không mở được. Bài test bắt đúng chỗ này.
    let turn = ctx.isolate::<pai_tools::Approval>().scoped("luot");
    // Không dựng được phạm vi thì **không chạy**, thay vì chạy không hạn chế: một lượt
    // chạy đầy quyền sau khi người dùng vừa hạ quyền là đúng lời nói dối mà bộ chọn này
    // sinh ra để chấm dứt.
    let key = turn
        .scope_key()
        .ok_or("không dựng được phạm vi riêng cho lượt")?;
    if let Some(restriction) = han_che(&registry, Some(key), scope) {
        turn.keep(registry.restrict(key, restriction));
    }
    if let Some(text) = loi_nhac(scope) {
        turn.keep(turn.on_waterfall::<AgentRequest>(Arc::new(NhacPhamVi(text))));
    }
    // Người duyệt cắm vào **cõi riêng của lượt**, vì nó ôm `Channel` của chính lượt ấy.
    // Cắm ở gốc thì hai lượt song song tranh nhau một cửa sổ để hỏi — và câu hỏi của lượt
    // này sẽ hiện lên trong cửa sổ của lượt kia.
    turn.keep(
        turn.provide::<pai_tools::Approval>(approver)
            .map_err(|err| err.to_string())?,
    );
    Ok(turn)
}
