//! `time.now` — the clock, and nothing else.
//! Replaces the `mcp-server-time` subprocess, whose two tools (`get_current_time`,
//! `convert_time`) are one question here: "what does this moment read as, over there?".
//! One tool because the model otherwise has to pick between them, and picking wrong costs a
//! round trip; the two shapes differ only by whether `time` is given.
//!
//! Three rules hold the design together:
//!   * every rendered instant carries its UTC offset, because the model does arithmetic on
//!     what it reads, and "14:00" alone is not a moment;
//!   * a wall-clock reading that does not exist, or exists twice, is named as such instead of
//!     being silently rounded — the two hours a year when clocks jump are exactly the hours a
//!     scheduling mistake is made;
//!   * the answer is a handful of lines by construction, so it needs no overflow budget.

use async_trait::async_trait;
use chrono::{DateTime, LocalResult, NaiveDateTime, NaiveTime, TimeZone, Utc};
use chrono_tz::Tz;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

use crate::schema::{ToolMeta, ToolSchema, json_schema_for};
use crate::tool::{Invocation, Tool, ToolError, ToolOutcome};

/// Accepted spellings of a full moment, tried in order. Deliberately short: seconds optional,
/// `T` or a space between date and time, and nothing else — a parser that accepts everything
/// also accepts `03/04/2026`, which is two different days depending on who wrote it.
const FORMATS: &[&str] = &[
    "%Y-%m-%dT%H:%M:%S",
    "%Y-%m-%d %H:%M:%S",
    "%Y-%m-%dT%H:%M",
    "%Y-%m-%d %H:%M",
];

/// Time-only spellings, resolved against today's date in the source zone. Kept because
/// `convert_time` took exactly this shape ("what is 15:30 there?") and it is the common case.
const CLOCK_FORMATS: &[&str] = &["%H:%M:%S", "%H:%M"];

/// Longest argument fragment quoted back inside an error. An unusable argument may be
/// megabytes long; the complaint about it must not be.
const MAX_ECHO: usize = 64;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct TimeNowArgs {
    /// Múi giờ muốn xem kết quả, viết theo tên IANA như `Asia/Ho_Chi_Minh`, `Europe/Paris`,
    /// `UTC`. Bỏ trống thì dùng múi giờ của máy đang chạy.
    pub timezone: Option<String>,
    /// Mốc giờ cần quy đổi, dạng `YYYY-MM-DD HH:MM[:SS]` (hoặc dùng `T` thay dấu cách), chỉ
    /// `HH:MM` để hiểu là hôm nay, hoặc ISO 8601 kèm offset như `2026-09-05T14:30:00+07:00`.
    /// Bỏ trống thì lấy đúng thời điểm hiện tại.
    pub time: Option<String>,
    /// Mốc ở `time` đang được viết theo múi giờ nào. Bỏ trống thì hiểu là múi giờ của máy.
    /// Không có `time`, hoặc `time` đã kèm sẵn offset, thì trường này vô nghĩa.
    pub source_timezone: Option<String>,
}

/// The clock tool. Holds only the fallback zone, so it is trivially shareable across calls.
pub struct TimeNow {
    /// Zone used whenever the caller names none.
    ///
    /// Resolved once at construction from the operating system, then cached: the machine's zone
    /// does not change mid-session, and re-reading it per call would turn a pure computation
    /// into a syscall on the hot path. UTC is the fallback when the OS will not say, because a
    /// wrong-but-labelled answer beats a failed one — and the label is always printed, so the
    /// model can see it guessed.
    ///
    /// Local, not UTC, as the default: "what time is it" asked by a person in Ho Chi Minh City
    /// answered in UTC is wrong seven hours out of every seven, and the model has no other way
    /// to learn where it is running.
    local: Tz,
}

impl Default for TimeNow {
    fn default() -> TimeNow {
        TimeNow::new()
    }
}

impl TimeNow {
    pub const NAME: &'static str = "time.now";

    pub fn new() -> TimeNow {
        TimeNow {
            local: detect_local(),
        }
    }

    /// A fixed fallback zone; the seam tests use to stop depending on the machine they run on.
    pub fn with_default_zone(zone: Tz) -> TimeNow {
        TimeNow { local: zone }
    }

    /// Which zone answers when the caller names none.
    pub fn default_zone(&self) -> Tz {
        self.local
    }

