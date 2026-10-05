// Pixel-diff each scene between the ref's UI (:1430) and this checkout's
// (:1420), both started by scripts/ui-diff.sh. Each scene is a fresh page with
// the same clock and the same key presses; any differing pixel is a DIFF, and
// the diff image is left next to the two shots.
// usage: node scripts/ui-diff.mjs [scene…]   (shots in $UI_DIFF_DIR/out)
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
const WORK = process.env.UI_DIFF_DIR ?? path.join(process.env.TMPDIR ?? os.tmpdir(), 'alethiometer-ui-diff');
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

// Key presses, or { click: title }, { clickText }, { exact: text, nth }, { button: name }, { type: text }.
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
    const ea = await shoot('http://localhost:1430/', scenes[name], a);
    const eb = await shoot('http://localhost:1420/', scenes[name], b);
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
