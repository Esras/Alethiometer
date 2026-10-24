// The People index (views/People.svelte): its pickers open and close on their
// keys and leave the data alone (none of these checks writes anything).

const sortButton = (t) => t.page.getByRole('button', { name: /^Sort:/ });
const total = (t) => t.page.locator('.people-filter .total').innerText();

export const checks = {
  'People: s opens the sort picker, ↓ ↵ sorts by Signals, ⇧S reverses, Esc closes': async (t) => {
    await t.steps({ tab: 'People' }, 's');
    await t.visible(t.page.getByRole('listbox', { name: 'Sort people by' }), 's did not open the sort picker');
    await t.steps('ArrowDown', 'Enter');
    await t.gone(t.page.getByRole('listbox', { name: 'Sort people by' }), '↵ left the picker open');
    t.ok((await sortButton(t).innerText()).includes('Signals'), 'the sort is not Signals');
    await t.visible(t.page.getByText('Signals ↓ · 12 weeks'), 'the Signals column is not marked');
    await t.steps('Shift+S');
    await t.visible(t.page.getByText('Signals ↑ · 12 weeks'), '⇧S did not reverse');
    await t.steps('s', 'Escape');
    await t.gone(t.page.getByRole('listbox', { name: 'Sort people by' }), 'Esc left the picker open');
  },
  'People: a click outside closes the sort picker': async (t) => {
    await t.steps({ tab: 'People' }, 's');
    await t.page.mouse.click(400, 900);
    await t.gone(t.page.getByRole('listbox', { name: 'Sort people by' }), 'the picker stayed open');
  },
  'People: t picks a topic as a column, ⌫ removes it': async (t) => {
    await t.steps({ tab: 'People' }, 't');
    const field = t.page.getByPlaceholder('Filter by topic…');
    await t.focused(field, 't did not open the topic picker');
    await t.steps({ type: 'Datadog' }, 'Enter');
    await t.visible(t.page.getByRole('button', { name: 'Remove the Datadog renewal filter' }), 'no chip');
    t.ok((await total(t)).includes(' of '), 'the count does not say it is filtered');
    await t.steps('Backspace');
    await t.gone(t.page.getByRole('button', { name: 'Remove the Datadog renewal filter' }), '⌫ kept the chip');
  },
  'People: / filters by name and Esc leaves the field': async (t) => {
    await t.steps({ tab: 'People' }, '/');
    const field = t.page.getByPlaceholder('Filter people');
    await t.focused(field, '/ did not focus the filter');
    await t.steps({ type: 'zzz' });
    t.ok((await total(t)).startsWith('0 of'), `the count is "${await total(t)}"`);
    await t.steps('Escape');
    t.ok(!(await field.evaluate((el) => el === document.activeElement)), 'Escape left the field focused');
  },
  'People: m opens the merge picker and the dialog, and Esc backs out of both': async (t) => {
    await t.steps({ tab: 'People' }, 'm');
    const field = t.page.getByPlaceholder(/^Merge “.*” into…$/);
    await t.focused(field, 'm did not open the merge picker');
    await t.steps('Control+a', 'Backspace', 'Enter');
    await t.visible(t.page.getByText('after the merge'), '↵ did not open the merge dialog');
    await t.steps('Escape');
    await t.gone(t.page.getByText('after the merge'), 'Esc left the dialog open');
    await t.gone(t.page.getByText('undo merge'), 'Esc staged a merge');
  },
  'People: n asks for a name and Esc drops it': async (t) => {
    await t.steps({ tab: 'People' }, 'n');
    const field = t.page.getByPlaceholder("New person's name");
    await t.focused(field, 'n did not open the name field');
    await t.steps('Escape');
    await t.gone(field, 'Esc kept the name field');
  },
};
