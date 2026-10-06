export type UpdateCheckStatus = 'up_to_date' | 'update_available' | 'unavailable';

export interface UpdateCheckResult {
  status: UpdateCheckStatus;
  current_version: string;
  latest_version: string | null;
  update_available: boolean;
  release_url: string | null;
}
