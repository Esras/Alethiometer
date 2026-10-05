// lib/ui/picker.svelte.ts: what the five pickers do the same way. The topic
// picker (Todos' new-todo form), the person picker (Incoming), the category
// picker (Topics index), the notes picker (Tab on a line) and the font picker
// (Settings), plus the ⌃j/⌃k list keys (lib/ui/listNav.ts) where they were
// hand-written: ⌘K, Settings, the Topics index. (Slack review needs a Slack
// source, which the sample data lacks.)
//   typing filters · ↓ ⌃j ↑ move the cursor · ↵ picks · Esc closes ·
//   a click outside closes · the pointer moves the cursor · a click picks.
// ⌃k is only tested in Settings: everywhere else App's ⌘K handler takes it
// first and opens the palette, before any picker or list sees it.
// What a pick changes is put back where it can be, but a run still leaves a
// mark (the notes line's target, Settings' "Saved"): restart ui-diff.sh before
// a ui-diff after it, or its notes and settings scenes differ.
// (The person picker's ↵ writes a People note and can't be put back: it isn't
// here. Its ↵ is the same code as the topic picker's.)
const outside = (t) => t.page.mouse.click(1, 1170);

const cursor = (t, cfg) => cfg.opts(t).evaluateAll((els, c) => els.findIndex((e) => e.classList.contains(c)), cfg.on ?? 'on');

async function expectCursor(t, cfg, want, what) {
  const got = await cursor(t, cfg);
  t.ok(got === want, `${what}: the cursor is on row ${got}, not ${want}`);
}

function cases(name, cfg) {
  const open = async (t) => {
    await cfg.open(t);
    await t.visible(cfg.root(t), 'did not open');
    await t.page.waitForTimeout(300);
  };
  const out = {
    [`${name}: typing filters`]: async (t) => {
      await open(t);
      const before = await cfg.opts(t).count();
      await t.steps({ type: cfg.filter.type });
      await t.page.waitForTimeout(400);
      const after = await cfg.opts(t).count();
      t.ok(after > 0 && after < before, `${before} rows before, ${after} after`);
      t.ok(cfg.filter.expect.test(await cfg.opts(t).first().innerText()), `first row after filtering: ${await cfg.opts(t).first().innerText()}`);
    },
    [`${name}: ↓ and ↑ move the cursor`]: async (t) => {
      await open(t);
      await expectCursor(t, cfg, cfg.start ?? 0, 'at the start');
      const s = cfg.start ?? 0;
      await t.steps('ArrowDown');
      await expectCursor(t, cfg, s + 1, 'after ↓');
      await t.steps('ArrowDown');
      await expectCursor(t, cfg, s + 2, 'after ↓↓');
      await t.steps('ArrowUp');
      await expectCursor(t, cfg, s + 1, 'after ↑');
    },
    [`${name}: the cursor stops at the top`]: async (t) => {
      await open(t);
      for (let i = 0; i < (cfg.start ?? 0) + 2; i++) await t.steps('ArrowUp');
      await expectCursor(t, cfg, 0, 'at the top');
    },
    [`${name}: Escape closes`]: async (t) => {
      await open(t);
      await t.steps('Escape');
      await t.gone(cfg.root(t), 'still open after Escape');
      if (cfg.after) await cfg.after(t);
    },
    [`${name}: a click outside closes`]: async (t) => {
      await open(t);
      await (cfg.outside ?? outside)(t);
      await t.page.waitForTimeout(300);
      await t.gone(cfg.root(t), 'still open after a click outside');
      if (cfg.after) await cfg.after(t);
    },
    [`${name}: the pointer moves the cursor`]: async (t) => {
      await open(t);
      const box = await cfg.opts(t).nth(2).boundingBox();
      await t.page.mouse.move(box.x + box.width / 2, box.y - 40);
      await t.page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
      await t.page.waitForTimeout(200);
      await expectCursor(t, cfg, 2, 'after the pointer moved');
      await t.visible(cfg.root(t), 'closed when the pointer moved');
    },
  };
  if (cfg.pick) out[`${name}: ↵ picks`] = async (t) => (await open(t), cfg.pick(t, 'enter'));
  if (cfg.click) out[`${name}: a click picks`] = async (t) => (await open(t), cfg.click(t));
  return out;
}

