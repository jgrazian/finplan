/**
 * A session's `User-Agent`, as something a person recognizes.
 *
 * The server stores the header verbatim and never interprets it, so the guess
 * lives here where being wrong is cosmetic. When nothing matches, the raw
 * string is shown rather than "Unknown device" — a name you do not recognize
 * is the point of the list.
 */
const BROWSERS: Array<[RegExp, string]> = [
  [/\bEdg\//, "Edge"],
  [/\bOPR\/|\bOpera\b/, "Opera"],
  [/\bFirefox\//, "Firefox"],
  [/\bChrome\//, "Chrome"],
  [/\bSafari\//, "Safari"],
  [/\bcurl\//, "curl"],
];

const PLATFORMS: Array<[RegExp, string]> = [
  [/\biPhone\b/, "iPhone"],
  [/\biPad\b/, "iPad"],
  [/\bAndroid\b/, "Android"],
  [/\bMac OS X\b|\bMacintosh\b/, "macOS"],
  [/\bWindows\b/, "Windows"],
  [/\bCrOS\b/, "ChromeOS"],
  [/\bLinux\b/, "Linux"],
];

export function deviceLabel(userAgent: string | null): string {
  if (!userAgent) return "Unknown device";

  // A non-browser client names itself first, e.g. `finplan-cli/0.6.2`.
  if (!/Mozilla\//.test(userAgent)) return userAgent.split(/\s+/)[0];

  const browser = BROWSERS.find(([pattern]) => pattern.test(userAgent))?.[1];
  const platform = PLATFORMS.find(([pattern]) => pattern.test(userAgent))?.[1];
  if (!browser && !platform) return userAgent;
  return [browser, platform].filter(Boolean).join(" · ");
}

/**
 * A UTC timestamp from the API as "3d ago" / "2h ago" / "now".
 *
 * SQLite's `datetime('now')` has no zone marker, so one is added rather than
 * letting the browser read it as local time and report a session in the future.
 */
export function timeAgo(sqlTimestamp: string, now = Date.now()): string {
  const at = Date.parse(sqlTimestamp.replace(" ", "T") + "Z");
  if (Number.isNaN(at)) return sqlTimestamp;

  const seconds = Math.max(0, Math.round((now - at) / 1000));
  if (seconds < 90) return "now";
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.round(minutes / 60);
  if (hours < 48) return `${hours}h ago`;
  return `${Math.round(hours / 24)}d ago`;
}

/** The date part of an API timestamp, which is already ISO-ordered. */
export function isoDate(sqlTimestamp: string): string {
  return sqlTimestamp.slice(0, 10);
}
