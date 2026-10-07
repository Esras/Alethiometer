// Priorities (app/docs/priorities.md): one cursor across three columns.
//   j/k move in the focused column · [ ] the same todo in the next column ·
//   / focuses the filter, Esc leaves it (the text stays) · a filter removes a
//   todo from every column · the Selected panel never changes height.

const selected = (t) => t.page.locator('.selected .sel-title').innerText();
const focusedCol = (t) => t.page.locator('.colhead .panel-head.focused').innerText();
const counts = (t) => t.page.locator('.colhead .count').allInnerTexts();

export const checks = {
  'priorities: j moves the cursor and the Selected panel follows': async (t) => {
    await t.steps('Control+2');
    const first = await selected(t);
    await t.steps('j');
    t.ok((await selected(t)) !== first, 'j left the selection where it was');
    await t.steps('k');
    t.ok((await selected(t)) === first, 'k did not come back');
  },
  'priorities: ] and [ keep the todo and move the focus': async (t) => {
    await t.steps('Control+2', 'j', 'j');
    const todo = await selected(t);
    await t.steps(']');
    t.ok(/Quick wins/i.test(await focusedCol(t)), `] focused ${await focusedCol(t)}`);
    t.ok((await selected(t)) === todo, '] changed the todo');
    await t.steps(']', ']');
    t.ok(/Neglect/i.test(await focusedCol(t)), '] went past the last column');
    await t.steps('[', '[', '[');
    t.ok(/Priority/i.test(await focusedCol(t)), '[ went past the first column');
    t.ok((await selected(t)) === todo, '[ changed the todo');
  },
  'priorities: the cursor row is lit in the focused column only': async (t) => {
    await t.steps('Control+2', ']');
    t.ok((await t.page.locator('.prow.cur').count()) === 1, 'not exactly one cursor row');
    t.ok((await t.page.locator('.prow.open').count()) === 2, 'the same todo is not open-row grey in the other two columns');
  },
  'priorities: / focuses the filter, Esc leaves it and keeps the text': async (t) => {
    await t.steps('Control+2', '/');
    const field = t.page.getByPlaceholder('Filter todos');
    await t.focused(field, '/ did not focus the filter');
    await t.steps({ type: 'webhook' });
    await t.steps('Escape');
    t.ok(!(await field.evaluate((el) => el === document.activeElement)), 'Esc left the field focused');
    t.ok((await field.inputValue()) === 'webhook', 'Esc cleared the text');
    const c = await counts(t);
    t.ok(c.length === 3 && c.every((n) => n === c[0]) && Number(c[0]) > 0, `columns disagree after filtering: ${c}`);
    await t.steps({ button: 'Clear' });
    t.ok((await field.inputValue()) === '', 'Clear left the text');
  },
  'priorities: a filter option removes todos from every column': async (t) => {
    await t.steps('Control+2');
    const before = await counts(t);
    await t.page.locator('.dim .chip', { hasText: 'Source' }).click();
    await t.page.locator('.dim .menu .opt', { hasText: 'Jira' }).click();
    await t.page.waitForTimeout(200);
    const after = await counts(t);
    t.ok(after.every((n) => n === after[0]) && Number(after[0]) < Number(before[0]), `${before} → ${after}`);
    await t.visible(t.page.locator('.filters .shown', { hasText: 'of' }), 'no "n of m" count');
  },
  'priorities: picking a todo never moves the page': async (t) => {
    await t.steps('Control+2');
    const top = () => t.page.locator('.cols').evaluate((el) => el.getBoundingClientRect().top);
    const a = await top();
    for (let i = 0; i < 6; i++) await t.steps('j');
    await t.steps(']', ']');
    t.ok((await top()) === a, 'the columns moved');
  },
};
