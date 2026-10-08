// lib/ui/FilterField.svelte: the filter boxes keep their keys, and the box is
// the system's .input: aqua border only while the field has focus.
//   Topics index · Todos · Settings' find · the agents tree:
//   `/` focuses the field · Esc leaves it · focus is the only aqua.
// Esc clears the text in Todos, Settings and the agents tree (Esc in Settings
// leaves the field, not the modal), but keeps it in the Topics index; each
// check says which.
// Todos has no todos in the sample data, which is fine: the filter is there.

// The border colour of the .input box around a field, and what --aqua resolves to.
const border = (field) => field.evaluate((el) => getComputedStyle(el.closest('.input')).borderTopColor);
const aqua = (t) =>
  t.page.evaluate(() => {
    const probe = document.createElement('i');
    probe.style.color = 'var(--aqua)';
    document.body.append(probe);
    const c = getComputedStyle(probe).color;
    probe.remove();
    return c;
  });

async function leaves(t, field, { clears }) {
  await t.focused(field, '/ did not focus the field');
  await t.steps({ type: 'zz' });
  t.ok((await field.inputValue()) === 'zz', 'typing did not reach the field');
  const on = await border(field);
  t.ok(on === (await aqua(t)), `the box is ${on} while focused, not aqua`);
  await t.steps('Escape');
  t.ok(!(await field.evaluate((el) => el === document.activeElement)), 'Escape left the field focused');
  const value = await field.inputValue();
  t.ok(
    clears ? value === '' : value === 'zz',
    `Escape ${clears ? 'should clear' : 'should keep'} the text; it is "${value}"`,
  );
  t.ok((await border(field)) !== (await aqua(t)), 'the box is still aqua after leaving it');
}

export const checks = {
  'Topics index: / focuses the filter and Esc leaves it': async (t) => {
    await t.steps('Control+3');
    const field = t.page.getByPlaceholder('Filter topics');
    t.ok((await border(field)) !== (await aqua(t)), 'the box is aqua at rest');
    await t.steps('/');
    await leaves(t, field, { clears: false });
  },
  'Todos: / focuses the filter and Esc clears and leaves it': async (t) => {
    await t.steps('Control+4');
    const field = t.page.getByPlaceholder('Filter todos');
    t.ok((await border(field)) !== (await aqua(t)), 'the box is aqua at rest');
    await t.steps('/');
    await leaves(t, field, { clears: true });
  },
  'Settings: / focuses find and Esc clears it without closing Settings': async (t) => {
    await t.steps('Control+,');
    const field = t.page.getByPlaceholder('Find a setting');
    await t.steps('/');
    await leaves(t, field, { clears: true });
    await t.visible(field, 'Esc closed Settings');
  },
  'Agents: / focuses the tree filter and Esc clears and leaves it': async (t) => {
    await t.steps('Control+7');
    const field = t.page.getByPlaceholder('Filter sessions');
    t.ok((await border(field)) !== (await aqua(t)), 'the box is aqua at rest');
    await t.steps('/');
    await leaves(t, field, { clears: true });
  },
};
