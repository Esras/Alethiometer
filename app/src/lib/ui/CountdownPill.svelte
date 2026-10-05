<script lang="ts">
  // The next-meeting pill: a coloured dot, the title and "in N min" in the same colour.
  let { title, left, color }: { title: string | null; left: number; color: string } = $props();
</script>

<span class="pill-cd">
  <span class="dot" style:background={color}></span>
  {title}
  <span class="tnum" style:color style:font-weight="500">in {left} min</span>
</span>

<style>
  .pill-cd {
    display: flex;
    gap: 8px;
    align-items: center;
    padding: 4px 12px;
    border-radius: var(--radius-full);
    border: 1px solid var(--border);
    background: var(--bg-panel);
    font-size: 0.9231rem;
    color: var(--tx-muted);
    white-space: nowrap;
  }
  .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
  }
</style>
