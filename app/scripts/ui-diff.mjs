// Pixel-diff each scene between the ref's UI and this checkout's, both started
// by scripts/ui-diff.sh (same UI_DIFF_SLOT). Each scene is a fresh page with
// the same clock and the same key presses; any differing pixel is a DIFF, and
// the diff image is left next to the two shots.
// usage: [UI_DIFF_SLOT=n] node scripts/ui-diff.mjs [scene…]   (shots in <work>/<slot>/out)
// Needs Playwright (global is fine) and ImageMagick's `compare`.
import { createRequire } from 'node:module';
import { execSync, execFileSync } from 'node:child_process';
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
const [PB, PA] = SLOT === 0 ? [1420, 1430] : [1600 + 10 * SLOT, 1601 + 10 * SLOT];
const WORK = path.join(process.env.UI_DIFF_DIR ?? path.join(process.env.TMPDIR ?? os.tmpdir(), 'alethiometer-ui-diff'), String(SLOT));
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
  'incoming-topicpicker': ['t'],
  unsorted: [']', ']', ']', ']', ']'],
  'unsorted-expanded': [']', ']', ']', ']', ']', 'Enter'],
  'incoming-agents': [{ exact: 'Agents waiting' }],
  'incoming-agents-open': [{ exact: 'Agents waiting' }, 'Enter'],
  'incoming-agents-approval': [{ exact: 'Agents waiting' }, 'j', 'Enter'],
  priorities: ['Control+2'],
  'priorities-peek': ['Control+2', 'p'],
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
  // Priorities' compact quadrants are chips; the palette opened from a topic shows
  // the anchor chip. The Todos Jira status chip needs a Jira todo the sample data lacks.
  'priorities-compact': ['Control+2', 'v'],
  'palette-in-topic': ['Control+3', 'Enter', 'Control+k'],
  // Inputs
  // A focused field is the only aqua one: these scenes focus the filters, the new-name
  // field, Reflect's ask box and the agents' reply and deny fields. The todo ones make
  // todos first (the sample data has none), so they come last of this group.
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
  // Done and dismissed
  // These change the sample data (a todo done, a signal dismissed, a merge staged), so
  // keep them last and run them after the others, or restart ui-diff.sh between runs.
  // The sample data has no todos: make two from signals (n, type, ⌘↵, confirm), then do the first.
  'todos-done': ['n', { type: 'Write the RCA' }, 'Control+Enter', 'c', 'n', { type: 'Reply to Dana' }, 'Control+Enter', 'c', 'Control+4', 'x', 'j'],
  'incoming-dismissed': ['j', 'd', 'j'],
  'incoming-dismissed-selected': ['j', 'd'],
  // The cursor's fill hides the hatching, so move off the dismissed message.
  'incoming-split-dismissed-away': ['j', 's', 'd', 'j'],
  'topics-merged': ['Control+3', 'm', 'ArrowDown', 'Enter'],
};

async function shoot(base, steps, file) {
  const page = await browser.newPage({ viewport: { width: 1760, height: 1175 }, deviceScaleFactor: 1 });
  await page.clock.setFixedTime(FIXED);
  const errs = [];
  page.on('pageerror', (e) => errs.push(e.message.split('\n')[0]));
  await page.goto(base);
  await page.evaluate(() => document.fonts.ready);
  // A transition caught half-way is timing, not a change.
  await page.addStyleTag({ content: '*, *::before, *::after { transition: none !important; animation: none !important; }' });
  await page.waitForTimeout(1500);
  for (const k of steps) {
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
    await page.waitForTimeout(450);
  }
  await page.waitForTimeout(600);
  await page.mouse.move(1, 1170); // no hover states
  await page.addStyleTag({ content: '*, *::before, *::after { caret-color: transparent !important; }' });
  await page.evaluate(() => document.fonts.ready);
  await page.waitForTimeout(300);
  await page.screenshot({ path: file });
  await page.close();
  return errs;
}

function differing(a, b, d) {
  try {
    execFileSync('compare', ['-metric', 'AE', a, b, d], { stdio: 'pipe' });
    return 0;
  } catch (e) {
    return Number(String(e.stderr).trim().split(/\s/)[0]);
  }
}

const names = process.argv.slice(2).length ? process.argv.slice(2) : Object.keys(scenes);
let bad = 0;
for (const name of names) {
  if (!scenes[name]) throw new Error(`no scene "${name}"`);
  const [a, b, d] = ['a', 'b', 'diff'].map((x) => path.join(OUT, `${name}.${x}.png`));
  let px = 0;
  let errs = [];
  // One retry: a scene that crosses a minute boundary between the two shots differs.
  for (let attempt = 0; attempt < 2; attempt++) {
    const ea = await shoot(`http://localhost:${PA}/`, scenes[name], a);
    const eb = await shoot(`http://localhost:${PB}/`, scenes[name], b);
    errs = [...new Set([...ea.map((x) => 'ref: ' + x), ...eb.map((x) => 'this: ' + x)])];
    px = differing(a, b, d);
    if (px === 0) break;
  }
  if (px) bad++;
  else fs.rmSync(d, { force: true });
  console.log(`${px ? 'DIFF' : 'same'} ${name}${px ? ` (${px} px)` : ''}${errs.length ? ' · ' + errs.join(' | ') : ''}`);
}
await browser.close();
console.log(bad ? `${bad} scene(s) differ; shots in ${OUT}` : 'all scenes identical');
process.exit(bad ? 1 : 0);
