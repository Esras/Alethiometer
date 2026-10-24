// Behaviour checks the pixel diff can't see: Escape, outside clicks, focus.
// Runs every check in scripts/ui-keys/*.mjs against this checkout's UI as
// started by scripts/ui-diff.sh (same UI_DIFF_SLOT).
// usage: [UI_DIFF_SLOT=n] node scripts/ui-keys.mjs [file-or-check…]
//
// A check file exports `checks`: { 'name': async (t) => { … } }, where t has
//   t.page                  a fresh page on the app, fonts loaded
//   t.steps(…steps)         key presses or { tab } / { click } / { clickText } / { exact } / { button } / { type }, as in ui-diff.mjs
//   t.visible(locator)      fails unless it's visible;  t.gone(locator) unless it isn't
//   t.focused(locator)      fails unless it holds focus
//   t.ok(cond, message)     fails unless cond
import { createRequire } from 'node:module';
import { execSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const require = createRequire(import.meta.url);
let pw;
try {
  pw = require('playwright');
} catch {
  pw = require(execSync('npm root -g').toString().trim() + '/playwright');
}
const SLOT = Number(process.env.UI_DIFF_SLOT ?? 0);
const BASE = `http://localhost:${SLOT === 0 ? 1420 : 1600 + 10 * SLOT}/`;
const DIR = path.join(path.dirname(fileURLToPath(import.meta.url)), 'ui-keys');
const executablePath = fs.existsSync('/opt/pw-browsers/chromium-1194/chrome-linux/chrome')
  ? '/opt/pw-browsers/chromium-1194/chrome-linux/chrome'
  : undefined;
const browser = await pw.chromium.launch({ executablePath });

const want = process.argv.slice(2);
const files = fs.existsSync(DIR)
  ? fs
      .readdirSync(DIR)
      .filter((f) => f.endsWith('.mjs'))
      .sort()
  : [];
let failed = 0;
let ran = 0;
for (const file of files) {
  const { checks } = await import(pathToFileURL(path.join(DIR, file)).href);
  for (const [name, fn] of Object.entries(checks)) {
    if (want.length && !want.includes(file.replace(/\.mjs$/, '')) && !want.includes(name)) continue;
    ran++;
    const page = await browser.newPage({ viewport: { width: 1760, height: 1175 } });
    const errs = [];
    page.on('pageerror', (e) => errs.push(e.message.split('\n')[0]));
    const t = {
      page,
      async steps(...steps) {
        for (const k of steps) {
          if (typeof k === 'string') await page.keyboard.press(k);
          else if (k.tab) {
            const i = await page
              .locator('nav.tabs .tab')
              .evaluateAll((els, l) => els.findIndex((e) => e.textContent.trim().startsWith(l)), k.tab);
            await page.keyboard.press(`Control+${i + 1}`);
          } else if (k.type) await page.keyboard.type(k.type, { delay: 30 });
          else if (k.click) await page.getByTitle(k.click).first().click({ timeout: 3000 });
          else if (k.clickText) await page.getByText(k.clickText).first().click({ timeout: 3000 });
          else if (k.exact)
            await page
              .getByText(k.exact, { exact: true })
              .nth(k.nth ?? 0)
              .click({ timeout: 3000 });
          else if (k.button) await page.getByRole('button', { name: k.button }).first().click({ timeout: 3000 });
          await page.waitForTimeout(300);
        }
      },
      async visible(loc, msg) {
        if (!(await loc.first().isVisible())) throw new Error(msg ?? `not visible: ${loc}`);
      },
      async gone(loc, msg) {
        if ((await loc.count()) && (await loc.first().isVisible())) throw new Error(msg ?? `still visible: ${loc}`);
      },
      async focused(loc, msg) {
        if (!(await loc.first().evaluate((el) => el === document.activeElement || el.contains(document.activeElement))))
          throw new Error(msg ?? `not focused: ${loc}`);
      },
      ok(cond, msg) {
        if (!cond) throw new Error(msg);
      },
    };
    try {
      await page.goto(BASE);
      await page.evaluate(() => document.fonts.ready);
      await page.waitForTimeout(1200);
      await fn(t);
      if (errs.length) throw new Error('page errors: ' + errs.join(' | '));
      console.log(`ok   ${file.replace(/\.mjs$/, '')} · ${name}`);
    } catch (e) {
      failed++;
      console.log(`FAIL ${file.replace(/\.mjs$/, '')} · ${name}: ${String(e.message).split('\n')[0]}`);
    }
    await page.close();
  }
}
await browser.close();
console.log(failed ? `${failed} of ${ran} failed` : `${ran} checks passed`);
process.exit(failed ? 1 : 0);
