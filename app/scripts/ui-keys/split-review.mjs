// Incoming's split review: dismissing every message of the row it was opened
// on empties the row, which leaves the list. The review stays open on it
// until Esc or c (nothing moves in Incoming while you split), then the keys
// come back to the list.
// Runs on the sample data's #acme-incident signal, so run it on a fresh data dir.

const selected = (t) => t.page.locator('.sig.sel').first().innerText();

export const checks = {
  'Escape ends the review and j moves again': async (t) => {
    await t.steps('j', 's');
    await t.visible(t.page.getByText('By topic'));
    await t.steps('Escape');
    await t.gone(t.page.getByText('By topic'));
    const before = await selected(t);
    await t.steps('j');
    t.ok((await selected(t)) !== before, 'j did not move after Escape');
  },
  'dismissing the whole split signal keeps the review open until Escape': async (t) => {
    await t.steps('j', 's');
    t.ok((await t.page.locator('.groups').count()) === 1, 's did not open the review');
    await t.steps(...Array(10).fill('Shift+J'), 'd');
    await t.visible(t.page.getByText('By topic'), 'the review closed when its row emptied');
    await t.steps('Escape');
    await t.gone(t.page.getByText('By topic'));
    const before = await selected(t);
    await t.steps('j');
    t.ok((await selected(t)) !== before, 'j does nothing after the review closed');
  },
};
