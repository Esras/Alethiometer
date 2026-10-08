// lib/ui/Modal: what every modal does the same way. Settings, the ⌘K
// palette, the New agent modal and the Keys sheet (Slack review needs a Slack
// source, which the sample data lacks).
//   Escape closes · a mousedown on the scrim closes · Tab stays inside ·
//   focus goes back to where it was.

const sites = {
  Settings: {
    open: (t) => t.steps('Control+,'),
    probe: (t) => t.page.getByPlaceholder('Find a setting'),
    // Tab in a field is the trap's; outside one, Settings cycles its sections.
    inField: (t) => t.steps('/'),
    from: (t) => t.page.locator('button').first(),
  },
  Palette: {
    open: (t) => t.steps('Control+k'),
    probe: (t) => t.page.getByPlaceholder('Search everything'),
    from: (t) => t.page.locator('button').first(),
  },
  'New agent': {
    prepare: (t) => t.steps('Control+7'),
    // The Enter key would go to the tree, so click.
    open: (t) => t.page.getByRole('button', { name: 'New agent' }).first().click(),
    probe: (t) => t.page.locator('.modal.agent-form'),
    from: (t) => t.page.getByRole('button', { name: 'New agent' }).first(),
  },
  Keys: {
    prepare: (t) => t.steps('Control+7'),
    open: (t) => t.steps('Shift+?'),
    probe: (t) => t.page.getByText('? or Esc to close'),
    from: (t) => t.page.getByRole('button', { name: 'New agent' }).first(),
  },
};

const dialog = (t) => t.page.getByRole('dialog');
const insideDialog = (t) =>
  t.page.evaluate(() => {
    const a = document.activeElement;
    return !!a && (!!a.closest('[role=dialog]') || !!a.querySelector?.('[role=dialog]'));
  });

// A point just outside the site's box (the dialog wrapper has no box of its own).
async function outside(t) {
  const b = await dialog(t).locator('> *').first().boundingBox();
  return { x: Math.max(2, b.x - 12), y: b.y + b.height / 2 };
}

const checks = {};
for (const [name, s] of Object.entries(sites)) {
  const start = async (t) => {
    await s.prepare?.(t);
    await s.open(t);
    await t.visible(s.probe(t), `${name} didn't open`);
    await t.ok((await dialog(t).count()) === 1, 'one dialog, role=dialog');
    await t.ok((await dialog(t).getAttribute('aria-modal')) === 'true', 'aria-modal');
    await t.ok(!!(await dialog(t).getAttribute('aria-label')), 'aria-label');
  };

  checks[`${name}: Escape closes`] = async (t) => {
    await start(t);
    await t.steps('Escape');
    await t.gone(s.probe(t), 'still open after Escape');
  };

  checks[`${name}: mousedown on the scrim closes`] = async (t) => {
    await start(t);
    const p = await outside(t);
    await t.page.mouse.move(p.x, p.y);
    await t.page.mouse.down();
    await t.page.waitForTimeout(200);
    await t.gone(s.probe(t), 'still open after mousedown outside');
    await t.page.mouse.up();
  };

  checks[`${name}: a mousedown inside the box stays open`] = async (t) => {
    await start(t);
    const b = await dialog(t).locator('> *').first().boundingBox();
    await t.page.mouse.click(b.x + b.width / 2, b.y + b.height - 3);
    await t.page.waitForTimeout(200);
    await t.visible(s.probe(t), 'closed by a click inside the box');
  };

  checks[`${name}: Tab stays inside`] = async (t) => {
    await start(t);
    await s.inField?.(t);
    for (const key of [
      'Tab',
      'Tab',
      'Tab',
      'Tab',
      'Shift+Tab',
      'Shift+Tab',
      'Shift+Tab',
      'Shift+Tab',
      'Shift+Tab',
      'Shift+Tab',
    ]) {
      await t.page.keyboard.press(key);
      await t.page.waitForTimeout(60);
      await t.ok(await insideDialog(t), `focus left the dialog on ${key}`);
    }
    // From outside it (the button behind), Tab comes in.
    await t.page.evaluate(() => document.activeElement?.blur());
    await t.page.keyboard.press('Tab');
    await t.ok(await insideDialog(t), 'Tab from nowhere stayed outside');
  };

  checks[`${name}: focus returns`] = async (t) => {
    await s.prepare?.(t);
    const from = s.from(t);
    await from.focus();
    await t.page.evaluate(() => document.activeElement?.setAttribute('data-origin', ''));
    await s.open(t);
    await t.visible(s.probe(t), `${name} didn't open`);
    await t.steps('Escape');
    await t.gone(s.probe(t), 'still open after Escape');
    await t.focused(t.page.locator('[data-origin]'), 'focus did not return to where it was');
  };
}

// What stays as it was.
checks['Keys: keys behind the sheet stay behind it, and ? closes it'] = async (t) => {
  await t.steps('Control+7', 'Shift+?');
  await t.visible(t.page.getByText('? or Esc to close'));
  await t.steps('n');
  await t.gone(t.page.locator('.modal.agent-form'), 'n opened the New agent form under the sheet');
  await t.visible(t.page.getByText('? or Esc to close'), 'n closed the sheet');
  await t.steps('Shift+?');
  await t.gone(t.page.getByText('? or Esc to close'), '? did not close the sheet');
};

checks['New agent: ⌘K does not reach the window under it'] = async (t) => {
  await t.steps('Control+7', { button: 'New agent' });
  await t.visible(t.page.getByRole('dialog'));
  await t.steps('Control+k');
  await t.gone(t.page.getByPlaceholder('Search everything'), '⌘K opened the palette under the modal');
  await t.visible(t.page.getByRole('dialog'));
};

checks['Palette: its own keys still work'] = async (t) => {
  await t.steps('Control+k', { type: 'acme' }, 'ArrowDown');
  await t.visible(t.page.getByPlaceholder('Search everything'));
  await t.steps('Control+Enter');
  await t.gone(t.page.getByPlaceholder('Search everything'), '⌘↵ did not leave the palette for Search');
};

checks['Settings: its own keys still work'] = async (t) => {
  await t.steps('Control+,', '2');
  await t.visible(t.page.getByText('Categories').first());
  await t.steps('/');
  await t.focused(t.page.getByPlaceholder('Find a setting'));
  await t.steps('Escape');
  await t.visible(
    t.page.getByPlaceholder('Find a setting'),
    'Escape in the find field closed Settings; it should only leave the field',
  );
  await t.steps('Escape');
  await t.gone(t.page.getByPlaceholder('Find a setting'));
};

export { checks };
