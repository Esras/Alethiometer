// Incoming's split review: dismissing every message of the row it was opened
// on makes the row leave the list. The keys must come back to the list (they
// used to go to a review that was no longer there).
// Runs on the sample data's #acme-incident signal, so run it on a fresh data dir.

const selected = (t) => t.page.locator('.sig.sel').first().innerText();

export const checks = {
  'dismissing the whole split signal in the review hands the keys back': async (t) => {
    await t.steps('j', 's');
    t.ok((await t.page.locator('.groups').count()) === 1, 's did not open the review');
    await t.steps(...Array(10).fill('Shift+J'), 'd');
    const before = await selected(t);
    await t.steps('j');
    t.ok((await selected(t)) !== before, 'j does nothing after the review closed under the user');
  },
};
