// Screenshot the UI in browser mode (devserver + vite must be running).
// usage: [ALETHIOMETER_UI_URL=http://localhost:1420/] node scripts/shot.mjs <out.png> [tab] [keys...]
import { createRequire } from 'node:module';
const require = createRequire(import.meta.url);
let pw;
try { pw = require('playwright'); } catch { pw = require(require('node:child_process').execSync('npm root -g').toString().trim() + '/playwright'); }
const [out, tab = 'incoming', ...keys] = process.argv.slice(2);
const browser = await pw.chromium.launch();
const page = await browser.newPage({ viewport: { width: 1320, height: 880 }, deviceScaleFactor: 1 });
page.on('console', (m) => m.type() === 'error' && console.error('console:', m.text()));
page.on('pageerror', (e) => console.error('pageerror:', e.message));
await page.goto(process.env.ALETHIOMETER_UI_URL ?? 'http://localhost:1420/');
await page.waitForTimeout(800);
const idx = ['incoming', 'priorities', 'topics', 'notes', 'reflect'].indexOf(tab) + 1;
await page.keyboard.press(`Control+${idx}`);
await page.waitForTimeout(500);
for (const k of keys) { await page.keyboard.press(k); await page.waitForTimeout(250); }
await page.screenshot({ path: out });
await browser.close();
