<script lang="ts">
  export let title: string;
  export let subtitle: string;
  export let tr: (key: string) => string;
  export let kicker: string = 'EXOROUTE CONTROL PLANE';
  export let compact = false;
</script>

<div class="page-heading" class:compact>
  <div>
    <div class="eyebrow"><span class="eyebrow-line"></span>{tr(kicker)}</div>
    <h1>{title}</h1>
    <p>{subtitle}</p>
  </div>
  <div class="heading-actions"><slot /></div>
</div>