// Todos' new-todo form: nothing is written until it's saved, so a pick only fills the field.
const todoTopic = {
  open: async (t) => {
    await t.steps('Control+4', 'n');
    await t.page.getByRole('button', { name: 'No topic' }).click();
  },
  root: (t) => t.page.locator('.picker'),
  opts: (t) => t.page.locator('.picker .opt'),
  filter: { type: 'q4', expect: /Q4/i },
};

const incomingPerson = {
  open: async (t) => {
    await t.steps('Enter', { exact: 'Marco R.' });
    // It starts on "Marco", which matches no one: clear it to list the People notes.
    await t.steps('Control+a', 'Backspace');
    await t.page.waitForTimeout(400);
  },
  root: (t) => t.page.locator('.card[aria-label="Link to person"]'),
  opts: (t) => t.page.locator('.card .rows .row'),
  filter: { type: 'pri', expect: /Priya/ },
};

const topicsCategory = {
  open: (t) => t.steps('Control+3', 'c'),
  root: (t) => t.page.locator('.pick .picker'),
  opts: (t) => t.page.locator('.pick .picker .opt'),
  filter: { type: 'w', expect: /w/i },
};

const notesLine = {
  open: (t) => t.steps('Control+5', 'Enter', 'Tab'),
  root: (t) => t.page.locator('.picker[aria-label="Where it goes"]'),
  opts: (t) => t.page.locator('.picker .opt'),
  filter: { type: 'q4', expect: /Q4/i },
  // Closing puts the cursor back in the line.
  after: (t) => t.focused(t.page.locator('textarea').last(), 'the line lost the focus'),
};

const fontPick = {
  open: async (t) => {
    await t.steps('Control+,');
    await t.page.locator('.pick .current').first().click();
  },
  root: (t) => t.page.locator('.pop[role=listbox]'),
  opts: (t) => t.page.locator('.pop .opt'),
  on: 'sel',
  filter: { type: 'dejavu', expect: /DejaVu/i },
  // Inside Settings: its scrim would close it, so click its title.
  outside: (t) => t.page.getByText('General', { exact: true }).first().click(),
  after: async (t) => t.visible(t.page.getByPlaceholder('Find a setting'), 'Settings closed too'),
};

