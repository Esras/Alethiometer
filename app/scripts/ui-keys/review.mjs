// Incoming's split review (incoming/SplitReview): Incoming's own keys must come
// back however the review ends, including when the reviewed signal leaves the
// list under it (every message dismissed, or the signal dismissed).

// The DM with Jordan is the sample data's split Slack thread; j selects it.
const openReview = (t) => t.steps('j', 's');
const sel = (t) => t.page.locator('.sig.sel .title').first().innerText();

export const checks = {
  'Escape ends the review and j moves again': async (t) => {
    await openReview(t);
    await t.visible(t.page.getByText('By topic'));
    await t.steps('Escape');
    await t.gone(t.page.getByText('By topic'));
    const before = await sel(t);
    await t.steps('j');
    t.ok((await sel(t)) !== before, 'j did not move after Escape');
  },
  'Dismissing every message ends the review and the list keys work again': async (t) => {
    await openReview(t);
    await t.visible(t.page.getByText('By topic'));
    // d dismisses the focused group; enough presses dismiss them all.
    for (let i = 0; i < 12; i++) await t.steps('d', 'j');
    await t.page.waitForTimeout(1500);
    await t.steps('Escape');
    await t.page.waitForTimeout(500);
    await t.gone(t.page.getByText('By topic'));
    // The j presses above may already have reached the last signal once the
    // review ended, so move up.
    const before = await sel(t).catch(() => '');
    await t.steps('k');
    await t.page.waitForTimeout(300);
    t.ok((await sel(t).catch(() => '')) !== before, 'k did not move: Incoming still thinks a review is open');
  },
};
