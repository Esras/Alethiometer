// Pixel-diff each scene between the ref's UI and this checkout's, both started
// by scripts/ui-diff.sh (same UI_DIFF_SLOT). Each scene is a fresh page with
// the same clock and the same key presses; any differing pixel is a DIFF, and
// the diff image is left next to the two shots.
// usage: [UI_DIFF_SLOT=n] [UI_DIFF_SHARDS=n] node scripts/ui-diff.mjs [scene…]   (shots in <work>/<slot>/out)
// With UI_DIFF_SHARDS=n (default: what ui-diff.sh started, from <work>/<slot>/shards)
// the scenes are split into n contiguous chunks that run at once, each on its own pair
// of servers (ports in ui-diff.sh's header). The scenes that change the sample data
// (`// Done and dismissed`) always stay in the last chunk, in order. Each line is
// printed when its scene finishes, so with n > 1 not in scene order; the summary and the
// exit code are the same, and when scenes differ they are listed again in scene order.
// Needs Playwright (global is fine) and ImageMagick's `compare`.
import { createRequire } from 'node:module';
import { execSync, execFile } from 'node:child_process';
import { promisify } from 'node:util';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

const require = createRequire(import.meta.url);
let pw;
try {
  pw = require('playwright');
} catch {
  pw = require(execSync('npm root -g').toString().trim() + '/playwright');
}
// Same slot as scripts/ui-diff.sh: its ports and folder.
const SLOT = Number(process.env.UI_DIFF_SLOT ?? 0);
// [B, A] ports of shard s; shard 0 is the slot's own pair.
const shardPorts = (s) => {
  if (s > 0) return [2000 + 100 * SLOT + 10 * s, 2001 + 100 * SLOT + 10 * s];
  return SLOT === 0 ? [1420, 1430] : [1600 + 10 * SLOT, 1601 + 10 * SLOT];
};
const WORK = path.join(process.env.UI_DIFF_DIR ?? path.join(process.env.TMPDIR ?? os.tmpdir(), 'alethiometer-ui-diff'), String(SLOT));
// How many shards ui-diff.sh started (1 if it didn't say); UI_DIFF_SHARDS may use fewer.
const STARTED = Number(fs.existsSync(path.join(WORK, 'shards')) ? fs.readFileSync(path.join(WORK, 'shards'), 'utf8') : 1);
const SHARDS = Number(process.env.UI_DIFF_SHARDS ?? STARTED);
if (!Number.isInteger(SHARDS) || SHARDS < 1 || SHARDS > STARTED) {
  throw new Error(`UI_DIFF_SHARDS=${process.env.UI_DIFF_SHARDS}: ui-diff.sh started ${STARTED} shard(s) for slot ${SLOT}`);
}
const OUT = path.join(WORK, 'out');
fs.mkdirSync(OUT, { recursive: true });

// Deterministic raster: without these, antialiasing varies by a pixel or two
// between two loads of the same page.
const ARGS = ['--deterministic-mode', '--disable-gpu', '--force-color-profile=srgb', '--disable-partial-raster', '--disable-lcd-text'];
const executablePath = fs.existsSync('/opt/pw-browsers/chromium-1194/chrome-linux/chrome') ? '/opt/pw-browsers/chromium-1194/chrome-linux/chrome' : undefined;
const browser = await pw.chromium.launch({ executablePath, args: ARGS });
const FIXED = new Date();
// Give Vite's watcher time to see edits saved just before this run.
await new Promise((r) => setTimeout(r, 2500));

