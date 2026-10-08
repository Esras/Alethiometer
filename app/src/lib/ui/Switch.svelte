<script lang="ts">
  // An on/off switch (Settings).
  let {
    on,
    label,
    disabled = false,
    onclick,
  }: { on: boolean; label: string; disabled?: boolean; onclick: () => void } = $props();
</script>

<button class="switch" class:on {disabled} {onclick} aria-label={label} aria-pressed={on}><span></span></button>

<style>
  .switch {
    width: 34px;
    height: 20px;
    border-radius: var(--radius-full);
    background: var(--border-strong);
    position: relative;
    transition: background 120ms;
  }
  .switch span {
    position: absolute;
    top: 3px;
    left: 3px;
    width: 14px;
    height: 14px;
    border-radius: 50%;
    background: var(--bg-app);
    transition: transform 120ms;
  }
  .switch.on {
    background: var(--aqua);
  }
  .switch.on span {
    transform: translateX(14px);
  }
</style>
