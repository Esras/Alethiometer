// The Agents tree's right-click menu (Agents Archive 05): its items act on the row it opened on.
export const checks = {
  'agents menu: Archive session opens the confirm': async (t) => {
    await t.steps('Control+7');
    await t.page.locator('.tree .row', { hasText: 'migration' }).first().click({ button: 'right' });
    await t.visible(t.page.getByRole('menu'));
    await t.page.getByRole('menuitem', { name: /^Archive/ }).click();
    await t.page.waitForTimeout(400);
    await t.gone(t.page.getByRole('menu'), 'the menu stayed open');
    await t.visible(t.page.getByRole('dialog', { name: 'Archive' }), 'no confirm opened');
  },
};
