import type { ProviderUsageQuota, ProviderUsageSnapshot } from '../../lib/types';

export function safePercent(value: number): number {
  return Number.isFinite(value) ? Math.min(100, Math.max(0, value)) : 0;
}

/** Deduplicated quota rows plus a synthetic monthly row derived from the
 * credit balance when the upstream does not report one. */
export function getDisplayQuotas(snap: ProviderUsageSnapshot | null): ProviderUsageQuota[] {
  if (!snap) return [];
  const list: ProviderUsageQuota[] = [];
  const seen = new Set<string>();
  for (const quota of snap.quotas ?? []) {
    // Upstream aliases can normalize to the same model ID. Keep the first
    // row so the keyed each block remains stable even if the API is stale.
    if (seen.has(quota.id)) continue;
    seen.add(quota.id);
    list.push(quota);
  }
  if (!list.some((q) => q.id === 'monthly') && snap.credit_balance) {
    const cb = snap.credit_balance;
    if (
      cb.period_used !== undefined && cb.period_used !== null &&
      cb.monthly_remaining !== undefined && cb.monthly_remaining !== null
    ) {
      const total = cb.period_used + cb.monthly_remaining;
      if (total > 0) {
        const usedPct = (cb.period_used / total) * 100;
        const remainingPct = safePercent(100 - usedPct);
        list.push({
          id: 'monthly',
          label: 'Monthly',
          used_percent: usedPct,
          remaining_percent: remainingPct,
          used_amount: cb.period_used,
          limit_amount: total,
          unit: cb.unit ?? 'credits',
          reset_at: cb.period_ends_at,
          uncapped: false,
        });
      }
    }
  }
  return list;
}

/** True when every usable quota or the credit balance is exhausted. */
export function isAccountLimitReached(snap: ProviderUsageSnapshot | null): boolean {
  if (!snap) return false;
  const quotasToCheck = getDisplayQuotas(snap);
  if (quotasToCheck.length > 0) {
    const anyQuotaSaturated = quotasToCheck.some(
      (q: ProviderUsageQuota) => !q.uncapped && safePercent(q.remaining_percent) <= 0
        || (q.limit_amount != null && q.limit_amount > 0 && (q.used_amount ?? 0) >= q.limit_amount)
    );
    if (anyQuotaSaturated) return true;

    if (snap.credit_balance) {
      const cb = snap.credit_balance;
      const hasCredits = (cb.monthly_remaining ?? 0) > 0 || (cb.purchased_remaining ?? 0) > 0 || (cb.free_remaining ?? 0) > 0;
      if (hasCredits) {
        return false;
      }
      if (cb.monthly_remaining !== undefined && cb.monthly_remaining !== null && cb.monthly_remaining <= 0) {
        return true;
      }
    }

    return false;
  }

  if (snap.credit_balance) {
    const cb = snap.credit_balance;
    if (cb.monthly_remaining !== undefined && cb.monthly_remaining !== null && cb.monthly_remaining <= 0 && (cb.purchased_remaining ?? 0) <= 0 && (cb.free_remaining ?? 0) <= 0) {
      return true;
    }
  }

  return Boolean(snap.limit_reached);
}