// Key presses, or { click: title }, { clickText }, { exact: text, nth }, { button: name }, { type: text }, { placeholder: text }.
const scenes = {
  incoming: [],
  'incoming-expanded': ['j', 'Enter'],
  'incoming-split': ['j', 's'],
  'incoming-todo': ['n'],
  'incoming-personpicker': ['Enter', { exact: 'Marco R.' }],
  // The person picker's rows (avatars): Marco has no note, so the field is filled with a
  // letter that matches people. Kept beside incoming-personpicker: run after the empty-* scenes,
  // its click on Marco failed and it shot nothing.
  'incoming-personpicker-hits': ['Enter', { exact: 'Marco R.' }, 'Control+a', { type: 'a' }],
  'incoming-topicpicker': ['t'],
  unsorted: [']', ']', ']', ']', ']'],
  'unsorted-expanded': [']', ']', ']', ']', ']', 'Enter'],
  'incoming-agents': [{ exact: 'Agents waiting' }],
  'incoming-agents-open': [{ exact: 'Agents waiting' }, 'Enter'],
  'incoming-agents-approval': [{ exact: 'Agents waiting' }, 'j', 'Enter'],
  priorities: ['Control+2'],
  // The cursor two rows down, focus on Neglect: the splines and the open-row grey.
  'priorities-moved': ['Control+2', 'j', 'j', ']', ']'],
  // A topic picked (count badge, "n of m", Clear) and the Factor menu open.
  'priorities-filter': ['Control+2', { exact: 'Topic' }, { exact: 'Webhook reliability', nth: 0 }, { exact: 'Factor' }],
  'topics-index': ['Control+3'],
  'topics-catpicker': ['Control+3', 'c'],
  'topics-categories': ['Control+3', { click: 'Manage categories' }],
  'topic-page': ['Control+3', 'Enter'],
  todos: ['Control+4'],
  'todos-new': ['Control+4', 'n'],
  notes: ['Control+5'],
  reflect: ['Control+6'],
  'reflect-week': ['Control+6', 'w'],
  agents: ['Control+7'],
  'agents-approval': ['Control+7', { exact: 'fake-agent' }],
  'agents-ask': ['Control+7', { exact: 'fakeask-agent' }],
  'agents-new': ['Control+7', { button: 'New agent' }],
  'agents-keys': ['Control+7', 'Shift+?'],
  palette: ['Control+k', { type: 'acme' }],
  search: ['Control+k', { type: 'acme' }, 'Control+Enter'],
  assistant: ['Control+i'],
  sync: [{ click: 'Sources and writes' }],
  daystrip: [{ clickText: 'Eng leads' }],
  settings: ['Control+,'],
  'settings-categories': ['Control+,', '2'],
  'settings-people': ['Control+,', '3'],
  'settings-sources': ['Control+,', '4'],
  'settings-source': ['Control+,', '4', 'j', 'Enter'],
  'settings-ai': ['Control+,', '5'],
  'settings-agents': ['Control+,', '6'],
  // Not Developer (7): it shows each dev server's own data directory.
  'settings-find': ['Control+,', '/', { type: 'font' }],
  // Popover
  // The Composer's menu is the @ completion in a Scion room, so this scene needs
  // the fake hub (docs/agents.md) as a scion source on both dev servers.
  'daystrip-popover': [{ exact: 'Life' }],
  'composer-menu': ['Control+7', { exact: 'migration', nth: 1 }, 'Tab', 'Tab', { type: '@' }],
  // Small moves
  'incoming-split-dismissed': ['j', 's', 'd'],
  // Picker
  // Notes saves what you type, so these open the picker on the last line as it is.
  'notes-picker': ['Control+5', 'Enter', 'Tab'],
  'notes-picker-moved': ['Control+5', 'Enter', 'Tab', 'ArrowDown', 'ArrowDown'],
  'settings-fontpick': ['Control+,', { exact: 'System', nth: 0 }],
  'settings-fontpick-typed': ['Control+,', { exact: 'System', nth: 0 }, { type: 'dej' }, 'ArrowDown'],
  'incoming-topicpicker-typed': ['t', { type: 'acme' }, 'ArrowDown'],
  'incoming-personpicker-typed': ['Enter', { exact: 'Marco R.' }, 'ArrowDown'],
  'topics-catpicker-moved': ['Control+3', 'c', 'ArrowDown', 'ArrowDown'],
  // Buttons
  // The reply toggle is .btn.on. The Assistant's Sources button (.btn.on too) only
  // shows after a Glean answer, which the sample data can't give.
  'agents-ask-reply': ['Control+7', { exact: 'fakeask-agent' }, 'r'],
  // Counts and chips
  // The palette opened from a topic shows the anchor chip.
  'palette-in-topic': ['Control+3', 'Enter', 'Control+k'],
  // Inputs
  // A focused field is the only aqua one: these scenes focus the filters, the new-name
  // field, Reflect's ask box and the agents' reply and deny fields. The todo ones make
  // a todo first, so they come last of this group.
  'todos-filter-focus': ['Control+4', '/'],
  'topics-filter-focus': ['Control+3', '/'],
  'topics-naming': ['Control+3', 'n'],
  'agents-filter-focus': ['Control+7', '/'],
  'reflect-ask-focus': ['Control+6', 'a'],
  'agents-deny': ['Control+7', { exact: 'fake-agent' }, '3'],
  'incoming-agents-reply': [{ exact: 'Agents waiting' }, 'j', 'Enter', 'r'],
  'daystrip-buffer': [{ exact: 'Life' }, 'b'],
  'settings-edit-number': ['Control+,', 'Enter'],
  'todos-estimate': ['n', { type: 'Write the RCA' }, 'Control+Enter', 'c', 'Control+4', 'e'],
  // A local todo's scheduled day (the first row, due earliest, is local in the sample data).
  'todos-sched': ['Control+4', 'd'],
  // Key hints
  // Keys written inside sentences and legends (decision 9): the legend under an
  // expanded Search result and Topics' chronology, and the note pane's editing line.
  'search-expanded': ['Control+k', { type: 'acme' }, 'Control+Enter', ' '],
  'topic-expanded': ['Control+3', 'Enter', 'j', 'j', 'Enter'],
  'topic-note-edit': ['Control+3', 'Enter', 'e'],
  // Labels
  // Chronology's Now rule only shows on a topic with open entries (Acme outage has two
  // meetings today). Not covered: SlackReview's column header (the sample data has no
  // Slack source) and the approval's "If planted" label (Explained needs a Bedrock answer).
  'topic-page-now': ['Control+3', 'j', 'Enter'],
  // Surfaces
  // Popovers, modals and insets (decision 5) that no scene above reaches: the topic
  // picker where it isn't docked (the palette's ⌘T, a new todo's Topic field), the
  // Topics index's docked picker and the topic page's category picker, the New agent
  // modal for a Scion room (it has the instructions inset), and an approval's command
  // block in Incoming. Not covered: the Nudge toast (it shows near the end of the
  // day, the clock is at 10:52) and Slack review (it needs a Slack source).
  'palette-link-topic': ['Control+k', { type: 'acme' }, 'ArrowDown', 'Alt+a', 'Control+t'],
  'todos-new-topic-picker': ['Control+4', 'n', { exact: 'No topic' }],
  'topics-index-merge-picker': ['Control+3', 'm'],
  'topic-page-catpicker': ['Control+3', 'Enter', 'c'],
  'incoming-agents-command': [{ exact: 'Agents waiting' }, { clickText: 'needs approval' }],
  'agents-new-scion': ['Control+7', { exact: 'migration', nth: 1 }, 'n'],
  // Colours
  // Error lines (decision 11): the assistant's failed ask is red (the dev server's
  // Bedrock rejects the keys, so the AI can't be reached); a todo's bad estimate is
  // orange (it needs you). Not covered: the countdown's red (at 10:52 the next block is
  // 7 minutes away, so the ladder reads yellow before and after), the Agents tree's host
  // line and Scion hub error, a failing source in the sync popover (the sample data has
  // none) and Notes' margin states.
  'assistant-ask-failed': ['Control+i', { placeholder: 'Ask, or @ to reference…' }, { type: 'What is blocking Acme?' }, 'Enter'],
  'todos-estimate-bad': ['n', { type: 'Write the RCA' }, 'Control+Enter', 'c', 'Control+4', 'e', { type: 'zzz' }, 'Enter'],
  // Empty states (decision 18): a filter nobody matches, the Archived topics, a topic with
  // nothing open and no note (the sixth has both), a day with no calendar, decisions or
  // carry rows. The existing agents scene has the Agents hello, todos the empty list and
  // sync the "No live sources" line. Not covered: Incoming's and Priorities' empty lists
  // (the sample data fills them), the topic picker's "No topic matches" (typing offers
  // Create instead), Slack review and the Assistant's Sources panel.
  'empty-search': ['Control+k', { type: 'zzqx' }, 'Control+Enter'],
  'empty-palette': ['Control+k', { type: 'zzqx' }],
  'empty-agents-tree': ['Control+7', '/', { type: 'zzqx' }],
  'empty-settings-find': ['Control+,', '/', { type: 'zzqx' }],
  'empty-catpicker': ['Control+3', 'c', { type: 'zzqx' }],
  'empty-fontpick': ['Control+,', { exact: 'System', nth: 0 }, { type: 'zzqx' }],
  'empty-topics-index': ['Control+3', { exact: 'Archived' }, '/', { type: 'zz' }],
  'empty-topic-page': ['Control+3', 'j', 'j', 'j', 'j', 'j', 'j', 'Enter'],
  'empty-reflect-day': ['Control+6', '[', 'T', 'D'],
  // People and reach-ins (decision 2, Phase 0-2 leftovers): Marco has no People note, so
  // his signal shows the New person strip; New agent on a Scion hub marks a name already
  // in the project ("planner") as taken. Not covered: Topics' People panel with an
  // unknown person who has an account (the sample data gives Marco none; checked by
  // patching the topic's response), and Glean's avatars (parked).
  'incoming-new-person': ['Enter'],
  'agents-new-scion-taken': ['Control+7', { exact: 'migration', nth: 1 }, 'n', { type: 'planner' }],
  // A pi-style gate on a long multi-line command (the fakepi agent): the approval keeps
  // the transcript's room, shows only the gate's `rule *` and scrolls Parsed and Explained
  // separately. The same at 1760×800 (VIEWPORT), where the card must shrink.
  'agents-approval-pi': ['Control+7', { exact: 'fakepi-agent' }],
  'agents-approval-pi-short': ['Control+7', { exact: 'fakepi-agent' }],
  // Done and dismissed
  // These change the sample data (a todo done, a signal dismissed, a merge staged), so
  // keep them last and run them after the others, or restart ui-diff.sh between runs.
  // Sharding keeps all of them in the last chunk: `todos-done` must stay the first of them.
  // Make two todos from signals (n, type, ⌘↵, confirm), then do the first row of the Todos tab.
  'todos-done': ['n', { type: 'Write the RCA' }, 'Control+Enter', 'c', 'n', { type: 'Reply to Dana' }, 'Control+Enter', 'c', 'Control+4', 'x', 'j'],
  'incoming-dismissed': ['j', 'd', 'j'],
  'incoming-dismissed-selected': ['j', 'd'],
  // The cursor's fill hides the hatching, so move off the dismissed message.
  'incoming-split-dismissed-away': ['j', 's', 'd', 'j'],
  // A topic's priority (Priorities): p on the Topics index sets the first topic's to H.
  'topics-priority': ['Control+3', 'p'],
  'topics-merged': ['Control+3', 'm', 'ArrowDown', 'Enter'],
};

