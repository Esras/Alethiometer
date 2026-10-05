// The ⌘ shortcuts take ⌘ on the Mac and Ctrl elsewhere, never both, so ⌃k
// isn't ⌘K on the Mac (lib/keys.svelte.ts mod()).
async function asMac(t) {
  await t.page.addInitScript(() => Object.defineProperty(Navigator.prototype, 'platform', { get: () => 'MacIntel' }));
  await t.page.reload();
  await t.page.waitForTimeout(1200);
}
const palette = (t) => t.page.locator('.palette input').first();

export const checks = {
  'Linux: Ctrl+K opens the palette': async (t) => {
    await t.steps('Control+k');
    await t.visible(palette(t), 'Ctrl+K did nothing');
  },
  'Mac: ⌃k does nothing, ⌘K opens the palette': async (t) => {
    await asMac(t);
    await t.steps('Control+k');
    await t.gone(palette(t), '⌃k opened the palette');
    await t.steps('Meta+k');
    await t.visible(palette(t), '⌘K did nothing');
  },
  "Mac: ⌘2 switches to Priorities, ⌃2 doesn't": async (t) => {
    await asMac(t);
    const tab = () => t.page.locator('.tab.on').first().innerText();
    await t.steps('Control+2');
    t.ok(!/Priorities/.test(await tab()), '⌃2 switched to Priorities');
    await t.steps('Meta+2');
    t.ok(/Priorities/.test(await tab()), `⌘2 did nothing (on ${await tab()})`);
  },
};
