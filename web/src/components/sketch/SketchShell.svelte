<script lang="ts">
  import type { Snippet } from 'svelte';
  import SketchIconSprite from './SketchIconSprite.svelte';
  import { sketchToast, showSketchToast, submitTopSearch } from '../../lib/sketch';
  import { getStoredSidebarCollapsed, saveSidebarCollapsed } from '../../lib/sidebar';
  import '../../styles/sketch/base.css';

  interface Props {
    /** Sketch page id — also scopes the page stylesheet (sketch-page-<id>). */
    page: string;
    /** Nav entry to highlight; defaults to `page` ('provider' maps to 'providers'). */
    activePage?: string;
    providerCount?: number;
    gatewayAddress?: string;
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
    { id: 'overview', label: 'Overview', icon: 'i-grid', count: '3' },
    { id: 'providers', label: 'Providers', icon: 'i-link', count: providerCount ? String(providerCount) : '' },
    { id: 'combos', label: 'Combos', icon: 'i-code', count: '' },
    { id: 'quota', label: 'Quota', icon: 'i-chart', count: '' },
    { id: 'requests', label: 'Requests', icon: 'i-clipboard', count: '' },
    { id: 'chat', label: 'Chat', icon: 'i-video', count: '' },
  ]);

  const generalItems = [
    { id: 'api-keys', label: 'API keys', icon: 'i-key' },
    { id: 'settings', label: 'Settings', icon: 'i-settings' },
    { id: 'docs', label: 'Docs', icon: 'i-book' },
    { id: 'help', label: 'Help Center', icon: 'i-help' },
  ];

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

<div class="sketch-app sketch-page-{page}">
  <SketchIconSprite />
  <main class="poster">
    <section class="dashboard-card" aria-label="ExoRoute dashboard">
      <div class="app-layout" class:collapsed>
        <aside class="sidebar" class:open={mobileOpen}>
          <div class="brand-card">
            <div class="brand">
              <svg class="brand-mark" viewBox="0 0 36 32" aria-hidden="true">
                <path d="M5 7.5 11 4h21l-6 5H7a3 3 0 0 0-2 5.3v6.2L1 18V11a4 4 0 0 1 4-3.5Z" fill="#283442" />
                <path d="M31 24.5 25 28H4l6-5h19a3 3 0 0 0 2-5.3v-6.2l4 2.5v7a4 4 0 0 1-4 3.5Z" fill="#283442" />
                <path d="M23 9h7l-5 4H10l5-4h8Z" fill="#f16b50" />
                <path d="M7 19h8l-5 4H3z" fill="#e9edf0" />
              </svg>
              <span>ExoRoute</span>
            </div>
            <button class="collapse-button" type="button" aria-label="Collapse sidebar" onclick={toggleCollapse}>
              <svg class="icon"><use href="#i-chevron" /></svg>
            </button>
          </div>

          <nav class="side-panel" aria-label="Main navigation">
            <div class="nav-section">
              <p class="eyebrow">MENU</p>
              <div class="nav-list">
                {#each menuItems as item (item.id)}
                  <button
                    class="nav-button"
                    class:active={navPage === item.id}
                    type="button"
                    aria-current={navPage === item.id ? 'page' : undefined}
                    onclick={() => handleNav(item.id)}
                  >
                    <span class="nav-icon"><svg class="icon"><use href="#{item.icon}" /></svg></span>
                    <span class="nav-label">{item.label}</span>
                    {#if item.count}<span class="nav-count">{item.count}</span>{/if}
                  </button>
                {/each}
              </div>
            </div>

            <div class="nav-section">
              <p class="eyebrow">GENERAL</p>
              <div class="nav-list">
                {#each generalItems as item (item.id)}
                  <button
                    class="nav-button"
                    class:active={navPage === item.id}
                    type="button"
                    onclick={() => handleNav(item.id)}
                  >
                    <span class="nav-icon"><svg class="icon"><use href="#{item.icon}" /></svg></span>
                    <span class="nav-label">{item.label}</span>
                  </button>
                {/each}
              </div>
            </div>

            <button class="nav-button logout" type="button" onclick={handleSignOut}>
              <span class="nav-icon"><svg class="icon"><use href="#i-logout" /></svg></span>
              <span class="nav-label">Log Out</span>
            </button>
          </nav>
        </aside>
        <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
        <div class="sidebar-backdrop" hidden={!mobileOpen} onclick={() => setMobileOpen(false)}></div>

        <section class="main-area">
          <header class="topbar">
            <label class="search-box">
              <svg class="icon" aria-hidden="true"><use href="#i-search" /></svg>
              <input
                type="search"
                placeholder="Search requests..."
                aria-label="Search requests"
                bind:value={searchQuery}
                onkeydown={(e) => { if (e.key === 'Enter') handleTopSearch(); }}
              />
            </label>
            <div class="top-actions">
              <button class="icon-button mobile-menu-btn" type="button" aria-label="Toggle navigation" aria-expanded={mobileOpen} onclick={() => setMobileOpen(!mobileOpen)}>
                <svg class="icon"><use href="#i-menu" /></svg>
              </button>
              <button class="icon-button" type="button" aria-label="Messages" onclick={() => showSketchToast('No new messages')}>
                <svg class="icon"><use href="#i-mail" /></svg>
              </button>
              <button class="icon-button" type="button" aria-label="Notifications" onclick={() => showSketchToast('No unread notifications')}>
                <svg class="icon"><use href="#i-bell" /></svg>
              </button>
              <span class="top-divider" aria-hidden="true"></span>
              <div class="profile">
                <svg class="avatar" viewBox="0 0 44 44" role="img" aria-label="ExoRoute Admin">
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
                  <strong>ExoRoute Admin</strong>
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

  {#if $sketchToast}
    <div class="toast" role="status" aria-live="polite">{$sketchToast.message}</div>
  {/if}
</div>
