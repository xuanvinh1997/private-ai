//! Nguyên liệu cho gợi ý câu hỏi ở màn hình trống.
//!
//! # Vì sao phải hỏi lõi thay vì viết sẵn mấy câu
//!
//! Một gợi ý dựng sẵn chỉ có hai kết cục: hoặc nó chung chung tới mức không dạy được gì,
//! hoặc nó gọi tên một thứ **không có trong dự án của người dùng** — và một nút bấm vào
//! là hỏng dạy người ta rằng cả ứng dụng chưa dùng được. Lõi là chỗ duy nhất biết trong
//! kho này thật sự có ký hiệu nào, thư mục nào, tài liệu nào, nên nó trả về đúng chỗ đó.
//!
//! # Vì sao không `sync()` trước khi đọc
//!
//! Lệnh này chạy mỗi lần mở một phiên trống, tức là rất thường xuyên và luôn nằm chắn
//! trước mặt người dùng. Một lượt đồng bộ chỉ mục ở đây là vài giây đứng hình để đổi lấy
//! mấy con chip. Ta đọc **những gì chỉ mục đã có**; chưa quét lần nào thì trả về rỗng và
//! giao diện lùi về bộ tĩnh — chậm một nhịp còn hơn khựng một nhịp.
//!
//! # Vì sao rỗng chứ không lỗi
//!
//! Chưa mở dự án, chỉ mục trống, thư viện chưa có tài liệu — cả ba đều là trạng thái hợp
//! lệ mà người dùng gặp trong năm phút đầu tiên. Trả `Err` ở đây là dựng một hộp thoại
//! lỗi lên màn hình chào mừng.

use pai_index::Index;
use pai_rag::Docs;
use tauri::State;

use crate::AppState;
use crate::protocol::PromptSeeds;

/// Trần cho mỗi loại nguyên liệu.
///
/// Giao diện hiện tối đa năm con chip và luôn giữ vài câu tĩnh trong đó, nên xin nhiều
/// hơn chỗ này là tải thêm dữ liệu để rồi vứt đi.
const MAX_SEEDS: usize = 3;

/// Tên dài hơn chỗ nhìn thấy trong một con chip.
///
/// Cắt ở **lõi** chứ không ở giao diện: một tiêu đề PDF dài trăm ký tự đi qua wire rồi
/// mới bị `text-overflow` nuốt vẫn là một con chip mà người dùng không đọc nổi để quyết
/// định có bấm hay không. Cắt theo ký tự Unicode, không theo byte — tiếng Việt có dấu.
const MAX_LEN: usize = 48;

fn short(text: &str) -> String {
    let text = text.trim();
    if text.chars().count() <= MAX_LEN {
        return text.to_string();
    }
    let cut: String = text.chars().take(MAX_LEN - 1).collect();
    format!("{}…", cut.trim_end())
}

#[tauri::command]
pub async fn prompt_seeds(state: State<'_, AppState>) -> Result<PromptSeeds, String> {
    let harness = state.harness().await?;

    if let Some(index) = harness.ctx.get::<Index>() {
        // Chỉ mục hỏng không phải lý do để màn hình chào mừng hiện lỗi: bộ tĩnh vẫn dùng
        // được, và người dùng sẽ gặp cùng lỗi ấy ở chỗ nó thật sự cản trở họ.
        let Ok(stats) = index.stats().await else {
            return Ok(PromptSeeds::default());
        };
        if stats.files == 0 {
            return Ok(PromptSeeds::default());
        }
        let Ok(map) = index.overview().await else {
            return Ok(PromptSeeds::default());
        };
        return Ok(PromptSeeds {
            symbols: map
                .central
                .iter()
                .take(MAX_SEEDS)
                .map(|central| short(&central.node.name))
                .collect(),
            directories: map
                .directories
                .iter()
                .take(MAX_SEEDS)
                .map(|folder| short(&folder.path))
                .collect(),
            documents: Vec::new(),
        });
    }

    if let Some(docs) = harness.ctx.get::<Docs>() {
        let Ok(documents) = docs.documents().await else {
            return Ok(PromptSeeds::default());
        };
        return Ok(PromptSeeds {
            symbols: Vec::new(),
            directories: Vec::new(),
            documents: documents
                .into_iter()
                .take(MAX_SEEDS)
                .map(|doc| short(&doc.title))
                .collect(),
        });
    }

    Ok(PromptSeeds::default())
}

#[cfg(test)]
mod tests {
    use super::short;

    #[test]
    fn giu_nguyen_ten_ngan() {
        assert_eq!(short("CentralSymbol"), "CentralSymbol");
    }

    #[test]
    fn cat_ten_dai_va_them_dau_ba_cham() {
        let long = "a".repeat(80);
        let cut = short(&long);
        assert_eq!(cut.chars().count(), super::MAX_LEN);
        assert!(cut.ends_with('…'));
    }

    /// Cắt theo ký tự chứ không theo byte — mỗi chữ có dấu là nhiều byte, và cắt giữa
    /// một ký tự là một panic trong `String`.
    #[test]
    fn cat_theo_ky_tu_khong_theo_byte() {
        let long = "đường".repeat(30);
        assert_eq!(short(&long).chars().count(), super::MAX_LEN);
    }

    #[test]
    fn bo_khoang_trang_thua() {
        assert_eq!(short("  Hợp đồng thuê nhà  "), "Hợp đồng thuê nhà");
    }
}
