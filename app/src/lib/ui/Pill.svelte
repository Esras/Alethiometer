<script lang="ts">
  import type { Pill } from '../types';
  let { pill }: { pill: Pill } = $props();
</script>

<span class="pill {pill.weight}">{pill.label}</span>