// A pick that writes: do it, look at the result, put it back.
const picks = {
  'topic picker (todo form): ↵ picks': async (t) => {
    await todoTopic.open(t);
    await t.steps({ type: 'q4' }, 'Enter');
    await t.gone(t.page.locator('.picker'), 'still open after ↵');
    await t.visible(t.page.getByText('Q4 headcount').first(), 'the topic did not land in the form');
  },
  'topic picker (todo form): ↓ ↵ picks the row under the cursor': async (t) => {
    await todoTopic.open(t);
    const second = (await t.page.locator('.picker .opt').nth(1).innerText()).trim();
    await t.steps('ArrowDown', 'Enter');
    await t.gone(t.page.locator('.picker'), 'still open after ↵');
    await t.visible(t.page.getByText(second, { exact: true }).first(), `${second} did not land in the form`);
  },
  'topic picker (todo form): a click picks, and the field keeps the focus until then': async (t) => {
    await todoTopic.open(t);
    const third = (await t.page.locator('.picker .opt').nth(2).innerText()).trim();
    await t.page.locator('.picker .opt').nth(2).click();
    await t.page.waitForTimeout(300);
    await t.gone(t.page.locator('.picker'), 'still open after a click');
    await t.visible(t.page.getByText(third, { exact: true }).first(), `${third} did not land in the form`);
  },
  'topic picker (todo form): a click on its padding keeps it open': async (t) => {
    await todoTopic.open(t);
    await t.page.locator('.picker .field').click({ position: { x: 2, y: 2 } });
    await t.page.waitForTimeout(300);
    await t.visible(t.page.locator('.picker'), 'closed by a click inside');
    await t.focused(t.page.locator('.picker input'), 'the field lost the focus');
  },
  'person picker: typing sends fewer searches than keystrokes, and the last query wins': async (t) => {
    await incomingPerson.open(t);
    let searches = 0;
    t.page.on('request', (r) => r.url().includes('people.search') && searches++);
    await t.page.keyboard.type('priya', { delay: 20 });
    await t.page.waitForTimeout(600);
    t.ok(searches > 0 && searches < 5, `${searches} searches for 5 keystrokes`);
    const rows = await t.page.locator('.card .rows .row').allInnerTexts();
    t.ok(rows.length === 2 && /Priya/.test(rows[0]) && /Create person/.test(rows[1]), `rows for "priya": ${rows.join(' | ')}`);
  },
  'person picker: the create row is the last one and the cursor stops on it': async (t) => {
    await incomingPerson.open(t);
    await t.steps({ type: 'zz' });
    await t.page.waitForTimeout(400);
    const rows = await t.page.locator('.card .rows .row').allInnerTexts();
    t.ok(rows.length === 1 && /Create person “zz”/.test(rows[0]), `rows for "zz": ${rows.join(' | ')}`);
    await t.steps('ArrowDown'); // one row: the cursor stays on it
    t.ok((await cursor(t, incomingPerson)) === 0, 'the cursor left the only row');
  },
  'category picker: ↓ ↵ changes the category, and a click puts it back': async (t) => {
    await t.steps('Control+3');
    const bar = t.page.locator('.row.sel .bar').first();
    const before = await bar.evaluate((e) => e.style.background);
    await t.steps('c');
    await t.steps('ArrowDown', 'Enter');
    await t.gone(t.page.locator('.pick .picker'), 'still open after ↵');
    await t.page.waitForTimeout(300);
    const after = await bar.evaluate((e) => e.style.background);
    t.ok(after !== before, 'the category did not change');
    // Put it back: the cursor starts on the current one, so ↑ is the old one.
    await t.steps('c', 'ArrowUp', 'Enter');
    await t.page.waitForTimeout(300);
    t.ok((await bar.evaluate((e) => e.style.background)) === before, 'did not put the category back');
  },
  'category picker: a click picks': async (t) => {
    await t.steps('Control+3');
    const bar = t.page.locator('.row.sel .bar').first();
    const before = await bar.evaluate((e) => e.style.background);
    await t.steps('c');
    await t.page.locator('.pick .picker .opt').nth(1).click();
    await t.page.waitForTimeout(400);
    await t.gone(t.page.locator('.pick .picker'), 'still open after a click');
    t.ok((await bar.evaluate((e) => e.style.background)) !== before, 'the category did not change');
    await t.steps('c', 'ArrowUp', 'Enter');
    await t.page.waitForTimeout(300);
    t.ok((await bar.evaluate((e) => e.style.background)) === before, 'did not put the category back');
  },
  'category picker: on a topic page it sits under its button': async (t) => {
    await t.steps('Control+3', 'Enter', 'c');
    const pick = t.page.locator('.catpick .picker');
    await t.visible(pick, 'did not open');
    const [a, b] = await Promise.all([pick.boundingBox(), t.page.locator('.catpick > button').boundingBox()]);
    t.ok(Math.abs(a.y - (b.y + b.height + 4)) < 1.5 && Math.abs(a.x - b.x) < 1.5, `at ${a.x},${a.y}, the button is ${b.x},${b.y + b.height}`);
    await t.steps('Escape');
    await t.gone(pick);
  },
  'notes picker: Tab and ⇧Tab move the cursor, ↵ picks': async (t) => {
    await notesLine.open(t);
    await t.steps('Tab');
    await expectCursor(t, notesLine, 1, 'after Tab');
    await t.steps('Tab', 'Shift+Tab');
    await expectCursor(t, notesLine, 1, 'after Tab, ⇧Tab');
    await t.steps('Enter');
    await t.gone(notesLine.root(t), 'still open after ↵');
  },
  'notes picker: a click picks': async (t) => {
    await notesLine.open(t);
    await t.page.locator('.picker .opt').nth(1).click();
    await t.page.waitForTimeout(300);
    await t.gone(notesLine.root(t), 'still open after a click');
  },
  'notes picker: typing searches': async (t) => {
    await notesLine.open(t);
    await t.steps({ type: 'zzzz' });
    await t.page.waitForTimeout(500);
    const n = await t.page.locator('.picker .opt').count();
    t.ok(n <= 2, `${n} rows for a query nothing matches`);
  },
  'font picker: ↓ ↵ picks, and the setting follows': async (t) => {
    await fontPick.open(t);
    await t.steps('ArrowDown', 'Enter');
    await t.gone(fontPick.root(t), 'still open after ↵');
    await t.visible(t.page.locator('.pick .current', { hasText: 'DejaVu Sans' }).first(), 'the setting did not change');
    await t.page.locator('.pick .current').first().click();
    await t.steps('Home'); // nothing: the cursor starts on the current font
    await t.page.locator('.pop .opt').first().click(); // System
    await t.page.waitForTimeout(300);
    await t.visible(t.page.locator('.pick .current', { hasText: /^System/ }).first(), 'did not put the font back');
  },
  'font picker: Escape leaves Settings open': async (t) => {
    await fontPick.open(t);
    await t.steps('Escape');
    await t.gone(fontPick.root(t));
    await t.visible(t.page.getByPlaceholder('Find a setting'), 'Escape closed Settings too');
  },
  'font picker: its button closes it again': async (t) => {
    await fontPick.open(t);
    await t.page.locator('.pick .current').first().click();
    await t.page.waitForTimeout(400);
    await t.gone(fontPick.root(t), 'the button reopened it');
  },
  'font picker: the cursor follows ↓ down a long list': async (t) => {
    await fontPick.open(t);
    for (let i = 0; i < 14; i++) await t.steps('ArrowDown');
    const inView = await t.page.locator('.pop .opt.sel').evaluate((e) => {
      const r = e.getBoundingClientRect();
      const box = e.closest('.opts').getBoundingClientRect();
      return r.top >= box.top - 1 && r.bottom <= box.bottom + 1;
    });
    t.ok(inView, 'the cursor row scrolled out of view');
  },
};

