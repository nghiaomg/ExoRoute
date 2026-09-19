<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import { api, getAdminAccessToken } from '../../lib/api';
  import { pagePaths, type DashboardPage } from '../../lib/navigation';
  import type { Translate } from '../../lib/format';

  export let tr: Translate;
  export let mustChangePassword = false;
  export let authVersion = 0;
  export let onNavigate: (page: DashboardPage, updatePath?: boolean) => void;
  export let onRefresh: () => void;
  export let notice = '';
  export let noticeTone: 'info' | 'success' | 'error' = 'info';

  let pendingCallback: string | null = null;
  let submitting = false;
  let timer: number | null = null;
  let observedAuthVersion = authVersion;

  function showNotice(message: string, tone: 'info' | 'success' | 'error'): void {
    notice = message;
    noticeTone = tone;
    onNavigate('providers', false);
    onRefresh();
  }

  function clearTimer(): void {
    if (timer !== null) window.clearTimeout(timer);
    timer = null;
  }

  async function poll(providerId: string, flowId: string, deadline: number): Promise<void> {
    clearTimer();
    if (Date.now() >= deadline) {
      showNotice(tr('The callback is being processed. You can continue to the Providers page.'), 'info');
      return;
    }
    try {
      const result = await api.providerAuthStatus(providerId, flowId);
      if (result.status === 'connected') {
        showNotice(tr('Provider sign-in completed and the account is connected.'), 'success');
        return;
      }
      if (result.status === 'failed' || result.status === 'expired') {
        showNotice(tr('The callback was rejected or expired. Start a new sign-in.'), 'error');
        return;
      }
      timer = window.setTimeout(() => void poll(providerId, flowId, deadline), 1200);
    } catch {
      showNotice(tr('Could not process the OAuth callback. Start a new sign-in and try again.'), 'error');
    }
  }

  async function complete(): Promise<void> {
    const callbackUrl = pendingCallback;
    if (!callbackUrl || submitting || mustChangePassword || !getAdminAccessToken()) return;
    submitting = true;
    try {
      const result = await api.completeProviderAuthCallback(callbackUrl);
      pendingCallback = null;
      if (result.status === 'connected') {
        showNotice(tr('Provider sign-in completed and the account is connected.'), 'success');
      } else if (result.status === 'failed' || result.status === 'expired') {
        showNotice(tr('The callback was rejected or expired. Start a new sign-in.'), 'error');
      } else {
        showNotice(tr('The callback is being processed. You can continue to the Providers page.'), 'info');
        void poll(result.provider_id, result.flow_id, Date.now() + 5 * 60 * 1000);
      }
    } catch {
      pendingCallback = null;
      showNotice(tr('Could not process the OAuth callback. Start a new sign-in and try again.'), 'error');
    } finally {
      submitting = false;
    }
  }

  function callbackFromMessage(data: unknown): string | null {
    if (!data || typeof data !== 'object') return null;
    const message = data as { type?: unknown; callback_url?: unknown };
    if (message.type !== 'exoroute-provider-oauth-callback' || typeof message.callback_url !== 'string') return null;
    const callbackUrl = message.callback_url.trim();
    if (!callbackUrl) return null;
    try {
      const parsed = new URL(callbackUrl);
      if (parsed.origin !== window.location.origin || parsed.pathname.replace(/\/+$/, '') !== '/callback') return null;
    } catch {
      return null;
    }
    return callbackUrl;
  }

  function handleMessage(event: MessageEvent<unknown>): void {
    if (event.origin !== window.location.origin || event.source === window) return;
    const callbackUrl = callbackFromMessage(event.data);
    if (!callbackUrl) return;
    pendingCallback = callbackUrl;
    void complete();
  }

  $: if (authVersion !== observedAuthVersion) {
    observedAuthVersion = authVersion;
    void complete();
  }

  onMount(() => {
    if (window.location.pathname.replace(/\/+$/, '') === '/callback') {
      const callbackQuery = new URLSearchParams(window.location.search);
      if (callbackQuery.has('code') || callbackQuery.has('state') || callbackQuery.has('error')) {
        if (window.opener && window.opener !== window) {
          window.opener.postMessage(
            { type: 'exoroute-provider-oauth-callback', callback_url: window.location.href },
            window.location.origin,
          );
          window.close();
          return;
        }
        pendingCallback = window.location.href;
        window.history.replaceState(null, '', pagePaths.providers);
      }
    }
    window.addEventListener('message', handleMessage);
    void complete();
    return () => window.removeEventListener('message', handleMessage);
  });

  onDestroy(clearTimer);
</script>
