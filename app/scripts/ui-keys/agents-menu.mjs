// The Agents tree's right-click menu (Agents Archive 05): its items act on the row it opened on.
// In the fake Hub's #migration room, reviewer is idle and planner and schema-diff are busy, so
// reviewer can be archived and the room can't until they stop.
export const checks = {
  'agents menu: Archive session opens the confirm': async (t) => {
    await t.steps('Control+7');
    await t.page.locator('.tree .row', { hasText: 'reviewer' }).first().click({ button: 'right' });
    await t.visible(t.page.getByRole('menu'));
    await t.page.getByRole('menuitem', { name: /^Archive session/ }).click({ timeout: 3000 });
    await t.page.waitForTimeout(400);
    await t.gone(t.page.getByRole('menu'), 'the menu stayed open');
    await t.visible(t.page.getByRole('dialog', { name: 'Archive' }), 'no confirm opened');
  },
  'agents menu: a group with a busy agent can’t be archived': async (t) => {
    await t.steps('Control+7');
    await t.page.locator('.tree .row', { hasText: 'migration' }).first().click({ button: 'right' });
    await t.visible(t.page.getByRole('menu'));
    const item = t.page.getByRole('menuitem', { name: /^Archive group/ });
    if (await item.isEnabled()) throw new Error('Archive group is enabled while the room has busy agents');
    if (!(await item.innerText()).includes('stop it first')) throw new Error('no "stop it first" on the disabled item');
  },
};