// Waits until the page has been still for `quiet` ms: no DOM change, and no call to
// the server in flight when that time is up (live.wait, the live queries' long poll,
// is always open and doesn't count). Never longer than `max` ms, the old fixed
// waits. Uses the page's own timers, which page.clock leaves running.
const settle = (page, quiet, max) =>
  page.evaluate(
    ([quiet, max]) =>
      new Promise((done) => {
        let t;
        const finish = () => {
          obs.disconnect();
          clearTimeout(t);
          clearTimeout(cap);
          done();
        };
        const arm = () => {
          clearTimeout(t);
          t = setTimeout(() => (window.__inflight ? arm() : finish()), quiet);
        };
        const obs = new MutationObserver(arm);
        obs.observe(document, { subtree: true, childList: true, attributes: true, characterData: true });
        const cap = setTimeout(finish, max);
        arm();
      }),
    [quiet, max],
  );
// Counts the page's calls to the server in flight, for settle().
const COUNT_FETCH = `(() => {
  window.__inflight = 0;
  const f = window.fetch;
  window.fetch = (...a) => {
    if (String(a[0]).endsWith('/live.wait')) return f(...a);
    window.__inflight++;
    return f(...a).finally(() => window.__inflight--);
  };
})()`;

// A scene's window when it isn't the usual one.
const VIEWPORT = { 'agents-approval-pi-short': { width: 1760, height: 800 } };

