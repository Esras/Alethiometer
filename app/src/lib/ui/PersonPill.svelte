<script lang="ts">
  // A person named inside text (a Slack, GitHub or Jira mention, an @name in an answer, a [[People/…]] link in
  // a note): their name in their person colour on a tint of it, the way PersonName shows them as an author.
  // Someone without a People note gets PersonName's dotted underline. `title`: what the text said.
  import { personColor, personTint, type Who } from '../people';

  let { who, name, title }: { who: Pick<Who, 'color' | 'known'> & Partial<Who>; name: string; title?: string } =
    $props();
</script>

<span
  class="person-pill"
  class:unknown={!who.known}
  style:color={personColor(who.color)}
  style:background={personTint(who.color, 0.14)}
  title={title && title !== name ? `${name} · ${title}` : name}>{name}</span
>

<style>
  .person-pill {
    padding: 0 4px;
    border-radius: var(--radius-sm);
    font-weight: 500;
    white-space: nowrap;
    box-decoration-break: clone;
  }
  .person-pill.unknown {
    text-decoration: underline dotted;
    text-underline-offset: 3px;
  }
</style>
