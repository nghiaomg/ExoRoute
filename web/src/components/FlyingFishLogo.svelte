<script lang="ts">
  interface Props {
    size?: number;
    variant?: 'mark' | 'badge' | 'white' | 'monochrome';
    class?: string;
  }

  let { size = 22, variant = 'mark', class: className = '' }: Props = $props();
</script>

{#if variant === 'badge'}
  <svg
    xmlns="http://www.w3.org/2000/svg"
    viewBox="0 0 64 64"
    width={size}
    height={size}
    fill="none"
    class={className}
    aria-hidden="true"
  >
    <defs>
      <linearGradient id="compFavBody" x1="12" y1="46" x2="56" y2="16" gradientUnits="userSpaceOnUse">
        <stop offset="0%" stop-color="#c2410c" />
        <stop offset="50%" stop-color="#f97316" />
        <stop offset="100%" stop-color="#fb923c" />
      </linearGradient>
      <linearGradient id="compFavBelly" x1="16" y1="44" x2="52" y2="24" gradientUnits="userSpaceOnUse">
        <stop offset="0%" stop-color="#7c2d12" />
        <stop offset="60%" stop-color="#c2410c" />
        <stop offset="100%" stop-color="#fdba74" />
      </linearGradient>
      <linearGradient id="compFavWing" x1="37" y1="23" x2="16" y2="6" gradientUnits="userSpaceOnUse">
        <stop offset="0%" stop-color="#f97316" />
        <stop offset="50%" stop-color="#fdba74" />
        <stop offset="100%" stop-color="#fed7aa" />
      </linearGradient>
      <linearGradient id="compFavBackWing" x1="32" y1="28" x2="14" y2="22" gradientUnits="userSpaceOnUse">
        <stop offset="0%" stop-color="#7c2d12" stop-opacity="0.9" />
        <stop offset="100%" stop-color="#f97316" stop-opacity="0.95" />
      </linearGradient>
      <linearGradient id="compFavTail" x1="20" y1="40" x2="6" y2="54" gradientUnits="userSpaceOnUse">
        <stop offset="0%" stop-color="#c2410c" />
        <stop offset="100%" stop-color="#fb923c" />
      </linearGradient>
      <linearGradient id="compFavTrail" x1="6" y1="56" x2="28" y2="44" gradientUnits="userSpaceOnUse">
        <stop offset="0%" stop-color="#fb923c" stop-opacity="0" />
        <stop offset="100%" stop-color="#fdba74" stop-opacity="0.75" />
      </linearGradient>
    </defs>

    <path d="M6 56C12 55 18 51 22 47" stroke="url(#compFavTrail)" stroke-width="2.5" stroke-linecap="round" />
    <path d="M10 59C17 58 24 53 28 48" stroke="url(#compFavTrail)" stroke-width="2" stroke-dasharray="2 4" stroke-linecap="round" />
    <path d="M28 29L14 22C17 26 21 28 25 31L28 29Z" fill="url(#compFavBackWing)" />
    <path d="M18 42L11 34C13.5 38 15.5 40 17 41.5L18 42Z" fill="url(#compFavTail)" opacity="0.9" />
    <path d="M18 42L7 53C12 51 15 47.5 17 43.5L18 42Z" fill="url(#compFavTail)" />
    <path d="M18 42L11 34C13 39 12 47 7 53C12 50 16 46 18 42Z" fill="#fdba74" opacity="0.25" />
    <path d="M24 38L18 41C20.5 41 23 40 25 39L24 38Z" fill="#f97316" opacity="0.75" />
    <path d="M54 18C46 26 34 38 18 42C26 36 38 27 54 18Z" fill="url(#compFavBelly)" />
    <path d="M54 18C44 19 32 26 18 42C30 30 42 21 54 18Z" fill="url(#compFavBody)" />
    <path d="M37 23C32 17 25 10 16 6C18 12 21 16 25 19L33 24L37 23Z" fill="url(#compFavWing)" />
    <path d="M19 8.5C21.5 13 24 16 28 18.5" stroke="#ffffff" stroke-width="1.4" stroke-linecap="round" opacity="0.75" />
    <path d="M23 12C25 15 27 17.5 30 19.5" stroke="#ffedd5" stroke-width="1.2" stroke-linecap="round" opacity="0.65" />
    <path d="M27 15.5C28.5 17.5 30 19 32 20.5" stroke="#ffedd5" stroke-width="1" stroke-linecap="round" opacity="0.5" />
    <circle cx="48" cy="20" r="2.2" fill="#ffffff" />
    <circle cx="48.5" cy="19.8" r="1.2" fill="#7c2d12" />
    <circle cx="54" cy="18" r="1.1" fill="#fdba74" />
  </svg>
{:else if variant === 'white'}
  <!-- White & Cyan for dark/gradient containers -->
  <svg
    xmlns="http://www.w3.org/2000/svg"
    viewBox="0 0 64 64"
    width={size}
    height={size}
    fill="none"
    class={className}
    aria-hidden="true"
  >
    <!-- Streamlines -->
    <path d="M6 56C12 55 18 51 22 47" stroke="#fdba74" stroke-width="2" stroke-linecap="round" opacity="0.6" />
    <path d="M10 59C17 58 24 53 28 48" stroke="#fed7aa" stroke-width="1.5" stroke-dasharray="2 4" stroke-linecap="round" opacity="0.4" />
    <!-- Secondary Back Wing -->
    <path d="M28 29L14 22C17 26 21 28 25 31L28 29Z" fill="#ffedd5" opacity="0.5" />
    <!-- Forked Tail Fin -->
    <path d="M18 42L11 34C13.5 38 15.5 40 17 41.5L18 42Z" fill="#ffffff" opacity="0.8" />
    <path d="M18 42L7 53C12 51 15 47.5 17 43.5L18 42Z" fill="#ffffff" />
    <!-- Pelvic Fin -->
    <path d="M24 38L18 41C20.5 41 23 40 25 39L24 38Z" fill="#fdba74" opacity="0.7" />
    <!-- Body -->
    <path d="M54 18C46 26 34 38 18 42C26 36 38 27 54 18Z" fill="#fff7ed" opacity="0.9" />
    <path d="M54 18C44 19 32 26 18 42C30 30 42 21 54 18Z" fill="#ffffff" />
    <!-- Wing -->
    <path d="M37 23C32 17 25 10 16 6C18 12 21 16 25 19L33 24L37 23Z" fill="#ffffff" />
    <path d="M19 8.5C21.5 13 24 16 28 18.5" stroke="#c2410c" stroke-width="1.2" stroke-linecap="round" opacity="0.7" />
    <path d="M23 12C25 15 27 17.5 30 19.5" stroke="#fb923c" stroke-width="1" stroke-linecap="round" opacity="0.6" />
    <!-- Head & Eye -->
    <circle cx="48" cy="20" r="2.2" fill="#7c2d12" />
    <circle cx="48.5" cy="19.8" r="1" fill="#fdba74" />
    <circle cx="54" cy="18" r="1.1" fill="#fdba74" />
  </svg>
{:else if variant === 'monochrome'}
  <svg
    xmlns="http://www.w3.org/2000/svg"
    viewBox="0 0 64 64"
    width={size}
    height={size}
    fill="currentColor"
    class={className}
    aria-hidden="true"
  >
    <path d="M28 29L14 22C17 26 21 28 25 31L28 29Z" opacity="0.5" />
    <path d="M18 42L11 34C13.5 38 15.5 40 17 41.5L18 42Z" opacity="0.85" />
    <path d="M18 42L7 53C12 51 15 47.5 17 43.5L18 42Z" />
    <path d="M24 38L18 41C20.5 41 23 40 25 39L24 38Z" opacity="0.7" />
    <path d="M54 18C44 20 32 28 18 42C28 34 40 24 54 18Z" />
    <path d="M37 23C32 17 25 10 16 6C18 12 21 16 25 19L33 24L37 23Z" />
  </svg>
{:else}
  <!-- Full Color Vector Mark -->
  <svg
    xmlns="http://www.w3.org/2000/svg"
    viewBox="0 0 64 64"
    width={size}
    height={size}
    fill="none"
    class={className}
    aria-hidden="true"
  >
    <defs>
      <linearGradient id="compBodyGrad" x1="12" y1="46" x2="56" y2="16" gradientUnits="userSpaceOnUse">
        <stop offset="0%" stop-color="#c2410c" />
        <stop offset="50%" stop-color="#f97316" />
        <stop offset="100%" stop-color="#fb923c" />
      </linearGradient>
      <linearGradient id="compBellyGrad" x1="16" y1="44" x2="52" y2="24" gradientUnits="userSpaceOnUse">
        <stop offset="0%" stop-color="#7c2d12" />
        <stop offset="60%" stop-color="#c2410c" />
        <stop offset="100%" stop-color="#fdba74" />
      </linearGradient>
      <linearGradient id="compWingGrad" x1="37" y1="23" x2="16" y2="6" gradientUnits="userSpaceOnUse">
        <stop offset="0%" stop-color="#f97316" />
        <stop offset="50%" stop-color="#fdba74" />
        <stop offset="100%" stop-color="#fed7aa" />
      </linearGradient>
      <linearGradient id="compBackWing" x1="32" y1="28" x2="14" y2="22" gradientUnits="userSpaceOnUse">
        <stop offset="0%" stop-color="#7c2d12" stop-opacity="0.9" />
        <stop offset="100%" stop-color="#f97316" stop-opacity="0.95" />
      </linearGradient>
      <linearGradient id="compTailGrad" x1="20" y1="40" x2="6" y2="54" gradientUnits="userSpaceOnUse">
        <stop offset="0%" stop-color="#c2410c" />
        <stop offset="100%" stop-color="#fb923c" />
      </linearGradient>
      <linearGradient id="compTrailGrad" x1="6" y1="56" x2="28" y2="44" gradientUnits="userSpaceOnUse">
        <stop offset="0%" stop-color="#fb923c" stop-opacity="0" />
        <stop offset="100%" stop-color="#fdba74" stop-opacity="0.7" />
      </linearGradient>
    </defs>

    <!-- Speed / Flight Streamline Trails -->
    <path d="M6 56C12 55 18 51 22 47" stroke="url(#compTrailGrad)" stroke-width="2" stroke-linecap="round" />
    <path d="M10 59C17 58 24 53 28 48" stroke="url(#compTrailGrad)" stroke-width="1.5" stroke-dasharray="2 4" stroke-linecap="round" />

    <!-- Secondary Back Wing -->
    <path d="M28 29L14 22C17 26 21 28 25 31L28 29Z" fill="url(#compBackWing)" />

    <!-- Caudal Fin (Forked Tail) -->
    <path d="M18 42L11 34C13.5 38 15.5 40 17 41.5L18 42Z" fill="url(#compTailGrad)" opacity="0.9" />
    <path d="M18 42L7 53C12 51 15 47.5 17 43.5L18 42Z" fill="url(#compTailGrad)" />
    <path d="M18 42L11 34C13 39 12 47 7 53C12 50 16 46 18 42Z" fill="#fdba74" opacity="0.25" />

    <!-- Ventral Pelvic Fin -->
    <path d="M24 38L18 41C20.5 41 23 40 25 39L24 38Z" fill="#f97316" opacity="0.7" />

    <!-- Main Body -->
    <path d="M54 18C46 26 34 38 18 42C26 36 38 27 54 18Z" fill="url(#compBellyGrad)" />
    <path d="M54 18C44 19 32 26 18 42C30 30 42 21 54 18Z" fill="url(#compBodyGrad)" />

    <!-- Primary Gliding Wing -->
    <path d="M37 23C32 17 25 10 16 6C18 12 21 16 25 19L33 24L37 23Z" fill="url(#compWingGrad)" />
    <!-- Wing Rays -->
    <path d="M19 8.5C21.5 13 24 16 28 18.5" stroke="#ffffff" stroke-width="1.2" stroke-linecap="round" opacity="0.7" />
    <path d="M23 12C25 15 27 17.5 30 19.5" stroke="#ffedd5" stroke-width="1" stroke-linecap="round" opacity="0.6" />
    <path d="M27 15.5C28.5 17.5 30 19 32 20.5" stroke="#ffedd5" stroke-width="0.8" stroke-linecap="round" opacity="0.5" />

    <!-- Head Detail & Eye -->
    <circle cx="48" cy="20" r="2.2" fill="#ffffff" />
    <circle cx="48.5" cy="19.8" r="1.1" fill="#c2410c" />
    <circle cx="54" cy="18" r="1" fill="#fdba74" />
  </svg>
{/if}