async function shoot(base, steps, file, viewport = { width: 1760, height: 1175 }, fixed = FIXED) {
  const page = await browser.newPage({ viewport, deviceScaleFactor: 1 });
  await page.clock.setFixedTime(fixed);
  await page.addInitScript(COUNT_FETCH);
  const errs = [];
  page.on('pageerror', (e) => errs.push(e.message.split('\n')[0]));
  let lastStep = Date.now();
  await page.goto(base);
  await page.evaluate(() => document.fonts.ready);
  // A transition caught half-way is timing, not a change.
  await page.addStyleTag({ content: '*, *::before, *::after { transition: none !important; animation: none !important; }' });
  await settle(page, 400, 1500);
  for (const k of steps) {
    lastStep = Date.now();
    try {
      if (typeof k === 'string') await page.keyboard.press(k);
      else if (k.type) await page.keyboard.type(k.type, { delay: 30 });
      else if (k.click) await page.getByTitle(k.click).first().click({ timeout: 3000 });
      else if (k.clickText) await page.getByText(k.clickText).first().click({ timeout: 3000 });
      else if (k.exact) await page.getByText(k.exact, { exact: true }).nth(k.nth ?? 0).click({ timeout: 3000 });
      else if (k.placeholder) await page.getByPlaceholder(k.placeholder).first().click({ timeout: 3000 });
      else if (k.button) await page.getByRole('button', { name: k.button }).first().click({ timeout: 3000 });
    } catch {
      errs.push('step failed: ' + JSON.stringify(k));
    }
    await settle(page, 250, 450);
  }
  // Some views tick once a second: the Agents tree's ages jump from 5d to 6d at its first tick,
  // which reads the clock's offset. Shoot after that tick, not racing it: on a busy machine
  // (several shards at once) the shot lands right at it and the two sides split.
  await Promise.all([page.waitForTimeout(Math.max(0, 1100 - (Date.now() - lastStep))), settle(page, 600, 900)]);
  await settle(page, 150, 300);
  await page.mouse.move(1, viewport.height - 5); // no hover states
  await page.addStyleTag({ content: '*, *::before, *::after { caret-color: transparent !important; }' });
  await page.evaluate(() => document.fonts.ready);
  await page.screenshot({ path: file });
  await page.close();
  return errs;
}

