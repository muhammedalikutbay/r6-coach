// All date/time formatting lives here. The locale is fixed to en-GB so the UI stays English
// even when the system locale is not (the target PC is Turkish).
const LOCALE = 'en-GB';

const toDate = (iso: string) => new Date(iso);
const valid = (d: Date) => !Number.isNaN(d.getTime());

/** Local calendar-day key, used to group matches by day. */
export function dayKey(iso: string): string {
  const d = toDate(iso);
  return valid(d) ? `${d.getFullYear()}-${d.getMonth()}-${d.getDate()}` : iso;
}

/** "Friday 31 October" */
export function formatDayHeader(iso: string): string {
  const d = toDate(iso);
  return valid(d) ? d.toLocaleDateString(LOCALE, { weekday: 'long', day: 'numeric', month: 'long' }) : iso;
}

/** "14:05" (24-hour) */
export function formatTime(iso: string): string {
  const d = toDate(iso);
  return valid(d) ? d.toLocaleTimeString(LOCALE, { hour: '2-digit', minute: '2-digit', hour12: false }) : '';
}

/** "31 Oct" */
export function formatShortDate(iso: string): string {
  const d = toDate(iso);
  return valid(d) ? d.toLocaleDateString(LOCALE, { day: 'numeric', month: 'short' }) : iso;
}

/** "31 Oct 2026, 14:05" */
export function formatDateTime(iso: string): string {
  const d = toDate(iso);
  return valid(d)
    ? d.toLocaleString(LOCALE, { day: 'numeric', month: 'short', year: 'numeric', hour: '2-digit', minute: '2-digit', hour12: false })
    : iso;
}