    /// `named` is already normalised by [`given`], so an empty string never reaches the parser.
    fn zone(&self, named: Option<&str>) -> Result<Tz, ToolError> {
        let Some(name) = named else {
            return Ok(self.local);
        };
        name.parse::<Tz>().map_err(|_| {
            ToolError::Invalid(format!(
                "không có múi giờ `{}`. Hãy dùng tên IANA đầy đủ, ví dụ `Asia/Ho_Chi_Minh`, \
                 `America/New_York`, `Europe/London` hay `UTC`.",
                echo(name)
            ))
        })
    }
}

/// The machine's IANA zone name, or UTC. `chrono` already depends on this lookup for
/// `Local`, so asking for the name directly costs nothing the build was not paying.
fn detect_local() -> Tz {
    iana_time_zone::get_timezone()
        .ok()
        .and_then(|name| name.parse::<Tz>().ok())
        .unwrap_or(Tz::UTC)
}

/// An optional argument that was actually given. Every field here says "bỏ trống thì …", and a
/// model reading that writes `""` about as often as it omits the key; treating the two the same
/// turns `{"timezone": ""}` from a rejected call into the answer it was asking for. Trimming
/// here also means the parser and the error echo see the same, already-clean text.
fn given(value: Option<&String>) -> Option<&str> {
    value
        .map(|text| text.trim())
        .filter(|text| !text.is_empty())
}

/// Quote a caller-supplied fragment safely: bounded, and cut on a character boundary since the
/// argument is arbitrary UTF-8 chosen by the model.
fn echo(value: &str) -> String {
    match value.char_indices().nth(MAX_ECHO) {
        Some((at, _)) => format!("{}…", &value[..at]),
        None => value.to_string(),
    }
}

/// A moment that carries its own offset already names an instant: no zone is needed to read it,
/// and neither DST arm can apply, because the ambiguity lives in wall-clock readings only.
///
/// Accepted because it is precisely what this tool prints. "Convert the timestamp you just gave
/// me" is the obvious follow-up call, and a tool that cannot read its own output back makes the
/// model reformat by hand — which is where it drops the offset and gets the answer wrong.
/// RFC 3339 and nothing wider: the narrow [`FORMATS`] list exists so that guessing never
/// happens, and every extra spelling here is another guess.
fn read_fixed_instant(text: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(text.trim())
        .ok()
        .map(|moment| moment.with_timezone(&Utc))
}

/// Read a wall-clock reading; the zone is needed even to parse, since `HH:MM` means "today"
/// and today depends on where you stand.
fn read_wall_clock(text: &str, zone: Tz) -> Option<NaiveDateTime> {
    let text = text.trim();
    for format in FORMATS {
        if let Ok(moment) = NaiveDateTime::parse_from_str(text, format) {
            return Some(moment);
        }
    }
    for format in CLOCK_FORMATS {
        if let Ok(clock) = NaiveTime::parse_from_str(text, format) {
            return Some(Utc::now().with_timezone(&zone).date_naive().and_time(clock));
        }
    }
    None
}

/// Pin a wall-clock reading to an actual instant. The interesting arm is the third one.
fn pin(
    moment: NaiveDateTime,
    zone: Tz,
    text: &str,
) -> Result<(DateTime<Tz>, Option<String>), ToolError> {
    match zone.from_local_datetime(&moment) {
        LocalResult::Single(pinned) => Ok((pinned, None)),
        // Autumn: the hour runs twice, so the reading names two instants. Answering with the
        // first is a choice, not a fact, and the note says so rather than hiding it.
        LocalResult::Ambiguous(first, second) => {
            let note = format!(
                "Lưu ý: `{}` ở {} xảy ra hai lần trong ngày lùi giờ (offset {} rồi {}). \
                 Kết quả dưới đây lấy lần thứ nhất.",
                echo(text),
                zone.name(),
                first.format("%:z"),
                second.format("%:z"),
            );
            Ok((first, Some(note)))
        }
        // Spring: the hour is skipped, so the reading names no instant at all. Rounding it to
        // the next valid minute would answer a question nobody asked.
        LocalResult::None => Err(ToolError::Invalid(format!(
            "`{}` không tồn tại ở múi giờ {}: đồng hồ nhảy qua đúng khoảng đó khi vào giờ mùa hè. \
             Hãy chọn một mốc khác.",
            echo(text),
            zone.name(),
        ))),
    }
}

/// ISO 8601 with an explicit offset — the one format that means the same thing to everyone.
fn iso(moment: &DateTime<Tz>) -> String {
    moment.format("%Y-%m-%dT%H:%M:%S%:z").to_string()
}

