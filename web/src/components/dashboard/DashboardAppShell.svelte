<script lang="ts">
  import Button from "../Button.svelte";
  import type { Snippet } from 'svelte';
  import DashboardIconSprite from './DashboardIconSprite.svelte';
  import Input from '../Input.svelte';
  import { dashboardToast, submitTopSearch } from '../../lib/dashboard-ui';
  import { getStoredSidebarCollapsed, saveSidebarCollapsed } from '../../lib/sidebar';
  import { DASHBOARD_VERSION } from '../../lib/version';
  import { type Locale } from '../../lib/i18n';
  import LanguageSelect from '../LanguageSelect.svelte';
  import type { Translate } from '../../lib/format';
  import '../../styles/dashboard/base.css';

  interface Props {
    /** Dashboard page id — also scopes its page stylesheet. */
    page: string;
    /** Nav entry to highlight; defaults to `page` ('provider' maps to 'providers'). */
    activePage?: string;
    providerCount?: number;
    gatewayAddress?: string;
    /** Active dashboard language; drives the topbar language selector. */
    locale: Locale;
    /** Translation function for shell-owned copy. */
    tr: Translate;
    onLocaleChange: (next: Locale) => void;
    onNavigate: (page: string) => void;
    onSignOut?: () => void;
    /** Override topbar search behavior; default submits to pendingRequestSearch + navigates to requests. */
    onTopSearch?: (query: string) => void;
    children?: Snippet;
  }

  let {
    page,
    activePage,
    providerCount = 0,
    gatewayAddress = '127.0.0.1:8686',
    locale,
    tr,
    onLocaleChange,
    onNavigate,
    onSignOut,
    onTopSearch,
    children,
  }: Props = $props();

  let collapsed = $state(typeof window !== 'undefined' ? getStoredSidebarCollapsed() : false);
  let mobileOpen = $state(false);
  let searchQuery = $state('');

  const navPage = $derived(page === 'provider' ? 'providers' : (activePage ?? page));

  const menuItems = $derived([
    { id: 'overview', label: tr('Overview'), icon: 'i-grid', count: '' },
    { id: 'providers', label: tr('Providers'), icon: 'i-link', count: providerCount ? String(providerCount) : '' },
    { id: 'combos', label: tr('Combos'), icon: 'i-code', count: '' },
    { id: 'proxies', label: tr('Proxies'), icon: 'i-link', count: '' },
    { id: 'quota', label: tr('Quota'), icon: 'i-chart', count: '' },
    { id: 'requests', label: tr('Requests'), icon: 'i-clipboard', count: '' },
    { id: 'chat', label: tr('Chat'), icon: 'i-video', count: '' },
  ]);

  const generalItems = $derived([
    { id: 'api-keys', label: tr('API keys'), icon: 'i-key' },
    { id: 'settings', label: tr('Settings'), icon: 'i-settings' },
    { id: 'docs', label: tr('Docs'), icon: 'i-book' },
    { id: 'help', label: tr('Help Center'), icon: 'i-help' },
  ]);

  function toggleCollapse() {
    collapsed = !collapsed;
    saveSidebarCollapsed(collapsed);
  }

  function setMobileOpen(open: boolean) {
    mobileOpen = open;
  }

  function handleNav(id: string) {
    mobileOpen = false;
    onNavigate(id);
  }

  function handleTopSearch() {
    const q = searchQuery.trim();
    if (onTopSearch) {
      onTopSearch(q);
      return;
    }
    submitTopSearch(q);
    if (q) onNavigate('requests');
  }

  function handleSignOut() {
    mobileOpen = false;
    onSignOut?.();
  }

  // Modal delegation: [data-open-modal] / [data-close-modal], backdrop click, Esc.
  function onShellClick(event: MouseEvent) {
    const target = event.target as HTMLElement;
    const opener = target.closest('[data-open-modal]') as HTMLElement | null;
    if (opener) {
      const modalId = opener.dataset.openModal;
      const dialog = modalId ? document.getElementById(modalId) : null;
      if (dialog instanceof HTMLDialogElement) dialog.showModal();
      return;
    }
    const closer = target.closest('[data-close-modal]');
    if (closer) {
      const dialog = closer.closest('dialog');
      if (dialog instanceof HTMLDialogElement) dialog.close();
    }
  }

  function onShellKeydown(event: KeyboardEvent) {
    if (event.key === 'Escape') {
      mobileOpen = false;
      document.querySelectorAll('dialog[open]').forEach((d) => (d as HTMLDialogElement).close());
    }
  }

  function onDialogClick(event: MouseEvent) {
    if (event.target instanceof HTMLDialogElement) event.target.close();
  }

  $effect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') mobileOpen = false;
    };
    const mq = window.matchMedia('(min-width: 681px)');
    const onMq = (e: MediaQueryListEvent) => { if (e.matches) mobileOpen = false; };
    window.addEventListener('keydown', onKey);
    mq.addEventListener('change', onMq);
    return () => {
      window.removeEventListener('keydown', onKey);
      mq.removeEventListener('change', onMq);
    };
  });
