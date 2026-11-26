import { DASHBOARD_VERSION } from './version';

/**
 * Release update check for the dashboard.
 *
 * Once per hour (at most) the browser asks GitHub for the repository's latest
 * published release and compares its tag with the running version. The check
 * is best-effort: every failure path — offline, rate limited, private repo,
 * malformed payload, timeout — returns null and the UI simply stays silent.
 * The stored state holds release metadata only, never credentials.
 */

/** Published releases of the dashboard's own repository. */
export const RELEASE_API_URL = 'https://api.github.com/repos/nghiaomg/ExoRoute/releases/latest';
export const RELEASES_PAGE_URL = 'https://github.com/nghiaomg/ExoRoute/releases';

/** At most one request per hour, shared across reloads via localStorage. */
const CHECK_INTERVAL_MS = 60 * 60 * 1000;
const FETCH_TIMEOUT_MS = 8_000;

const CHECK_STORAGE_KEY = 'exoroute.release-check';
const DISMISSED_STORAGE_KEY = 'exoroute.dismissed-release';

export interface ReleaseUpdate {
  /** Latest release version without the leading `v`, e.g. `0.1.3`. */
  latest: string;
  /** Release page on GitHub, opened in a new tab by the notice. */
  url: string;
}

interface StoredCheck {
  checkedAt: number;
  latest: string | null;
  url: string | null;
}

/**
 * Parses a release tag into its numeric parts. Accepts the `v` prefix GitHub
 * adds; anything else (build suffixes, garbage, prototype shapes) is not a
 * comparable release and returns null.
 */
export function parseVersionTag(tag: string): readonly [number, number, number] | null {
  if (typeof tag !== 'string') return null;
  const match = /^v?(\d+)\.(\d+)\.(\d+)$/.exec(tag.trim());
  if (!match) return null;
  return [Number(match[1]), Number(match[2]), Number(match[3])];
}

/** True when `latestTag` is a strictly newer release than `currentVersion`. */
export function isNewerRelease(currentVersion: string, latestTag: string): boolean {
  const current = parseVersionTag(currentVersion);
  const latest = parseVersionTag(latestTag);
  if (!current || !latest) return false;
  for (let i = 0; i < current.length; i += 1) {
    if (latest[i] !== current[i]) return latest[i] > current[i];
  }
  return false;
}

function readStoredJson(key: string): unknown {
  try {
    const raw = localStorage.getItem(key);
    if (raw === null) return null;
    return JSON.parse(raw) as unknown;
  } catch {
    // Storage can be unavailable (privacy mode, quota) or hold foreign data;
    // the notice is best-effort, so an unreadable store means "no state".
    return null;
  }
}

function writeStoredJson(key: string, value: unknown): void {
  try {
    localStorage.setItem(key, JSON.stringify(value));
  } catch {
    // Same best-effort contract as readStoredJson.
  }
}

export function getDismissedRelease(): string | null {
  const value = readStoredJson(DISMISSED_STORAGE_KEY);
  return typeof value === 'string' && parseVersionTag(value) !== null ? value : null;
}

/** Remembers which version's notice the user dismissed; a newer release re-notifies. */
export function dismissRelease(version: string): void {
  if (parseVersionTag(version) === null) return;
  writeStoredJson(DISMISSED_STORAGE_KEY, version);
}

function readStoredCheck(): StoredCheck | null {
  const value = readStoredJson(CHECK_STORAGE_KEY);
  if (typeof value !== 'object' || value === null) return null;
  const stored = value as Partial<StoredCheck>;
  if (typeof stored.checkedAt !== 'number' || !Number.isFinite(stored.checkedAt)) return null;
  if (stored.latest !== null && typeof stored.latest !== 'string') return null;
  if (stored.url !== null && typeof stored.url !== 'string') return null;
  return { checkedAt: stored.checkedAt, latest: stored.latest ?? null, url: stored.url ?? null };
}

export interface ReleaseCheckOptions {
  fetchImpl?: typeof fetch;
  now?: () => number;
  apiUrl?: string;
  timeoutMs?: number;
  currentVersion?: string;
  forceRefresh?: boolean;
}

/**
 * Fetches the repository's latest published release. The response is bounded
 * by the abort timeout and GitHub's own single-release payload; anything
 * unexpected (status, shape, tag format) yields null instead of throwing.
 */
export async function fetchLatestRelease(options: ReleaseCheckOptions = {}): Promise<ReleaseUpdate | null> {
  const fetchImpl = options.fetchImpl ?? fetch.bind(globalThis);
  const controller = new AbortController();
  const timeout = setTimeout(() => controller.abort(), options.timeoutMs ?? FETCH_TIMEOUT_MS);
  try {
    const response = await fetchImpl(options.apiUrl ?? RELEASE_API_URL, {
      headers: { Accept: 'application/vnd.github+json' },
      signal: controller.signal,
    });
    if (!response.ok) return null;
    const payload = (await response.json()) as { tag_name?: unknown; html_url?: unknown };
    if (typeof payload?.tag_name !== 'string' || typeof payload?.html_url !== 'string') return null;
    if (parseVersionTag(payload.tag_name) === null) return null;
    if (!payload.html_url.startsWith('https://github.com/')) return null;
    return { latest: payload.tag_name.replace(/^v/, ''), url: payload.html_url };
  } catch {
    // Network failure, abort, or malformed JSON — stay silent by contract.
    return null;
  } finally {
    clearTimeout(timeout);
  }
}

/**
 * Runs the release check and returns the update the user should be told
 * about, or null when the dashboard is current, the check failed, or the
 * user already dismissed that version. Results are reused for an hour so
 * reloads do not spend GitHub's unauthenticated rate limit.
 */
export async function checkForReleaseUpdate(options: ReleaseCheckOptions = {}): Promise<ReleaseUpdate | null> {
  const now = options.now ?? Date.now;
  const currentVersion = options.currentVersion ?? DASHBOARD_VERSION;

  let stored = readStoredCheck();
  if (options.forceRefresh || stored === null || now() - stored.checkedAt >= CHECK_INTERVAL_MS) {
    const fetched = await fetchLatestRelease(options);
    stored = { checkedAt: now(), latest: fetched?.latest ?? null, url: fetched?.url ?? null };
    writeStoredJson(CHECK_STORAGE_KEY, stored);
  }

  if (stored.latest === null || stored.url === null) return null;
  if (!isNewerRelease(currentVersion, stored.latest)) return null;
  if (getDismissedRelease() === stored.latest) return null;
  return { latest: stored.latest, url: stored.url };
}
