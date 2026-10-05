// lib/ui/Cycler: one button shows the value; a click (Shift-click: back) or its key steps
// to the next option and wraps around. Where a view binds the keys itself (Reflect's d w m q,
// the split review's g, the todo form's ⌘1/⌘2) those keys still work and the cycler shows
// the result. The Assistant's Kept/Dropped (key Tab) needs a Glean answer the sample data
// can't give, so its key is not checked here.

// The cycler whose aria-label starts with `what`, and the value it shows.
const cycler = (t, what) => t.page.getByRole('button', { name: new RegExp(`^${what}:`) }).first();
const shown = (t, what) => cycler(t, what).locator('.val').innerText();
// A setting's value comes back from the server, so poll for a moment before giving up.
const expectShown = async (t, what, want, msg) => {
  let got = await shown(t, what);
  for (let i = 0; i < 20 && got !== want; i++) {
    await t.page.waitForTimeout(100);
    got = await shown(t, what);
  }
  t.ok(got === want, `${msg}: wanted "${want}", got "${got}"`);
};

export const checks = {
  'Topics index: Active/Archived steps by click and wraps': async (t) => {
    await t.steps('Control+3');
    await expectShown(t, 'Topics', 'Active', 'at rest');
    await cycler(t, 'Topics').click();
    await expectShown(t, 'Topics', 'Archived', 'one click');
    await cycler(t, 'Topics').click();
    await expectShown(t, 'Topics', 'Active', 'wraps');
    await cycler(t, 'Topics').click({ modifiers: ['Shift'] });
    await expectShown(t, 'Topics', 'Archived', 'Shift-click steps back');
  },

  'Reflect: the scope keys still jump, the cycler shows and steps': async (t) => {
    await t.steps('Control+6');
    await expectShown(t, 'Scope', 'Day', 'at rest');
    await t.steps('w');
    await expectShown(t, 'Scope', 'Week', 'w');
    await t.steps('m');
    await expectShown(t, 'Scope', 'Month', 'm');
    await t.steps('q');
    await expectShown(t, 'Scope', 'Quarter', 'q');
    await cycler(t, 'Scope').click();
    await expectShown(t, 'Scope', 'Day', 'a click on the last wraps to the first');
    await cycler(t, 'Scope').click();
    await expectShown(t, 'Scope', 'Week', 'a click steps on');
    await cycler(t, 'Scope').click({ modifiers: ['Shift'] });
    await expectShown(t, 'Scope', 'Day', 'Shift-click steps back');
    await t.steps('d');
    await expectShown(t, 'Scope', 'Day', 'd');
  },

  'Split review: g steps, wraps, and a click steps too': async (t) => {
    await t.steps('j', 's');
    // A DM opens in order, a channel by topic: start from whichever it is.
    const first = await shown(t, 'Order');
    const other = first === 'By topic' ? 'In order' : 'By topic';
    await t.steps('g');
    await expectShown(t, 'Order', other, 'g');
    await t.steps('g');
    await expectShown(t, 'Order', first, 'g wraps');
    await cycler(t, 'Order').click();
    await expectShown(t, 'Order', other, 'click');
    // The button now has the focus; g still steps it, and the review's own handler still
    // takes the key (it swallows every other letter).
    await t.steps('g');
    await expectShown(t, 'Order', first, 'g after a click');
  },

  'Todo form: ⌘1/⌘2 keep working, and the cycler is off without a Jira source': async (t) => {
    await t.steps('n');
    const c = cycler(t, 'Create as');
    await expectShown(t, 'Create as', 'Todo', 'at rest');
    t.ok(await c.isDisabled(), 'the Jira choice should be off: the sample data has no Jira source');
    await t.steps('Control+1');
    await expectShown(t, 'Create as', 'Todo', '⌘1');
    await t.visible(t.page.getByPlaceholder('What to do'));
  },

  'Settings: a cycler steps under the modal key floor, by click and by ←/→': async (t) => {
    await t.steps('Control+,', '6');
    await t.visible(t.page.getByPlaceholder('Find a setting'));
    const what = 'Color names by';
    const order = ['Agent', 'Template', 'Off'];
    const at = async () => order.indexOf(await shown(t, what));
    const start = await at();
    t.ok(start >= 0, `at rest: "${await shown(t, what)}" is not one of ${order}`);
    const next = (n) => order[(start + n + order.length) % order.length];
    await cycler(t, what).click();
    await expectShown(t, what, next(1), 'click');
    await cycler(t, what).click();
    await expectShown(t, what, next(2), 'click again');
    await cycler(t, what).click();
    await expectShown(t, what, next(0), 'wraps');
    await cycler(t, what).click({ modifiers: ['Shift'] });
    await expectShown(t, what, next(2), 'Shift-click steps back');
    // Settings' own ←/→ on the focused row still steps it (the cycler registers no key here).
    await t.page.getByText(what, { exact: true }).first().click();
    await t.steps('ArrowRight');
    await expectShown(t, what, next(0), 'ArrowRight');
    await t.steps('ArrowLeft');
    await expectShown(t, what, next(2), 'ArrowLeft');
    // Still in Settings.
    await t.visible(t.page.getByPlaceholder('Find a setting'));
  },
};