fn describe(moment: &DateTime<Tz>) -> String {
    format!("{} ({})", iso(moment), moment.timezone().name())
}

#[async_trait]
impl Tool for TimeNow {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            TimeNow::NAME,
            "Xem giờ hiện tại, hoặc quy đổi một mốc giờ giữa hai múi giờ. Tên múi giờ theo \
             chuẩn IANA (`Asia/Ho_Chi_Minh`, `America/New_York`, `UTC`). Kết quả luôn ở dạng \
             ISO 8601 kèm offset, nên dùng được ngay để so sánh và tính toán. Hãy gọi tool này \
             thay vì tự đoán giờ hiện tại.",
            json_schema_for::<TimeNowArgs>(),
        )
    }

    fn meta(&self) -> ToolMeta {
        // Reads a clock and a static zone table: nothing changes, nothing leaves the machine,
        // and two calls at once cannot interfere.
        ToolMeta::read_only().concurrency_safe(true)
    }

    async fn execute(&self, call: &Invocation) -> Result<ToolOutcome, ToolError> {
        let args: TimeNowArgs =
            serde_json::from_value(serde_json::Value::Object(call.arguments.clone()))
                .map_err(|err| ToolError::Invalid(err.to_string()))?;

        let target = self.zone(given(args.timezone.as_ref()))?;

        // No `time`: the plain clock reading. Nothing to convert, and no DST edge to hit,
        // because an instant that exists cannot be ambiguous.
        let Some(text) = given(args.time.as_ref()) else {
            let now = Utc::now().with_timezone(&target);
            let rendered = format!(
                "{}\nUTC: {} · Unix: {}",
                describe(&now),
                now.with_timezone(&Utc).format("%Y-%m-%dT%H:%M:%S+00:00"),
                now.timestamp(),
            );
            let structured = json!({
                "timezone": target.name(),
                "iso": iso(&now),
                "utc": now.with_timezone(&Utc).to_rfc3339(),
                "unix": now.timestamp(),
                "offset": now.format("%:z").to_string(),
                "ambiguous": false,
            });
            return Ok(ToolOutcome::ok(rendered).with_structured(structured));
        };

        let source = self.zone(given(args.source_timezone.as_ref()))?;
        let (pinned, note) = match read_fixed_instant(text) {
            // The offset in the text already pinned it, so `source_timezone` is no longer
            // "where this reading was taken" but only where to show it from.
            Some(instant) => (instant.with_timezone(&source), None),
            None => {
                let moment = read_wall_clock(text, source).ok_or_else(|| {
                    ToolError::Invalid(format!(
                        "không đọc được mốc giờ `{}`. Hãy viết dạng `2026-09-05 14:30` \
                         (giây là tuỳ chọn), chỉ `14:30` nếu là hôm nay, hoặc ISO 8601 kèm \
                         offset như `2026-09-05T14:30:00+07:00`.",
                        echo(text)
                    ))
                })?;
                pin(moment, source, text)?
            }
        };
        let converted = pinned.with_timezone(&target);

        let mut rendered = String::new();
        if let Some(note) = &note {
            rendered.push_str(note);
            rendered.push('\n');
        }
        rendered.push_str(&format!(
            "Nguồn: {}\nĐích:  {}\nUTC:   {} · Unix: {}",
            describe(&pinned),
            describe(&converted),
            pinned.with_timezone(&Utc).format("%Y-%m-%dT%H:%M:%S+00:00"),
            pinned.timestamp(),
        ));

        let structured = json!({
            "source": { "timezone": source.name(), "iso": iso(&pinned) },
            "timezone": target.name(),
            "iso": iso(&converted),
            "utc": pinned.with_timezone(&Utc).to_rfc3339(),
            "unix": pinned.timestamp(),
            "offset": converted.format("%:z").to_string(),
            "ambiguous": note.is_some(),
        });
        Ok(ToolOutcome::ok(rendered).with_structured(structured))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::name::ToolName;
    use serde_json::Value;

    /// Every test pins the fallback zone, or the suite would pass or fail by the machine's own clock settings.
    fn tool() -> TimeNow {
        TimeNow::with_default_zone(Tz::UTC)
    }

    fn call(args: Value) -> Invocation {
        Invocation::new(
            ToolName::new(TimeNow::NAME),
            "test",
            args.as_object().cloned().unwrap_or_default(),
        )
    }

    async fn run(args: Value) -> Result<ToolOutcome, ToolError> {
        tool().execute(&call(args)).await
    }

    fn field<'a>(outcome: &'a ToolOutcome, key: &str) -> &'a str {
        outcome
            .structured
            .as_ref()
            .and_then(|value| value.get(key))
            .and_then(Value::as_str)
            .unwrap_or_default()
    }

    #[tokio::test]
    async fn gio_hien_tai_doc_lai_duoc_bang_rfc3339() {
        let outcome = run(json!({ "timezone": "Asia/Ho_Chi_Minh" }))
            .await
            .expect("múi giờ hợp lệ");
        let iso = field(&outcome, "iso");
        let parsed = DateTime::parse_from_rfc3339(iso).expect("phải là ISO 8601 kèm offset");
        assert_eq!(parsed.offset().local_minus_utc(), 7 * 3600);
    }

    #[tokio::test]
    async fn khong_khai_mui_gio_thi_dung_mui_mac_dinh() {
        let outcome = run(json!({})).await.expect("không tham số vẫn chạy được");
        assert_eq!(field(&outcome, "timezone"), "UTC");
    }

    #[tokio::test]
    async fn doi_gio_giua_hai_mui() {
        let outcome = run(json!({
            "time": "2024-01-15 12:00",
            "source_timezone": "Asia/Ho_Chi_Minh",
            "timezone": "UTC",
        }))
        .await
        .expect("mốc hợp lệ");
        assert_eq!(field(&outcome, "iso"), "2024-01-15T05:00:00+00:00");
    }

    /// Round trip: the same instant, read from both ends, must agree.
    #[tokio::test]
    async fn doi_di_roi_doi_ve_thi_ra_dung_moc_cu() {
        let there = run(json!({
            "time": "2024-06-01 09:30",
            "source_timezone": "Europe/Paris",
            "timezone": "Asia/Tokyo",
        }))
        .await
        .expect("mốc hợp lệ");
        assert_eq!(field(&there, "iso"), "2024-06-01T16:30:00+09:00");

        let back = run(json!({
            "time": "2024-06-01 16:30",
            "source_timezone": "Asia/Tokyo",
            "timezone": "Europe/Paris",
        }))
        .await
        .expect("mốc hợp lệ");
        assert_eq!(field(&back, "iso"), "2024-06-01T09:30:00+02:00");
    }

    /// The same wall clock, six months apart, is not the same offset: this is the whole point of
    /// carrying a zone table instead of a number.
    #[tokio::test]
    async fn cung_gio_nhung_khac_mua_thi_khac_offset() {
        let winter = run(json!({
            "time": "2024-01-15 12:00",
            "source_timezone": "America/New_York",
            "timezone": "UTC",
        }))
        .await
        .expect("mốc hợp lệ");
        let summer = run(json!({
            "time": "2024-07-15 12:00",
            "source_timezone": "America/New_York",
            "timezone": "UTC",
        }))
        .await
        .expect("mốc hợp lệ");
        assert_eq!(field(&winter, "iso"), "2024-01-15T17:00:00+00:00");
        assert_eq!(field(&summer, "iso"), "2024-07-15T16:00:00+00:00");
    }

    /// Spring forward in New York, 2024: 02:00 to 03:00 never happened.
    #[tokio::test]
    async fn moc_roi_vao_gio_bi_nhay_qua_thi_bao_loi() {
        let err = run(json!({
            "time": "2024-03-10 02:30",
            "source_timezone": "America/New_York",
            "timezone": "UTC",
        }))
        .await
        .expect_err("mốc này không tồn tại");
        assert!(matches!(err, ToolError::Invalid(_)), "phải là lỗi tham số");
        assert!(err.to_string().contains("không tồn tại"));
    }

    /// Autumn in New York, 2024: 01:30 happens twice, at -04:00 and again at -05:00.
    #[tokio::test]
    async fn moc_lap_lai_khi_lui_gio_thi_chon_lan_dau_va_noi_ro() {
        let outcome = run(json!({
            "time": "2024-11-03 01:30",
            "source_timezone": "America/New_York",
            "timezone": "UTC",
        }))
        .await
        .expect("mốc lặp lại vẫn trả về được");
        assert_eq!(field(&outcome, "iso"), "2024-11-03T05:30:00+00:00");
        assert!(
            outcome.content.contains("hai lần"),
            "phải nói rõ là mốc lặp"
        );
        assert_eq!(
            outcome
                .structured
                .as_ref()
                .and_then(|value| value.get("ambiguous")),
            Some(&Value::Bool(true))
        );
    }

    #[tokio::test]
    async fn mui_gio_khong_co_that_thi_bao_loi_chu_khong_im_lang_doi_sang_utc() {
        let err = run(json!({ "timezone": "Asia/Saigon_City" }))
            .await
            .expect_err("tên múi giờ này không có");
        assert!(matches!(err, ToolError::Invalid(_)));
        assert!(err.to_string().contains("Asia/Saigon_City"));
    }

    #[tokio::test]
    async fn moc_gio_viet_sai_dinh_dang_thi_bao_loi_kem_mau() {
        let err = run(json!({ "time": "03/04/2026" }))
            .await
            .expect_err("định dạng này mơ hồ nên bị từ chối");
        assert!(err.to_string().contains("2026-09-05 14:30"));
    }

    /// A hostile argument must not turn into a hostile-sized tool result.
    #[tokio::test]
    async fn thong_bao_loi_khong_chep_lai_ca_tham_so_dai() {
        let err = run(json!({ "timezone": "x".repeat(10_000) }))
            .await
            .expect_err("tên múi giờ rác");
        assert!(err.to_string().len() < 400, "lỗi phải ngắn");
        assert!(
            err.to_string().contains('…'),
            "phải cắt và đánh dấu là đã cắt"
        );
    }

    #[tokio::test]
    async fn chi_viet_gio_thi_hieu_la_hom_nay_o_mui_nguon() {
        let outcome = run(json!({
            "time": "15:30",
            "source_timezone": "Asia/Ho_Chi_Minh",
            "timezone": "Asia/Ho_Chi_Minh",
        }))
        .await
        .expect("chỉ có giờ vẫn đọc được");
        let today = Utc::now()
            .with_timezone(&chrono_tz::Asia::Ho_Chi_Minh)
            .format("%Y-%m-%d")
            .to_string();
        assert_eq!(field(&outcome, "iso"), format!("{today}T15:30:00+07:00"));
    }

    /// The tool prints ISO 8601 with an offset; feeding that straight back is the obvious next
    /// call, so it has to parse — and the offset in the text must beat `source_timezone`.
    #[tokio::test]
    async fn doc_lai_duoc_chinh_dinh_dang_minh_in_ra() {
        let outcome = run(json!({
            "time": "2024-01-15T05:00:00+00:00",
            "source_timezone": "America/New_York",
            "timezone": "Asia/Ho_Chi_Minh",
        }))
        .await
        .expect("ISO kèm offset phải đọc được");
        assert_eq!(field(&outcome, "iso"), "2024-01-15T12:00:00+07:00");
    }

    /// `Z` is the same instant as `+00:00`, and models write both.
    #[tokio::test]
    async fn hau_to_z_hieu_la_utc() {
        let outcome =
            run(json!({ "time": "2024-07-15T16:00:00Z", "timezone": "America/New_York" }))
                .await
                .expect("hậu tố Z phải đọc được");
        assert_eq!(field(&outcome, "iso"), "2024-07-15T12:00:00-04:00");
    }

    /// Every field's description says "bỏ trống thì …", and models take that literally by
    /// sending `""`. An empty string must mean the same thing as an absent key, not an error.
    #[tokio::test]
    async fn truong_de_trong_thi_hieu_nhu_khong_khai() {
        let outcome = run(json!({ "timezone": "", "time": "  ", "source_timezone": "" }))
            .await
            .expect("chuỗi rỗng phải hiểu là bỏ trống");
        assert_eq!(field(&outcome, "timezone"), "UTC");
        assert!(
            outcome
                .structured
                .as_ref()
                .and_then(|v| v.get("source"))
                .is_none(),
            "không có `time` thì không có phần nguồn: {}",
            outcome.content
        );
    }

    /// Whitespace around a zone name is the model's formatting, not a different zone.
    #[tokio::test]
    async fn ten_mui_gio_thua_khoang_trang_van_doc_duoc() {
        let outcome = run(json!({ "timezone": " Asia/Tokyo " }))
            .await
            .expect("khoảng trắng thừa không phải lỗi");
        assert_eq!(field(&outcome, "timezone"), "Asia/Tokyo");
    }

    #[tokio::test]
    async fn tool_khong_ghi_gi_va_chay_song_song_duoc() {
        let meta = tool().meta();
        assert!(!meta.mutating);
        assert!(!meta.leaves_device);
        assert!(meta.concurrency_safe);
    }
}
