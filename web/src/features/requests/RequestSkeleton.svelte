<script lang="ts">
  import type { Translate } from '../../lib/format';

  export let tr: Translate;

  const placeholders = [0, 1, 2, 3, 4, 5];
  const columns = ['time', 'key', 'model', 'alias', 'provider', 'duration', 'tokens', 'status'];
</script>

<!-- Placeholder rows shaped like the request table, so the first load does not
     shift the page when the real rows arrive. -->
<div
  class="table-card request-table-card request-skeleton"
  role="status"
  aria-label={tr('Loading {page}…', { page: tr('Requests').toLowerCase() })}
>
  {#each placeholders as row (row)}
    <div class="request-skeleton-row" aria-hidden="true">
      {#each columns as column (column)}
        <span class="request-skeleton-block {column}"></span>
      {/each}
    </div>
  {/each}
</div>

<style>
  .request-skeleton {
    display: flex;
    flex-direction: column;
    gap: 1px;
    padding: 8px 0;
    border: 2px solid var(--ink);
    border-radius: 12px;
    background: #ffffff;
    box-shadow: 3px 3px 0 var(--ink);
  }

  .request-skeleton-row {
    display: grid;
    grid-template-columns:
      minmax(56px, 0.8fr) minmax(48px, 0.7fr) minmax(96px, 1.5fr) minmax(48px, 0.8fr)
      minmax(48px, 0.8fr) minmax(52px, 0.7fr) minmax(72px, 1fr) minmax(44px, 0.6fr);
    align-items: center;
    gap: 12px;
    height: 50px;
    padding: 0 14px;
    border-bottom: 1px solid #f1f2f6;
  }

  .request-skeleton-block {
    height: 10px;
    border-radius: 999px;
    background: linear-gradient(90deg, #f1f2f6 25%, #e4e5ee 37%, #f1f2f6 63%);
    background-size: 400% 100%;
    animation: request-skeleton-shimmer 1.4s ease infinite;
  }

  .request-skeleton-block.model {
    height: 12px;
  }

  .request-skeleton-block.status {
    height: 18px;
  }

  @keyframes request-skeleton-shimmer {
    0% {
      background-position: 100% 50%;
    }
    100% {
      background-position: 0 50%;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .request-skeleton-block {
      animation: none;
    }
  }

  :global(:root[data-theme='dark']) .request-skeleton {
    border-color: rgba(255, 255, 255, 0.08);
    background: #181926;
    box-shadow: 0 4px 20px rgba(0, 0, 0, 0.35);
  }

  :global(:root[data-theme='dark']) .request-skeleton-row {
    border-bottom-color: #222436;
  }

  :global(:root[data-theme='dark']) .request-skeleton-block {
    background: linear-gradient(90deg, #20223240 25%, #282a3c80 37%, #20223240 63%);
    background-color: #222436;
    background-size: 400% 100%;
  }

  @media (max-width: 650px) {
    .request-skeleton {
      background: transparent;
      border: none;
      box-shadow: none;
      gap: 10px;
      padding: 0;
    }
    .request-skeleton-row {
      height: auto;
      padding: 14px;
      border: 2px solid var(--ink);
      border-radius: 14px;
      background: #ffffff;
      box-shadow: 3px 3px 0 var(--ink);
      grid-template-columns: 1fr auto;
      grid-template-areas:
        'model status'
        'provider duration'
        'key time'
        'tokens tokens'
        'alias alias';
    }
    .request-skeleton-block.time {
      grid-area: time;
      justify-self: end;
      width: 60px;
    }
    .request-skeleton-block.key {
      grid-area: key;
      width: 72px;
    }
    .request-skeleton-block.model {
      grid-area: model;
      width: 60%;
    }
    .request-skeleton-block.alias {
      display: none;
    }
    .request-skeleton-block.provider {
      grid-area: provider;
      width: 40%;
    }
    .request-skeleton-block.duration {
      grid-area: duration;
      justify-self: end;
      width: 48px;
    }
    .request-skeleton-block.tokens {
      grid-area: tokens;
      width: 45%;
    }
    .request-skeleton-block.status {
      grid-area: status;
      justify-self: end;
      width: 56px;
    }
    :global(:root[data-theme='dark']) .request-skeleton-row {
      background: #181926;
      border-color: #202131;
    }
  }
</style>