// The list keys where they were hand-written (lib/ui/listNav.ts).
const lists = {
  '⌘K: ↓ and ↑ move the result': async (t) => {
    await t.steps('Control+k', { type: 'acme' });
    await t.page.waitForTimeout(500);
    const sel = () => t.page.locator('.row[data-i]').evaluateAll((els) => els.findIndex((e) => e.classList.contains('sel')));
    t.ok((await sel()) === 0, 'not at the top');
    await t.steps('ArrowDown');
    t.ok((await sel()) === 1, `↓: row ${await sel()}`);
    await t.steps('ArrowUp');
    t.ok((await sel()) === 0, `↑: row ${await sel()}`);
  },
  'Settings: ↓ and ↑ move the found setting, j and k move the setting': async (t) => {
    await t.steps('Control+,', '/', { type: 'font' });
    await t.page.waitForTimeout(300);
    const found = () => t.page.locator('.result').evaluateAll((els) => els.findIndex((e) => e.classList.contains('sel')));
    t.ok((await found()) === 0, 'not at the top');
    await t.steps('ArrowDown');
    t.ok((await found()) === 1, `↓: result ${await found()}`);
    await t.steps('ArrowUp');
    t.ok((await found()) === 0, `↑: result ${await found()}`);
    await t.steps('Escape', 'Escape');
    await t.gone(t.page.getByPlaceholder('Find a setting'));
    await t.steps('Control+,');
    const focused = () => t.page.locator('.row').evaluateAll((els) => els.findIndex((e) => e.classList.contains('focused')));
    const start = await focused();
    await t.steps('j');
    t.ok((await focused()) === start + 1, `j: setting ${await focused()} after ${start}`);
    await t.steps('ArrowDown');
    t.ok((await focused()) === start + 2, `↓: setting ${await focused()}`);
    await t.steps('k');
    t.ok((await focused()) === start + 1, `k: setting ${await focused()}`);
  },
  'Topics index: ↓ and ↑ move in the filter, j and k on the list': async (t) => {
    await t.steps('Control+3');
    const sel = () => t.page.locator('.row[data-topic]').evaluateAll((els) => els.findIndex((e) => e.classList.contains('sel')));
    const start = await sel();
    await t.steps('j');
    t.ok((await sel()) === start + 1, `j: row ${await sel()} after ${start}`);
    await t.steps('k');
    t.ok((await sel()) === start, `k: row ${await sel()}`);
    await t.steps('/');
    await t.steps('ArrowDown');
    t.ok((await sel()) === start + 1, `↓ in the filter: row ${await sel()}`);
    await t.steps('ArrowDown');
    t.ok((await sel()) === start + 2, `↓↓ in the filter: row ${await sel()}`);
    await t.steps('ArrowUp');
    t.ok((await sel()) === start + 1, `↑ in the filter: row ${await sel()}`);
  },
};

export const checks = {
  ...cases('topic picker (todo form)', todoTopic),
  ...cases('person picker', incomingPerson),
  ...cases('category picker', topicsCategory),
  ...cases('notes picker', notesLine),
  ...cases('font picker', fontPick),
  ...picks,
  ...lists,
};