</script>

<svelte:window onclick={onShellClick} />

<div class="dashboard-app dashboard-page-{page}">
  <DashboardIconSprite />
  <main class="poster">
    <section class="dashboard-card" aria-label="ExoRoute dashboard">
      <div class="app-layout" class:collapsed>
        <aside class="sidebar" class:open={mobileOpen}>
          <div class="brand-card">
            <div class="brand">
              <svg class="brand-mark" viewBox="0 0 64 64" fill="none" aria-hidden="true">
                <defs>
                  <linearGradient id="compBodyGrad" x1="12" y1="46" x2="56" y2="16" gradientUnits="userSpaceOnUse"><stop offset="0%" style="stop-color: var(--accent-deep)"></stop><stop offset="50%" style="stop-color: var(--accent)"></stop><stop offset="100%" style="stop-color: var(--accent-glow)"></stop></linearGradient>
                  <linearGradient id="compBellyGrad" x1="16" y1="44" x2="52" y2="24" gradientUnits="userSpaceOnUse"><stop offset="0%" style="stop-color: color-mix(in srgb, var(--accent) 45%, black)"></stop><stop offset="60%" style="stop-color: var(--accent-deep)"></stop><stop offset="100%" style="stop-color: var(--accent-soft)"></stop></linearGradient>
                  <linearGradient id="compWingGrad" x1="37" y1="23" x2="16" y2="6" gradientUnits="userSpaceOnUse"><stop offset="0%" style="stop-color: var(--accent)"></stop><stop offset="50%" style="stop-color: var(--accent-soft)"></stop><stop offset="100%" style="stop-color: color-mix(in srgb, var(--accent) 30%, #fff)"></stop></linearGradient>
                  <linearGradient id="compBackWing" x1="32" y1="28" x2="14" y2="22" gradientUnits="userSpaceOnUse"><stop offset="0%" style="stop-color: color-mix(in srgb, var(--accent) 45%, black)" stop-opacity="0.9"></stop><stop offset="100%" style="stop-color: var(--accent)" stop-opacity="0.95"></stop></linearGradient>
                  <linearGradient id="compTailGrad" x1="20" y1="40" x2="6" y2="54" gradientUnits="userSpaceOnUse"><stop offset="0%" style="stop-color: var(--accent-deep)"></stop><stop offset="100%" style="stop-color: var(--accent-glow)"></stop></linearGradient>
                  <linearGradient id="compTrailGrad" x1="6" y1="56" x2="28" y2="44" gradientUnits="userSpaceOnUse"><stop offset="0%" style="stop-color: var(--accent-glow)" stop-opacity="0"></stop><stop offset="100%" style="stop-color: var(--accent-soft)" stop-opacity="0.7"></stop></linearGradient>
                </defs>
                <path d="M6 56C12 55 18 51 22 47" stroke="url(#compTrailGrad)" stroke-width="2" stroke-linecap="round"></path>
                <path d="M10 59C17 58 24 53 28 48" stroke="url(#compTrailGrad)" stroke-width="1.5" stroke-dasharray="2 4" stroke-linecap="round"></path>
                <path d="M28 29L14 22C17 26 21 28 25 31L28 29Z" fill="url(#compBackWing)"></path>
                <path d="M18 42L11 34C13.5 38 15.5 40 17 41.5L18 42Z" fill="url(#compTailGrad)" opacity="0.9"></path>
                <path d="M18 42L7 53C12 51 15 47.5 17 43.5L18 42Z" fill="url(#compTailGrad)"></path>
                <path d="M18 42L11 34C13 39 12 47 7 53C12 50 16 46 18 42Z" style="fill: var(--accent-soft)" opacity="0.25"></path>
                <path d="M24 38L18 41C20.5 41 23 40 25 39L24 38Z" style="fill: var(--accent)" opacity="0.7"></path>
                <path d="M54 18C46 26 34 38 18 42C26 36 38 27 54 18Z" fill="url(#compBellyGrad)"></path>
                <path d="M54 18C44 19 32 26 18 42C30 30 42 21 54 18Z" fill="url(#compBodyGrad)"></path>
                <path d="M37 23C32 17 25 10 16 6C18 12 21 16 25 19L33 24L37 23Z" fill="url(#compWingGrad)"></path>
                <path d="M19 8.5C21.5 13 24 16 28 18.5" stroke="#ffffff" stroke-width="1.2" stroke-linecap="round" opacity="0.7"></path>
                <path d="M23 12C25 15 27 17.5 30 19.5" style="stroke: color-mix(in srgb, var(--accent) 14%, #fff)" stroke-width="1" stroke-linecap="round" opacity="0.6"></path>
                <path d="M27 15.5C28.5 17.5 30 19 32 20.5" style="stroke: color-mix(in srgb, var(--accent) 14%, #fff)" stroke-width="0.8" stroke-linecap="round" opacity="0.5"></path>
                <circle cx="48" cy="20" r="2.2" fill="#ffffff"></circle>
                <circle cx="48.5" cy="19.8" r="1.1" style="fill: var(--accent-deep)"></circle>
                <circle cx="54" cy="18" r="1" style="fill: var(--accent-soft)"></circle>
              </svg>
              <span>ExoRoute</span>
              <span class="brand-version">{DASHBOARD_VERSION}</span>
            </div>
            <Button class="collapse-button" type="button" aria-label={tr('Collapse sidebar')} onclick={toggleCollapse}>
              <svg class="icon"><use href="#i-chevron" /></svg>
            </Button>
          </div>

          <nav class="side-panel" aria-label={tr('Main navigation')}>
            <div class="nav-section">
              <p class="eyebrow">{tr('Menu')}</p>
              <div class="nav-list">
                {#each menuItems as item (item.id)}
                  <Button
                    class="nav-button" stateClasses={{ active: navPage === item.id }}

                    type="button"
                    aria-current={navPage === item.id ? 'page' : undefined}
                    onclick={() => handleNav(item.id)}
                  >
                    <span class="nav-icon"><svg class="icon"><use href="#{item.icon}" /></svg></span>
                    <span class="nav-label">{item.label}</span>
                    {#if item.count}<span class="nav-count">{item.count}</span>{/if}
                  </Button>
                {/each}
              </div>
            </div>

            <div class="nav-section">
              <p class="eyebrow">{tr('General')}</p>
              <div class="nav-list">
                {#each generalItems as item (item.id)}
                  <Button
                    class="nav-button" stateClasses={{ active: navPage === item.id }}

                    type="button"
                    onclick={() => handleNav(item.id)}
                  >
                    <span class="nav-icon"><svg class="icon"><use href="#{item.icon}" /></svg></span>
                    <span class="nav-label">{item.label}</span>
                  </Button>
                {/each}
              </div>
            </div>

            {#if onSignOut}
              <Button class="nav-button logout" type="button" onclick={handleSignOut}>
                <span class="nav-icon"><svg class="icon"><use href="#i-logout" /></svg></span>
                <span class="nav-label">{tr('Log Out')}</span>
              </Button>
            {/if}
          </nav>
        </aside>
        <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
        <div class="sidebar-backdrop" hidden={!mobileOpen} onclick={() => setMobileOpen(false)}></div>

        <section class="main-area">
          <header class="topbar">
            <label class="search-box">
              <svg class="icon" aria-hidden="true"><use href="#i-search" /></svg>
              <Input
                type="search"
                placeholder={tr('Search requests...')}
                aria-label={tr('Search requests')}
                bind:value={searchQuery}
                onkeydown={(e) => { if (e.key === 'Enter') handleTopSearch(); }}
              />
            </label>
            <div class="top-actions">
              <Button class="icon-button mobile-menu-btn" type="button" aria-label={tr('Toggle navigation')} aria-expanded={mobileOpen} onclick={() => setMobileOpen(!mobileOpen)}>
                <svg class="icon"><use href="#i-menu" /></svg>
              </Button>
              <LanguageSelect {locale} {tr} onLocaleChange={onLocaleChange} />
              <span class="top-divider" aria-hidden="true"></span>
              <div class="profile">
                <svg class="avatar" viewBox="0 0 44 44" role="img" aria-label={tr('ExoRoute Admin')}>
                  <defs>
                    <linearGradient id="avatar-bg" x1="0" y1="0" x2="1" y2="1">
                      <stop stop-color="#d6e1e3" />
                      <stop offset="1" stop-color="#9aa7ad" />
                    </linearGradient>
                  </defs>
                  <circle cx="22" cy="22" r="22" fill="url(#avatar-bg)" />
                  <path d="M9 43c1.6-8 6.2-12 13-12s11.4 4 13 12" fill="#293746" />
                  <path d="M15 16c0-6 3.2-9 7.4-9 5.1 0 8.5 3.6 8.5 9v5.2c0 5.3-3.9 8.8-8.4 8.8S15 26.5 15 21.2z" fill="#bd8a6a" />
                  <path d="M14.5 17c-.6-7.1 3.3-11 8.6-11 5.8 0 9.2 4.5 8.2 11-1.5-1.1-3.1-2.5-4.4-4.6-2.4 2.7-6.8 4-12.4 4.6Z" fill="#29323a" />
                  <path d="M19 22h.2m7.4 0h.2" stroke="#29323a" stroke-width="1.4" stroke-linecap="round" />
                  <path d="M20 26c1.5 1.1 3.4 1.1 4.9 0" fill="none" stroke="#855c4d" stroke-width="1.2" stroke-linecap="round" />
                </svg>
                <div class="profile-copy">
                  <strong>{tr('ExoRoute Admin')}</strong>
                  <span>{gatewayAddress}</span>
                </div>
              </div>
            </div>
          </header>

          <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
          <div class="content" onclick={onDialogClick}>
            {@render children?.()}
          </div>
        </section>
      </div>
    </section>
  </main>

  {#if $dashboardToast}
    <div class="toast" role="status" aria-live="polite">{$dashboardToast.message}</div>
  {/if}
</div>
