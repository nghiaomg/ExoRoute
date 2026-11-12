<script lang="ts">
  import type { Snippet } from 'svelte';
  import DashboardIconSprite from './DashboardIconSprite.svelte';
  import { dashboardToast, submitTopSearch } from '../../lib/dashboard-ui';
  import { getStoredSidebarCollapsed, saveSidebarCollapsed } from '../../lib/sidebar';
  import { DASHBOARD_VERSION } from '../../lib/version';
  import { type Locale } from '../../lib/i18n';
  import LanguageSelect from '../LanguageSelect.svelte';
  import Input from '../Input.svelte';
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

  const bottomItems = $derived([
    { id: 'overview', label: tr('Overview'), icon: 'i-grid' },
    { id: 'providers', label: tr('Providers'), icon: 'i-link' },
    { id: 'requests', label: tr('Requests'), icon: 'i-clipboard' },
    { id: 'chat', label: tr('Chat'), icon: 'i-video' },
    { id: 'more', label: tr('More'), icon: 'i-menu' },
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
      document.querySelectorAll('dialog[open]:not([data-prevent-close])').forEach((d) => (d as HTMLDialogElement).close());
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

  /* Shared shell utility strings. Breakpoints: `nav:` = >=681px (desktop
     fixed sidebar), `max-nav:` = <=680px (mobile drawer), `@max-[920px]:` =
     the dashboard-card container query. Colors come from the @theme bridge in
     styles/tailwind.css, which references the live theme custom properties.
     `!` marks utilities that must out-rank the unlayered legacy resets
     (`.dashboard-app button { color: inherit }`, `.icon` sizing). */
  const focusRing = 'focus-visible:outline-none focus-visible:ring-3 focus-visible:ring-db-accent-ring';

  const layoutClass = $derived(`grid min-w-0 max-nav:h-auto max-nav:min-h-[100svh] max-nav:grid-cols-[minmax(0,1fr)] max-nav:gap-2.5 nav:block nav:h-[100svh] nav:min-h-[max(700px,100svh)] @max-[920px]:gap-3 [--shell-header-row:68px] ${collapsed ? 'nav:[--shell-header-row:80px]' : ''}`);

  const sidebarClass = $derived(`grid min-w-0 min-h-0 max-nav:fixed max-nav:top-3 max-nav:bottom-3 max-nav:left-3 max-nav:z-50 max-nav:w-[min(300px,84vw)] max-nav:h-auto max-nav:gap-[9px] max-nav:[grid-template-rows:54px_auto] max-nav:transition-transform max-nav:duration-200 ${mobileOpen ? 'max-nav:translate-x-0' : 'max-nav:-translate-x-[115%]'} nav:fixed nav:inset-y-0 nav:left-0 nav:z-40 nav:transition-[width] nav:duration-200 nav:gap-[clamp(10px,1.32vw,18px)] nav:[grid-template-rows:var(--shell-header-row)_minmax(0,1fr)] ${collapsed ? 'nav:w-[78px]' : 'nav:w-[calc(100vw*0.204)]'}`);

  const brandCardClass = $derived(`flex min-w-0 items-center justify-between rounded-[15px] bg-surface max-nav:px-3.5 nav:px-[8%] ${collapsed ? 'nav:flex-col nav:justify-start nav:gap-2 nav:px-1 nav:py-2' : ''}`);

  const brandClass = $derived(`flex min-w-0 items-center gap-[7px] text-ink text-section-title font-bold tracking-[-0.035em] ${collapsed ? 'nav:justify-center' : ''}`);

  const sidePanelClass = $derived(`flex min-h-0 flex-col overflow-y-auto rounded-[17px] bg-surface px-[6.5%] py-[5.8%] max-nav:p-2.5 [scrollbar-width:thin] [scrollbar-color:var(--text-dim)_transparent] @max-[920px]:p-[9px] ${collapsed ? 'nav:px-2 nav:py-2.5 nav:items-center' : ''}`);

  function navButtonClass(active: boolean): string {
    return `relative flex w-full min-w-0 min-h-[clamp(32px,3.45vw,46px)] items-center justify-start gap-2.5 rounded-full border-0 px-[7px] text-left cursor-pointer ${focusRing} @max-[920px]:min-h-[30px] @max-[920px]:gap-1.5 @max-[920px]:px-1 max-nav:min-h-[38px] max-nav:gap-[7px] max-nav:px-1.5 ${collapsed ? 'nav:justify-center nav:min-h-10 nav:px-0' : ''} ${active ? 'bg-db-accent-tint text-accent-strong!' : 'bg-transparent text-ink hover:bg-db-tint-strong'} [aria-current='page']:cursor-default`;
  }

  function navIconClass(active: boolean): string {
    return `grid size-[clamp(27px,2.55vw,35px)] shrink-0 place-items-center rounded-full @max-[920px]:size-[26px] max-nav:size-[29px] ${collapsed ? 'nav:size-[34px]' : ''} ${active ? 'bg-accent text-accent-contrast' : 'bg-line text-muted'}`;
  }

  function navCountClass(active: boolean): string {
    // leading-none keeps the digits inside the pill: the inherited body
    // line-height (~1.45) makes a 13px caption line taller than the fixed
    // 19px/16px height, so multi-digit counts spilled past the rounded edge.
    return `grid ms-auto h-[19px] min-w-[19px] shrink-0 place-items-center rounded-full px-1 text-caption leading-none whitespace-nowrap font-semibold tabular-nums @max-[920px]:h-4 @max-[920px]:min-w-4 @max-[920px]:px-[3px] max-nav:h-[18px] max-nav:min-w-[18px] ${collapsed ? 'nav:hidden' : ''} ${active ? 'bg-accent text-accent-contrast' : 'bg-ink text-canvas'}`;
  }
</script>

<svelte:window onclick={onShellClick} />

<div class="dashboard-app dashboard-page-{page}">
  <DashboardIconSprite />
  <main class="poster relative min-h-[100svh] bg-db-tint-strong max-nav:block max-nav:overflow-visible">
    <section class="dashboard-card relative w-full min-h-[100svh] bg-db-tint-strong @container max-nav:p-3" aria-label="ExoRoute dashboard">
      <div class={layoutClass}>
        <aside class={sidebarClass} class:open={mobileOpen}>
          <div class={brandCardClass}>
            <div class={brandClass}>
              <svg class="w-[clamp(25px,2.05vw,29px)] h-[clamp(25px,2.05vw,29px)] shrink-0 overflow-visible max-nav:size-[27px]" viewBox="0 0 64 64" fill="none" aria-hidden="true">
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
              <span class="min-w-0 overflow-hidden text-ellipsis whitespace-nowrap {collapsed ? 'nav:hidden' : ''}">ExoRoute</span>
              <span class="shrink-0 self-center ms-0.5 rounded border px-[5px] py-[3px] text-[12px] font-bold leading-none tracking-[0.3px] whitespace-nowrap border-[color-mix(in_srgb,var(--accent)_15%,transparent)] bg-[color-mix(in_srgb,var(--accent)_8%,transparent)] text-accent-strong @max-[920px]:hidden {collapsed ? 'nav:hidden' : ''}">{DASHBOARD_VERSION}</span>
            </div>
            <button type="button" aria-label={tr('Collapse sidebar')} onclick={toggleCollapse} class="group/collapse inline-grid size-[25px] shrink-0 place-items-center rounded-full border-0 bg-db-tint-strong p-0 text-text-dim cursor-pointer transition-colors {focusRing} max-nav:hidden">
              <svg class="icon size-[13px]! transition-transform duration-200 group-hover/collapse:-translate-x-0.5 {collapsed ? 'nav:rotate-180 nav:group-hover/collapse:translate-x-0.5' : ''}"><use href="#i-chevron" /></svg>
            </button>
          </div>

          <nav class={sidePanelClass} aria-label={tr('Main navigation')}>
            <div>
              <p class="m-0 mb-2 ms-[7px] text-[11px] font-medium tracking-[0.03em] text-muted uppercase {collapsed ? 'nav:hidden' : ''}">{tr('Menu')}</p>
              <div class="grid gap-0.5 {collapsed ? 'nav:justify-items-center' : ''}">
                {#each menuItems as item (item.id)}
                  {@const active = navPage === item.id}
                  <button
                    type="button"
                    aria-current={active ? 'page' : undefined}
                    onclick={() => handleNav(item.id)}
                    class={navButtonClass(active)}
                  >
                    <span class={navIconClass(active)}><svg class="icon size-[53%]!"><use href="#{item.icon}" /></svg></span>
                    <span class="overflow-hidden text-ellipsis whitespace-nowrap text-[14px] font-normal {collapsed ? 'nav:hidden' : ''}">{item.label}</span>
                    {#if item.count}<span class={navCountClass(active)}>{item.count}</span>{/if}
                  </button>
                {/each}
              </div>
            </div>

            <div class="mt-[5.5%] max-nav:mt-2.5">
              <p class="m-0 mb-2 ms-[7px] text-[11px] font-medium tracking-[0.03em] text-muted uppercase {collapsed ? 'nav:hidden' : ''}">{tr('General')}</p>
              <div class="grid gap-0.5 {collapsed ? 'nav:justify-items-center' : ''}">
                {#each generalItems as item (item.id)}
                  {@const active = navPage === item.id}
                  <button
                    type="button"
                    onclick={() => handleNav(item.id)}
                    class={navButtonClass(active)}
                  >
                    <span class={navIconClass(active)}><svg class="icon size-[53%]!"><use href="#{item.icon}" /></svg></span>
                    <span class="overflow-hidden text-ellipsis whitespace-nowrap text-[14px] font-normal {collapsed ? 'nav:hidden' : ''}">{item.label}</span>
                  </button>
                {/each}
              </div>
            </div>

            {#if onSignOut}
              <button type="button" onclick={handleSignOut} class="mt-[3.5%] text-accent! {navButtonClass(false)}">
                <span class={navIconClass(true)}><svg class="icon size-[53%]!"><use href="#i-logout" /></svg></span>
                <span class="overflow-hidden text-ellipsis whitespace-nowrap text-[14px] font-normal {collapsed ? 'nav:hidden' : ''}">{tr('Log Out')}</span>
              </button>
            {/if}
          </nav>
        </aside>
        <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
        <div class="max-nav:fixed max-nav:inset-0 max-nav:z-[45] max-nav:bg-[#12182066] max-nav:backdrop-blur-[2px]" hidden={!mobileOpen} onclick={() => setMobileOpen(false)}></div>

        <section class="grid min-w-0 min-h-0 max-nav:min-h-[100svh] max-nav:[grid-template-rows:56px_auto] max-nav:gap-2.5 nav:h-full nav:ml-[calc(100vw*0.219)] nav:gap-[1.8%] nav:[grid-template-rows:var(--shell-header-row)_minmax(0,1fr)] {collapsed ? 'nav:ml-[92px]' : ''}">
          <header class="flex h-full min-w-0 items-center justify-between gap-[18px] rounded-[16px] bg-surface px-[2.3%] max-nav:gap-2 max-nav:rounded-[13px] max-nav:px-[9px]">
            <label class="group/search hidden w-[min(43%,370px)] min-w-[120px] items-center gap-3 rounded-xl border border-line bg-surface px-3.5 py-3 text-text-dim transition-[border-color,box-shadow] duration-150 focus-within:border-accent focus-within:ring-3 focus-within:ring-accent/15 nav:flex">
              <svg class="icon size-4! transition-colors duration-200 group-focus-within/search:text-accent" aria-hidden="true"><use href="#i-search" /></svg>
              <Input
                type="search"
                placeholder={tr('Search requests...')}
                aria-label={tr('Search requests')}
                bind:value={searchQuery}
                onkeydown={(e) => { if (e.key === 'Enter') handleTopSearch(); }}
                bare
class="w-full bg-transparent text-sm outline-none placeholder:text-muted"
              />
            </label>
            <div class="flex min-w-0 items-center gap-[clamp(8px,1.2vw,17px)] max-nav:gap-1.5">
              <button type="button" aria-label={tr('Toggle navigation')} aria-expanded={mobileOpen} onclick={() => setMobileOpen(!mobileOpen)} class="group/menu inline-grid size-[38px] shrink-0 place-items-center rounded-full border-0 bg-db-tint-strong p-0 text-muted transition-colors cursor-pointer hover:bg-line {focusRing} nav:hidden">
                <svg class="icon transition-transform duration-200 group-hover/menu:scale-110"><use href="#i-menu" /></svg>
              </button>
              <LanguageSelect {locale} {tr} onLocaleChange={onLocaleChange} />
              <span class="hidden w-px h-6 bg-line nav:block" aria-hidden="true"></span>
              <div class="group/profile flex min-w-0 items-center gap-2.5 max-nav:gap-1.5">
                <svg class="size-[clamp(32px,3.1vw,43px)] shrink-0 overflow-hidden rounded-full border-2 border-line max-nav:size-[31px] transition-transform duration-200 group-hover/profile:scale-[1.06]" viewBox="0 0 44 44" role="img" aria-label={tr('ExoRoute Admin')}>
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
                <div class="hidden grid gap-[3px] whitespace-nowrap leading-[1.15] nav:grid">
                  <strong class="overflow-hidden text-ellipsis text-label font-semibold transition-colors duration-150 group-hover/profile:text-accent max-nav:max-w-[78px]">{tr('ExoRoute Admin')}</strong>
                  <span class="text-muted text-meta">{gatewayAddress}</span>
                </div>
              </div>
            </div>
          </header>

          <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
          <div class="grid min-w-0 min-h-0 gap-[1.8%] max-nav:gap-3 max-nav:[grid-template-rows:auto_auto] max-nav:pb-[calc(64px+env(safe-area-inset-bottom,0px))] nav:flex nav:flex-col nav:h-full nav:min-h-0 nav:overflow-y-auto nav:[&>*]:shrink-0 [scrollbar-width:thin] [scrollbar-color:var(--text-dim)_transparent]" onclick={onDialogClick}>
            {@render children?.()}
          </div>
        </section>
      </div>
    </section>
  </main>

  <!-- Mobile app tab bar: fixed bottom, five primary destinations; "More"
       opens the full sidebar drawer. Hidden at >=681px where the rail owns
       navigation. -->
  <nav
    class="max-nav:fixed max-nav:inset-x-0 max-nav:bottom-0 max-nav:z-40 max-nav:grid max-nav:grid-cols-5 max-nav:border-t max-nav:border-line max-nav:bg-surface max-nav:pt-1 max-nav:pb-[max(env(safe-area-inset-bottom,0px),8px)] nav:hidden"
    aria-label={tr('Main navigation')}
  >
    {#each bottomItems as item (item.id)}
      <button
        type="button"
        aria-label={item.label}
        aria-current={item.id !== 'more' && navPage === item.id ? 'page' : undefined}
        onclick={() => (item.id === 'more' ? setMobileOpen(true) : handleNav(item.id))}
        class="flex min-h-[48px] flex-col items-center justify-center gap-0.5 border-0 bg-transparent px-1 py-1 text-[10px] font-medium leading-tight cursor-pointer transition-colors {navPage === item.id && item.id !== 'more' ? 'text-accent-strong' : 'text-muted'} {focusRing}"
      >
        <svg class="icon size-[18px]!"><use href="#{item.icon}" /></svg>
        <span class="max-w-full truncate">{item.label}</span>
      </button>
    {/each}
  </nav>

  {#if $dashboardToast}
    <div class="fixed right-5 bottom-5 z-20 max-nav:bottom-[calc(72px+env(safe-area-inset-bottom,0px))] max-w-[min(380px,calc(100vw-40px))] rounded-xl border border-ink bg-ink px-4 py-3 text-canvas text-dash-body leading-[1.4] shadow-[0_10px_30px_#0003]" role="status" aria-live="polite">{$dashboardToast.message}</div>
  {/if}
</div>
