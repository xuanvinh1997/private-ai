import type { SessionSummary } from "./protocol";

/** Ngày giờ tương đối, đủ thô để không phải cập nhật mỗi giây. */
export function relativeTime(at: number, now = Date.now()): string {
  const minutes = Math.round((now - at) / 60_000);
  if (minutes < 1) return "vừa xong";
  if (minutes < 60) return `${minutes} phút trước`;
  const hours = Math.round(minutes / 60);
  if (hours < 24) return `${hours} giờ trước`;
  const days = Math.round(hours / 24);
  return days < 30 ? `${days} ngày trước` : new Date(at).toLocaleDateString("vi-VN");
}

/** Giờ trong ngày cho hàng tiêu đề của một tin nhắn. */
export function clockTime(at: number): string {
  return new Date(at).toLocaleTimeString("vi-VN", { hour: "2-digit", minute: "2-digit" });
}

export interface SessionGroup {
  id: "today" | "week" | "older";
  label: string;
  sessions: SessionSummary[];
}

const DAY = 24 * 60 * 60 * 1000;

/**
 * Nhóm phiên theo thời gian.
 *
 * Mốc "hôm nay" tính từ **nửa đêm địa phương**, không phải "trong 24 giờ qua": một phiên
 * lúc 23h hôm qua không phải là "hôm nay", dù nó mới hơn một phiên lúc 1h sáng nay.
 * Nhóm rỗng bị loại luôn — một tiêu đề không có gì bên dưới chỉ làm danh sách dài ra.
 */
export function groupSessions(sessions: SessionSummary[], now = Date.now()): SessionGroup[] {
  const midnight = new Date(now);
  midnight.setHours(0, 0, 0, 0);
  const startOfToday = midnight.getTime();
  const startOfWeek = startOfToday - 6 * DAY;

  const buckets: Record<SessionGroup["id"], SessionSummary[]> = { today: [], week: [], older: [] };
  for (const session of [...sessions].sort((a, b) => b.updatedAt - a.updatedAt)) {
    if (session.updatedAt >= startOfToday) buckets.today.push(session);
    else if (session.updatedAt >= startOfWeek) buckets.week.push(session);
    else buckets.older.push(session);
  }

  return (
    [
      { id: "today", label: "Hôm nay", sessions: buckets.today },
      { id: "week", label: "7 ngày qua", sessions: buckets.week },
      { id: "older", label: "Cũ hơn", sessions: buckets.older },
    ] satisfies SessionGroup[]
  ).filter((group) => group.sessions.length > 0);
}
