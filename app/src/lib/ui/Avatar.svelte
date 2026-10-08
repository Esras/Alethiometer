<script lang="ts">
  // The round initials avatar that leads a row about a person (Topics' People
  // panel, the person picker, Glean's people). Never used in threads.
  // `lg` (--avatar-lg) is a person's card or page, `sm` (--avatar-sm) a picker
  // row. `px` is an escape hatch for Glean's 15px people, which are parked
  // (ui-plan decision 2); drop it when they move to `sm`.
  let { initials, color, size, px }: { initials: string; color: string; size: 'lg' | 'sm'; px?: number } = $props();
</script>

<span
  class="avatar {size}"
  style:background={color}
  style:width={px ? `${px}px` : undefined}
  style:height={px ? `${px}px` : undefined}>{initials}</span
>

<style>
  .avatar {
    flex: none;
    border-radius: 50%;
    display: flex;
    align-items: center;
    justify-content: center;
    font-weight: 600;
    color: var(--bg-app);
  }
  .lg {
    width: var(--avatar-lg);
    height: var(--avatar-lg);
    font-size: 0.7692rem;
  }
  .sm {
    width: var(--avatar-sm);
    height: var(--avatar-sm);
    font-size: 0.6923rem;
  }
</style>
