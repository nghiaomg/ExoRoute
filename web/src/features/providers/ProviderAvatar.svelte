<script lang="ts">
  import { providerLogo } from './provider-logo';

  export let name: string;
  export let logoUrl: string | null | undefined = null;
  export let adapterId: string | null | undefined = null;
  export let providerId: string | null | undefined = null;
  export let className = '';

  let imageFailed = false;

  $: logoSrc = providerLogo(logoUrl, adapterId, providerId, name);
  $: if (logoSrc) {
    imageFailed = false;
  }
  $: showImage = Boolean(logoSrc && !imageFailed);
  $: initial = getInitial(name);

  function getInitial(str?: string | null): string {
    if (!str) return 'P';
    const trimmed = str.trim();
    if (!trimmed) return 'P';
    const match = trimmed.match(/[\p{L}\p{N}]/u);
    return (match ? match[0] : trimmed.charAt(0)).toUpperCase();
  }
</script>

<span class="provider-avatar provider-logo {className}" class:has-image={showImage} aria-hidden="true">
  {#if showImage}
    <img
      src={logoSrc}
      alt=""
      loading="lazy"
      referrerpolicy="no-referrer"
      onload={() => { imageFailed = false; }}
      onerror={() => { imageFailed = true; }}
    />
  {:else}
    <span class="provider-avatar-fallback">{initial}</span>
  {/if}
</span>
