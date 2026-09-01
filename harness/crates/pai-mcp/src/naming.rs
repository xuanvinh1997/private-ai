//! Tiền tố `ext.<server>.` — một phép chiếu, và cả hai chiều của nó.
//!
//! Đây là bất biến trung tâm của nửa client. Nói cho gọn:
//!
//! > Tên mà một server bên thứ ba công bố **không bao giờ** được nhìn thấy ở dạng trần.
//! > Nó được đặt tiền tố ngay tại chỗ đọc về, và chỉ được cắt tiền tố ngay tại chỗ gửi đi.
//!
//! Hai lý do, và lý do thứ hai mới là lý do thật:
//!
//! 1. **Không trùng tên.** Hai server cùng có `search` thì vẫn là hai tool khác nhau.
//! 2. **Không giả dạng.** Nếu tên đi thẳng vào sổ đăng ký thì một server bên thứ ba đăng
//!    ký một tool tên `read` sẽ **che** `read` của `pai-fs` — sổ đăng ký cho đăng ký sau
//!    thắng đăng ký trước. Mô hình gọi `read`, chạm vào tool của người lạ, và không có
//!    dòng nào ở đâu nói rằng chuyện đó vừa xảy ra. Tiền tố làm cho việc đó bất khả thi
//!    **về mặt cấu trúc** chứ không phải nhờ một lần kiểm tra ai đó phải nhớ viết.
//!
//! Việc cắt dùng [`str::strip_prefix`], nghĩa là cắt **đúng một lần và đúng ở đầu**. Một
//! tool từ xa tên sẵn là `ext.other.thing` do đó thành `ext.srv.ext.other.thing` và cắt
//! ngược lại ra đúng `ext.other.thing` — cái tên nó tự khai, không phải một cái tên khác.

use pai_tools::ToolName;

/// Không gian tên của mọi thứ đến từ bên ngoài. Không tool nội bộ nào được bắt đầu bằng
/// chuỗi này; đó là điều kiện khiến bất biến ở trên đứng vững.
pub const EXTERNAL_PREFIX: &str = "ext";

/// `ext.<server>` — phần đầu chung của mọi tool từ một server.
pub fn namespace(server: &str) -> String {
    format!("{EXTERNAL_PREFIX}.{server}")
}

/// Đặt tiền tố. Gọi ở đúng một chỗ: lúc đọc danh sách tool về.
pub fn qualify(server: &str, remote: &str) -> ToolName {
    ToolName::new(format!("{EXTERNAL_PREFIX}.{server}.{remote}"))
}

/// Cắt tiền tố. Gọi ở đúng một chỗ: lúc chuyển tiếp một lần gọi.
///
/// `None` nghĩa là cái tên này không thuộc server đó — và đó là một lỗi lập trình, không
/// phải một trường hợp cần xử lý mềm, nên chỗ gọi được phép coi nó là từ chối.
pub fn remote_of<'a>(server: &str, name: &'a ToolName) -> Option<&'a str> {
    name.as_str()
        .strip_prefix(&format!("{}.", namespace(server)))
}

/// Cái tên này có đến từ bên ngoài không.
///
/// Dùng cho chiều ngược lại: server của ta **không** phơi lại tool bên thứ ba ra ngoài.
pub fn is_external(name: &ToolName) -> bool {
    name.as_str().starts_with(&format!("{EXTERNAL_PREFIX}."))
}
