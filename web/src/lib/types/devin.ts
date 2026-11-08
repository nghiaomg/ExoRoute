/**
 * Devin cloud-agent integration types. The dashboard proxies the official
 * `https://api.devin.ai/v1` REST surface, so session fields follow that API;
 * optional fields stay optional because upstream may omit or extend them.
 */

export interface DevinSettingsResult {
  configured: boolean;
  master_key_configured: boolean;
}

export interface DevinVerifyResult {
  verified: boolean;
  status: number | null;
  /** Upstream error text or payload; shown as-is, never parsed for control. */
  detail: unknown;
}

export interface DevinPullRequest {
  url?: string;
  state?: string;
  title?: string;
  number?: number;
}

export interface DevinSession {
  session_id?: string;
  id?: string;
  status?: string;
  title?: string;
  created_at?: string;
  updated_at?: string;
  repository_url?: string;
  repository?: string;
  branch?: string;
  pull_request?: DevinPullRequest;
  [key: string]: unknown;
}
