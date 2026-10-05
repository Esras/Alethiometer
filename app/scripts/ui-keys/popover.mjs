// Popovers (lib/ui/popover.ts): Escape closes, a click outside closes, a click
// inside doesn't. The day strip's block popover, the sync chip's popover, the
// Priorities peek and the Composer's @ menu.
const outside = (t) => t.page.mouse.click(1, 1170);

// One popover: how to open it, where it is, something to click inside it.
function cases(name, { open, pop, inside }) {
  return {
    [`${name}: Escape closes`]: async (t) => {
      await open(t);
      await t.visible(pop(t));
      await t.page.keyboard.press('Escape');
      await t.page.waitForTimeout(300);
      await t.gone(pop(t), 'still open after Escape');
    },
    [`${name}: a click outside closes`]: async (t) => {
      await open(t);
      await t.visible(pop(t));
      await outside(t);
      await t.page.waitForTimeout(300);
      await t.gone(pop(t), 'still open after a click outside');
    },
    [`${name}: a click inside stays open`]: async (t) => {
      await open(t);
      await t.visible(pop(t));
      await inside(t).click({ position: { x: 2, y: 2 } }); // the corner: the composer menu's padding, not an option
      await t.page.waitForTimeout(300);
      await t.visible(pop(t), 'closed by a click inside');
    },
  };
}

const life = (t) => t.page.getByRole('button', { name: /^Life, busy/ });
const composerMenu = async (t) => {
  await t.steps('Control+7', { exact: 'migration', nth: 1 });
  await t.page.getByPlaceholder(/^Message #migration/).click();
  await t.page.keyboard.type('@');
  await t.page.waitForTimeout(300);
};

export const checks = {
  // The keys the views use while a popover is open still reach them.
  'day strip: b edits the buffer, Esc leaves the editor first': async (t) => {
    await life(t).first().click();
    await t.steps('b');
    await t.visible(t.page.locator('.popover input.buf'));
    await t.steps('Escape');
    await t.gone(t.page.locator('.popover input.buf'), 'the editor stayed open');
    await t.visible(t.page.locator('.popover'), 'Esc in the editor closed the popover too');
    await t.steps('Escape');
    await t.gone(t.page.locator('.popover'));
  },
  'day strip: another block takes over the popover': async (t) => {
    await life(t).nth(0).click();
    const first = await t.page.locator('.popover .pop-title').innerText();
    await life(t).nth(1).click();
    await t.page.waitForTimeout(300);
    await t.visible(t.page.locator('.popover'), 'closed instead of switching');
    t.ok(first !== (await t.page.locator('.popover .pop-title').innerText()), 'still the first block');
  },
  'day strip: a click on its own block closes it': async (t) => {
    await life(t).first().click();
    await life(t).first().click();
    await t.page.waitForTimeout(300);
    await t.gone(t.page.locator('.popover'), 'reopened');
  },
  'priorities peek: j moves on and closes it': async (t) => {
    await t.steps('Control+2', 'p');
    await t.visible(t.page.locator('.peek'));
    await t.steps('j');
    await t.gone(t.page.locator('.peek'));
  },
  'composer menu: Esc closes the menu and keeps the focus, typing brings it back': async (t) => {
    await composerMenu(t);
    await t.steps('Escape');
    await t.gone(t.page.locator('.box .pop'));
    await t.focused(t.page.locator('.box textarea'), 'Esc moved the focus');
    await t.page.keyboard.type('p');
    await t.page.waitForTimeout(300);
    await t.visible(t.page.locator('.box .pop'), 'typing did not bring the menu back');
    await t.steps('Escape', 'Escape');
    await t.page.waitForTimeout(300);
    t.ok(!(await t.page.locator('.box textarea').evaluate((el) => el === document.activeElement)), 'the second Esc did not leave the composer');
  },
  ...cases('day strip block', {
    open: (t) => life(t).first().click(),
    pop: (t) => t.page.locator('.popover'),
    inside: (t) => t.page.locator('.popover .pop-title'),
  }),
  ...cases('sync chip', {
    open: (t) => t.steps({ click: 'Sources and writes' }),
    pop: (t) => t.page.locator('.wrap .pop'),
    inside: (t) => t.page.locator('.wrap .pop .label').first(),
  }),
  ...cases('priorities peek', {
    open: (t) => t.steps('Control+2', 'p'),
    pop: (t) => t.page.locator('.peek'),
    inside: (t) => t.page.locator('.peek .peek-title'),
  }),
  // Needs the Scion room from the fake hub (docs/agents.md): the menu is the @ completion in a room.
  ...cases('composer menu', {
    open: composerMenu,
    pop: (t) => t.page.locator('.box .pop'),
    inside: (t) => t.page.locator('.box .pop'),
  }),
};
