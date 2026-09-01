//! Tự trói mình rồi `exec`.
//!
//! Landlock giam **tiến trình gọi nó**, không giam một tiến trình khác. Không có API nào
//! kiểu "chạy lệnh này trong hộp" như `sandbox-exec` của macOS — nên vòng giam phải được
//! dựng trong chính tiến trình sắp trở thành cái lệnh đó, ở khoảnh khắc sau `fork` và
//! trước `exec`. Binary này *là* khoảnh khắc đó.
//!
//! Nó nhận chính sách trên dòng lệnh, áp lên bản thân, rồi thay mình bằng lệnh thật. Sau
//! `exec` nó không còn tồn tại; thứ còn lại là lệnh của người dùng, đã bị giam.
//!
//! Mã thoát khi **chưa** kịp `exec` cố ý nằm ngoài vùng lệnh thường dùng, để một lỗi ở
//! đây không bị đọc nhầm thành lỗi của lệnh: `2` là tham số hỏng, `3` là không giam được.

fn main() {
    #[cfg(target_os = "linux")]
    linux::main();

    #[cfg(not(target_os = "linux"))]
    {
        eprintln!("pai-landlock-run chỉ chạy trên Linux");
        std::process::exit(2);
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use std::os::unix::process::CommandExt;
    use std::process::Command;

    use landlock::{
        ABI, Access, AccessFs, CompatLevel, Compatible, Ruleset, RulesetAttr, RulesetCreatedAttr,
        RulesetStatus, path_beneath_rules,
    };

    /// ABI cao nhất crate biết. `BestEffort` tự hạ xuống theo kernel đang chạy, và
    /// `RulesetStatus` nói cho ta biết nó đã hạ tới đâu — đó là chỗ `Enforcement::Partial`
    /// đến từ, chứ không phải từ một phép đoán.
    const ABI_MONG_MUON: ABI = ABI::V5;

    pub fn main() {
        let mut writable: Vec<String> = Vec::new();
        let mut argv: Vec<String> = Vec::new();
        let mut sau_dau_gach = false;

        let mut args = std::env::args().skip(1);
        while let Some(arg) = args.next() {
            if sau_dau_gach {
                argv.push(arg);
            } else if arg == "--" {
                sau_dau_gach = true;
            } else if arg == "--allow-write" {
                match args.next() {
                    Some(path) => writable.push(path),
                    None => hong(2, "--allow-write thiếu đường dẫn"),
                }
            } else {
                hong(2, &format!("tham số lạ: {arg}"));
            }
        }
        if argv.is_empty() {
            hong(2, "không có lệnh nào để chạy");
        }

        let status = match dung_vong_giam(&writable) {
            Ok(status) => status,
            Err(err) => hong(3, &format!("không dựng được vòng giam: {err}")),
        };
        if matches!(status, RulesetStatus::NotEnforced) {
            // Không giam được thì **không chạy**. Chạy tiếp là để người gọi tin vào một
            // vòng vây không tồn tại, và đó là thứ nguy hiểm hơn hẳn việc không có sandbox.
            hong(3, "kernel không thi hành được Landlock");
        }

        let err = Command::new(&argv[0]).args(&argv[1..]).exec();
        // `exec` chỉ trả về khi nó hỏng.
        hong(3, &format!("không chạy được {}: {err}", argv[0]));
    }

    /// Đọc khắp nơi, ghi chỉ trong những gốc đã cho.
    ///
    /// Đọc để mở là có chủ ý và giống hệt bản macOS: một coding agent phải đọc được repo,
    /// toolchain, cache phụ thuộc và cấu hình git; đục đủ lỗ cho nó chạy thì ranh giới đọc
    /// chẳng còn nghĩa gì. Thứ chặn việc đọc bí mật là danh sách đường dẫn được bảo vệ của
    /// `pai-fs`, không phải chỗ này.
    fn dung_vong_giam(writable: &[String]) -> Result<RulesetStatus, Box<dyn std::error::Error>> {
        let mut ruleset = Ruleset::default()
            .set_compatibility(CompatLevel::BestEffort)
            .handle_access(AccessFs::from_all(ABI_MONG_MUON))?
            .create()?
            // Gốc `/` mở cho đọc. Không có luật này thì tiến trình không đọc nổi chính
            // binary nó sắp `exec`.
            .add_rules(path_beneath_rules(
                ["/"],
                AccessFs::from_read(ABI_MONG_MUON),
            ))?
            // Cống bắt buộc: gần như mọi lệnh đều mở `/dev/null` để vứt output đi, nên
            // không có nó thì `read-only` không chạy nổi một lệnh nào.
            //
            // Đây là chỗ Linux **rộng hơn** macOS, và con số này là đo chứ không đoán: một
            // luật `path_beneath` trỏ thẳng vào `/dev/null` không cấp được quyền ghi lên
            // nó với bất kỳ tổ hợp quyền nào — Landlock không quản device node theo từng
            // tệp. Một tập quyền hẹp trên thư mục `/dev` cũng không đủ. Hồ sơ SBPL của
            // macOS mở đúng một tệp; ở đây phải mở cả `/dev` như một gốc ghi được.
            //
            // Cái nó thật sự cho phép hẹp hơn nhiều so với vẻ ngoài: trên máy thật `/dev`
            // thuộc về root, nên một agent chạy dưới quyền người dùng chỉ ghi được vào
            // những device node vốn đã cho mọi người ghi — `/dev/null`, `/dev/zero`,
            // `/dev/tty`.
            .add_rules(path_beneath_rules(
                ["/dev"],
                AccessFs::from_all(ABI_MONG_MUON),
            ))?;

        for path in writable {
            // Gốc không tồn tại thì bỏ qua chứ không hỏng: `writable_roots` bên kia đã
            // lọc rồi, nhưng một thư mục có thể biến mất giữa lúc lọc và lúc chạy.
            if std::path::Path::new(path).exists() {
                ruleset = ruleset.add_rules(path_beneath_rules(
                    [path],
                    AccessFs::from_all(ABI_MONG_MUON),
                ))?;
            }
        }

        Ok(ruleset.restrict_self()?.ruleset)
    }

    fn hong(code: i32, message: &str) -> ! {
        eprintln!("pai-landlock-run: {message}");
        std::process::exit(code);
    }
}
