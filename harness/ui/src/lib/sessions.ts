import { S, locale, t } from "./i18n";
import type { SessionSummary } from "./protocol";

/** Language tag for `toLocaleDateString`: dates must follow the UI language or "15/01/2024" is ambiguous. */
const dateLocale = (): string => (locale() === "vi" ? "vi-VN" : "en-US");

/** Relative time, coarse enough that it need not refresh every second. */
export function relativeTime(at: number, now = Date.now()): string {
  const minutes = Math.round((now - at) / 60_000);
  if (minutes < 1) return t(S.libs.time.justNow);
  if (minutes < 60) return t(S.libs.time.minutes, { n: minutes });
  const hours = Math.round(minutes / 60);
  if (hours < 24) return t(S.libs.time.hours, { n: hours });
  const days = Math.round(hours / 24);
  return days < 30 ? t(S.libs.time.days, { n: days }) : new Date(at).toLocaleDateString(dateLocale());
}

/** Time of day for a message header row. */
export function clockTime(at: number): string {
  return new Date(at).toLocaleTimeString(dateLocale(), { hour: "2-digit", minute: "2-digit" });
}

/** Title derived from the *first* user message, truncated at a word boundary so names stay readable at a glance. */
export function titleFromMessage(text: string, max = 48): string {
  const line = text.trim().split("\n")[0]?.replace(/\s+/g, " ").trim() ?? "";
  if (line === "") return "";
  if (line.length <= max) return line;
  const cut = line.slice(0, max);
  const space = cut.lastIndexOf(" ");
  return `${(space > max / 2 ? cut.slice(0, space) : cut).trimEnd()}…`;
}

export interface SessionGroup {
  id: "today" | "week" | "older";
  label: string;
  sessions: SessionSummary[];
}

const DAY = 24 * 60 * 60 * 1000;

/** Group sessions by time from local midnight, not a rolling 24 hours; empty groups are dropped. */
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
      { id: "today", label: t(S.libs.sessionGroup.today), sessions: buckets.today },
      { id: "week", label: t(S.libs.sessionGroup.week), sessions: buckets.week },
      { id: "older", label: t(S.libs.sessionGroup.older), sessions: buckets.older },
    ] satisfies SessionGroup[]
  ).filter((group) => group.sessions.length > 0);
}

/** Strip Vietnamese diacritics for matching, since people filter unaccented; `NFD` misses d-stroke, so replace it. */
export function foldDiacritics(text: string): string {
  return text
    .normalize("NFD")
    .replace(/[\u0300-\u036f]/g, "")
    .toLowerCase()
    .replace(/đ/g, "d");
}

/** Score one token, or `null` when it does not match; only the ordering of the three tiers matters. */
function scoreToken(haystack: string, token: string): number | null {
  const at = haystack.indexOf(token);
  if (at < 0) return null;
  if (at === 0) return 3;
  return /\s/.test(haystack[at - 1] ?? "") ? 2 : 1;
}

/** Filter and rank sessions: every query token must match, so word order does not matter; an empty query lists all. */
export function rankSessions(sessions: SessionSummary[], query: string): SessionSummary[] {
  const tokens = foldDiacritics(query).trim().split(/\s+/).filter((token) => token !== "");
  if (tokens.length === 0) return [...sessions].sort((a, b) => b.updatedAt - a.updatedAt);

  const scored: { session: SessionSummary; score: number }[] = [];
  for (const session of sessions) {
    const haystack = foldDiacritics(session.title);
    let total = 0;
    for (const token of tokens) {
      const score = scoreToken(haystack, token);
      if (score === null) {
        total = -1;
        break;
      }
      total += score;
    }
    if (total >= 0) scored.push({ session, score: total });
  }

  // On a tie the newer session wins: with equal titles, the one just touched is almost always the target.
  return scored
    .sort((a, b) => b.score - a.score || b.session.updatedAt - a.session.updatedAt)
    .map((entry) => entry.session);
}