// Async: a blocking compare would stall every other shard's browser commands.
const run = promisify(execFile);
async function differing(a, b, d) {
  try {
    await run('compare', ['-metric', 'AE', a, b, d]);
    return 0;
  } catch (e) {
    return Number(String(e.stderr).trim().split(/\s/)[0]);
  }
}

// The scenes under `// Done and dismissed` change the sample data: they stay last in a
// run and in the last chunk. (`todos-done` is the first of them.)
const names = process.argv.slice(2).length ? process.argv.slice(2) : Object.keys(scenes);
for (const name of names) if (!scenes[name]) throw new Error(`no scene "${name}"`);
const all = Object.keys(scenes);
const DONE = new Set(all.slice(all.indexOf('todos-done')));

// Roughly how long a scene takes, in seconds: the page load and settles, a settle per step,
// and 30 ms per typed character. Only for balancing the chunks.
const cost = (name) => 2.2 + scenes[name].reduce((t, k) => t + 0.35 + (k.type ? k.type.length * 0.03 : 0), 0);

// Contiguous chunks of about the same cost, with no cut after the first scene that changes
// the data, so that scene and the ones after it share the last chunk.
function chunked(names, n) {
  const firstDone = names.findIndex((x) => DONE.has(x));
  const last = firstDone < 0 ? names.length : firstDone;
  const total = names.reduce((t, x) => t + cost(x), 0);
  const cuts = [0];
  let sum = 0;
  for (let i = 0; i < names.length && cuts.length < n; i++) {
    if (sum >= (cuts.length * total) / n || i >= last) cuts.push(Math.min(i, last));
    sum += cost(names[i]);
  }
  cuts.push(names.length);
  return cuts.slice(0, -1).map((c, i) => names.slice(c, cuts[i + 1])).filter((c) => c.length);
}

// One scene on shard s: both sides at once, since they have servers of their own.
async function scene(name, s) {
  const [PB, PA] = shardPorts(s);
  const [a, b, d] = ['a', 'b', 'diff'].map((x) => path.join(OUT, `${name}.${x}.png`));
  let px = 0;
  let errs = [];
  // One retry: a scene that crosses a minute boundary between the two shots differs. So
  // can one with an age in seconds (the Agents tree): the two sides started their agents a
  // few ms apart, and a clock that sits right at a rounding edge reads "18s" on one and
  // "19s" on the other. The retry's clock is half a second later, which clears the edge.
  for (let attempt = 0; attempt < 2; attempt++) {
    const fixed = new Date(FIXED.getTime() + attempt * 500);
    const [ea, eb] = await Promise.all([
      shoot(`http://localhost:${PA}/`, scenes[name], a, VIEWPORT[name], fixed),
      shoot(`http://localhost:${PB}/`, scenes[name], b, VIEWPORT[name], fixed),
    ]);
    errs = [...new Set([...ea.map((x) => 'ref: ' + x), ...eb.map((x) => 'this: ' + x)])];
    px = await differing(a, b, d);
    if (px === 0) break;
  }
  if (!px) fs.rmSync(d, { force: true });
  const line = `${px ? 'DIFF' : 'same'} ${name}${px ? ` (${px} px)` : ''}${errs.length ? ' · ' + errs.join(' | ') : ''}`;
  console.log(line);
  return { px, line };
}

const chunks = chunked(names, SHARDS);
const results = new Map();
await Promise.all(
  chunks.map(async (chunk, s) => {
    for (const name of chunk) results.set(name, await scene(name, s));
  }),
);
const differ = names.filter((x) => results.get(x).px);
if (chunks.length > 1 && differ.length) {
  console.log('differing, in scene order:');
  for (const x of differ) console.log('  ' + results.get(x).line);
}
await browser.close();
const bad = differ.length;
console.log(bad ? `${bad} scene(s) differ; shots in ${OUT}` : 'all scenes identical');
process.exit(bad ? 1 : 0);
