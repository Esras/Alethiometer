// The behaviour the five pickers share (topic, person, category, notes, font);
// each keeps its own markup and CSS.
//
//   const p = new Picker({ rows: () => matches.length, pick: (i) => …, onclose });
//   <div class="picker" use:p.frame>
//     <input use:p.field bind:value={p.q} />
//     <button class:on={i === p.sel} {...p.row(i)}>
//
//   Source:  the site derives its own list from `p.q`, or gives `fetch`: then
//            `p.result` holds the latest reply, with one debounce and a guard
//            against stale replies.
//   Cursor:  `p.sel`, over `rows()` rows. ↑ ↓ stop at the ends (no
//            wrap). It goes back to the top when the query changes or a reply
//            arrives. A site with a create row counts it in `rows`, or answers
//            `pick(i)` for the index past the list.
//   Keys:    one modal entry on the key stack (like lib/ui/Modal): ↵ picks,
//            Esc closes, `keys` first for the site's own (notes: Tab), and
//            what is left doesn't reach the view underneath.
//   Mouse:   a row moves the cursor when the pointer moves and picks on
//            mousedown. A mousedown inside never takes the focus off the field.
//   Close:   Esc, or the field losing focus (a click elsewhere, Tab, the
//            window going away). This is the one outside-click rule rather
//            than lib/ui/popover.ts: that would need the trigger left out, and
//            a picker's field holds the focus the whole time it's open.
//
// Differences between the sites that stay as options or in their own CSS. To
// settle in the visible pass, with the design system's one picker:
//   keys      notes' Tab steps the cursor
//   active    FontPick stays mounted while closed
//   debounce  80ms for the async sources (notes and person)
//   create    TopicPicker's create row isn't a cursor row (↵ with no match
//             creates); PersonPicker's is the last one
//   CategoryPicker's `placement` prop, and every frame, cursor edge and row
//   look, which stay in each site's markup and CSS
import { tick } from 'svelte';
import { on } from 'svelte/events';
import { mod, pointerMoved, pushKeys, type KeyHandler } from '../keys.svelte';
import { listNav, moveSel } from './listNav';

export type PickerOpts<R> = {
  /** How many rows the cursor can sit on. */
  rows: () => number;
  /** ↵ or a click on row `i` (for ↵ that is the cursor, which may be past the list). */
  pick: (i: number) => void;
  onclose: () => void;
  /** The site's own keys, before ↵ / Esc / ↑↓. Return true when it took the key. */
  keys?: KeyHandler;
  /** For a picker that stays mounted while closed (FontPick). */
  active?: () => boolean;
  /** The starting query. */
  q?: string;
  /** An async source: runs on each query, after `debounce` ms, but at once for the first one and for an empty query. */
  fetch?: (q: string) => Promise<R>;
  initial?: R;
  debounce?: number;
};

export class Picker<R = undefined> {
  q = $state('');
  sel = $state(0);
  result = $state() as R;
  private o: PickerOpts<R>;
  private input?: HTMLInputElement;
  private root?: HTMLElement;

  constructor(o: PickerOpts<R>) {
    this.o = o;
    this.q = o.q ?? '';
    this.result = o.initial as R;

    $effect(() => {
      if (o.active && !o.active()) return;
      return pushKeys((e) => this.key(e), { modal: true });
    });

    const search = o.fetch;
    if (search) {
      let first = true;
      let latest = 0;
      $effect(() => {
        const query = this.q;
        const timer = setTimeout(
          async () => {
            const mine = ++latest;
            const r = await search(query);
            if (mine !== latest || query !== this.q) return;
            this.result = r;
            this.sel = 0;
          },
          first || !query ? 0 : (o.debounce ?? 80),
        );
        first = false;
        return () => clearTimeout(timer);
      });
    }
  }

  private key(e: KeyboardEvent): boolean {
    if (this.o.keys?.(e)) return true;
    if (e.key === 'Escape') return this.o.onclose(), true;
    const d = listNav(e);
    if (d) return this.move(d), true;
    if (e.key === 'Enter' && !mod(e)) return this.o.pick(this.sel), true;
    return false;
  }

  /** Step the cursor and keep its row in view. */
  move(d: number) {
    this.sel = moveSel(this.sel, d, this.o.rows());
    void tick().then(() => this.root?.querySelector(`[data-row="${this.sel}"]`)?.scrollIntoView({ block: 'nearest' }));
  }

  /** On the filter input: takes the focus, sends the cursor back to the top as you type, and closes when it loses focus. */
  field = (node: HTMLInputElement) => {
    this.input = node;
    const typed = () => (this.sel = 0);
    const blur = () => this.o.onclose();
    // `on`, not addEventListener: it runs the handler outside the reactive
    // context, and removing the focused field fires blur, which closes us.
    const off = [on(node, 'input', typed), on(node, 'blur', blur)];
    node.focus();
    return { destroy: () => off.forEach((f) => f()) };
  };

  /** On the picker's outer element: a mousedown anywhere in it but the field keeps the focus in the field. */
  frame = (node: HTMLElement) => {
    this.root = node;
    const down = (e: MouseEvent) => {
      if (e.target !== this.input) e.preventDefault();
    };
    return { destroy: on(node, 'mousedown', down) };
  };

  /** Spread on row `i`: the pointer moves the cursor to it, and mousedown picks it. */
  row(i: number) {
    return {
      'data-row': i,
      onmousedown: this.down(() => this.o.pick(i)),
      onmousemove: (e: MouseEvent) => pointerMoved(e) && (this.sel = i),
    };
  }

  /** A mousedown handler for what isn't a cursor row (a create row). */
  down(fn: () => void) {
    return (e: MouseEvent) => {
      e.preventDefault();
      fn();
    };
  }
}
