import { describe, expect, it } from "vitest";

import { foldDiacritics, groupSessions, rankSessions, titleFromMessage } from "./sessions";
import type { SessionSummary } from "./protocol";

const NOW = 1_700_000_000_000;
const session = (id: string, title: string, updatedAt: number): SessionSummary =>
  ({ id, title, updatedAt }) as SessionSummary;

const ids = (list: SessionSummary[]) => list.map((entry) => entry.id);

describe("foldDiacritics", () => {
  it("bỏ dấu thanh và dấu mũ", () => {
    expect(foldDiacritics("Sửa tài liệu")).toBe("sua tai lieu");
  });

  // `đ` không phải `d` cộng dấu tổ hợp, nên `NFD` không tách nó — đây là ca dễ sót nhất.
  it("đổi đ thành d, cả hoa lẫn thường", () => {
    expect(foldDiacritics("Đọc Đề")).toBe("doc de");
  });

  it("để nguyên chuỗi vốn không dấu", () => {
    expect(foldDiacritics("Fix Auth")).toBe("fix auth");
  });
});

describe("rankSessions", () => {
  const sessions = [
    session("a", "Sửa authentication cho API", NOW - 1_000),
    session("b", "Fix authentication bug", NOW - 2_000),
    session("c", "Bỏ hết unwrap trong pai-core", NOW - 3_000),
    session("d", "Đọc tài liệu thiết kế", NOW - 4_000),
    session("e", "authentication", NOW - 5_000),
  ];

  // Lý do cả hàm này tồn tại: gõ không dấu là cách người ta thật sự lọc danh sách.
  it("truy vấn không dấu tìm ra tiêu đề có dấu", () => {
    expect(ids(rankSessions(sessions, "sua"))).toEqual(["a"]);
    expect(ids(rankSessions(sessions, "doc"))).toEqual(["d"]);
  });

  it("mọi token phải khớp, và thứ tự token không quan trọng", () => {
    expect(ids(rankSessions(sessions, "auth sua"))).toEqual(["a"]);
    expect(ids(rankSessions(sessions, "sua auth"))).toEqual(["a"]);
  });

  it("không khớp thì trả về rỗng, không nới lỏng để vớt vát", () => {
    expect(ids(rankSessions(sessions, "authx"))).toEqual([]);
  });

  // Khớp từ đầu tiêu đề ("e") đứng trên khớp đầu một từ ("a", "b"); "a" trên "b" vì mới hơn.
  it("khớp đầu tiêu đề đứng trước, rồi tới phiên mới hơn", () => {
    expect(ids(rankSessions(sessions, "authentication"))).toEqual(["e", "a", "b"]);
  });

  it("truy vấn rỗng trả về cả danh sách, mới nhất trước", () => {
    expect(ids(rankSessions(sessions, ""))).toEqual(["a", "b", "c", "d", "e"]);
    expect(ids(rankSessions(sessions, "   "))).toEqual(["a", "b", "c", "d", "e"]);
  });

  it("không sửa mảng gốc", () => {
    const before = ids(sessions);
    rankSessions(sessions, "");
    expect(ids(sessions)).toEqual(before);
  });
});

describe("titleFromMessage", () => {
  // Cắt thô ở đúng 24 ký tự cho ra "…trong bộ n…"; ranh giới từ cho ra "…trong bộ…".
  it("cắt ở ranh giới từ chứ không giữa từ", () => {
    expect(titleFromMessage("Bỏ hết unwrap trong bộ nạp cấu hình của pai-core", 24)).toBe(
      "Bỏ hết unwrap trong bộ…",
    );
  });

  it("để nguyên câu đủ ngắn", () => {
    expect(titleFromMessage("Chạy test")).toBe("Chạy test");
  });

  it("chỉ lấy dòng đầu", () => {
    expect(titleFromMessage("Dòng đầu\nDòng sau")).toBe("Dòng đầu");
  });
});

describe("groupSessions", () => {
  // Mốc "hôm nay" là nửa đêm địa phương, không phải "trong 24 giờ qua".
  it("một phiên 23h hôm qua không phải hôm nay, dù mới hơn 1h sáng nay", () => {
    const now = new Date(2024, 0, 15, 9, 0, 0).getTime();
    const lateYesterday = new Date(2024, 0, 14, 23, 0, 0).getTime();
    const earlyToday = new Date(2024, 0, 15, 1, 0, 0).getTime();

    const groups = groupSessions(
      [session("yesterday", "Cũ", lateYesterday), session("today", "Mới", earlyToday)],
      now,
    );

    expect(groups.find((group) => group.id === "today")?.sessions.map((s) => s.id)).toEqual(["today"]);
    expect(groups.find((group) => group.id === "week")?.sessions.map((s) => s.id)).toEqual(["yesterday"]);
  });

  it("nhóm rỗng bị loại hẳn", () => {
    const now = new Date(2024, 0, 15, 9, 0, 0).getTime();
    const groups = groupSessions([session("x", "Hôm nay", now - 60_000)], now);
    expect(groups.map((group) => group.id)).toEqual(["today"]);
  });
});
