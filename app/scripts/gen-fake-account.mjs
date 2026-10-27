// Invented account data for design mocks. Run: node app/scripts/gen-fake-account.mjs
import { writeFileSync, mkdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const out = join(dirname(fileURLToPath(import.meta.url)), '../../project/fixtures');
let s = 20261019;
const rnd = () => (s = (Math.imul(s, 1664525) + 1013904223) >>> 0) / 2 ** 32;
const int = (a, b) => a + Math.floor(rnd() * (b - a + 1));
const pick = (a) => a[Math.floor(rnd() * a.length)];
const wpick = (pairs) => {
  let r = rnd() * pairs.reduce((t, p) => t + p[1], 0);
  for (const [v, w] of pairs) if ((r -= w) < 0) return v;
  return pairs[0][0];
};
const wsample = (pool, n, weight) => {
  const left = [...pool];
  const got = [];
  while (got.length < n && left.length) {
    const i = left.indexOf(wpick(left.map((p) => [p, weight(p)])));
    got.push(left.splice(i, 1)[0]);
  }
  return got;
};
const shuffle = (a) => {
  for (let i = a.length - 1; i > 0; i--) {
    const j = Math.floor(rnd() * (i + 1));
    [a[i], a[j]] = [a[j], a[i]];
  }
  return a;
};
const lognormal = (sigma) => Math.exp(sigma * Math.sqrt(-2 * Math.log(1 - rnd())) * Math.cos(2 * Math.PI * rnd()));
const cap = (t) => t[0].toUpperCase() + t.slice(1);
const key = (ids) => [...ids].sort((a, b) => a - b).join();
const tally = (xs, f) => xs.reduce((m, x) => ((m[f(x)] = (m[f(x)] || 0) + 1), m), {});
const median = (a) => [...a].sort((x, y) => x - y)[Math.floor(a.length / 2)];

const FIRST =
  'Ada Bram Cleo Dara Elio Faye Gus Hana Ivo Jun Kai Lena Milo Nora Omar Pia Quin Rae Sol Tess Uma Vik Wren Xan Yara Zed Alma Beck Cora Dov Edda Finn Gwen Hugo Ines Jory Kit Lux Mara Nico Opal Pax Remy Suri Taj Una Vera Wade Yuki Zora Anya Basil Cyrus Delia Emeka Flora Gideon Hollis Isla Jonah Keiko Linus Maeve Nadia Orrin Priya Rafael Selma Tobias Ursa Vikram Willa Xiomara Yusuf Zainab'.split(
    ' ',
  );
const LAST =
  'Aldous Brandt Calloway Dunmore Estrada Fenwick Gallo Hargrove Imura Jessup Kowal Lindqvist Marlow Novak Okafor Pruitt Quarles Rinaldi Sato Thorne Ueda Voss Whitlock Yamada Zielinski Abara Bishop Corvin Delacroix Eckhart Farrow Galloway Haddad Ibsen Jaramillo Kessler Lacroix Mendes Nakagawa Oyelaran Petrov Quintero Rasmussen Szabo Tanaka'.split(
    ' ',
  );
const NOUN =
  'pipeline rollout migration dashboard cluster budget contract audit invoice roadmap schema ticket alert quota backup certificate runbook dependency forecast headcount vendor renewal incident postmortem rotation endpoint cache queue deploy release review proposal onboarding policy license benchmark config token webhook bucket index sprint estimate'.split(
    ' ',
  );
const VERB =
  'update check fix review move drop add rename revert retry document approve schedule split merge close reopen escalate pin bump rotate triage'.split(
    ' ',
  );
const ADJ =
  'flaky stale pending blocked urgent noisy slow duplicate missing legacy shared internal quarterly weekly external partial'.split(
    ' ',
  );
const ORGS = 'Northwind Globex Initech Umbrella Hooli Vandelay Stark Wayne Tyrell Cyberdyne Soylent Oscorp'.split(' ');
const CATS = [
  ['ai', 23],
  ['alert', 3],
  ['corporate', 11],
  ['customers', 20],
  ['engineering', 52],
  ['finance', 6],
  ['fun', 6],
  ['hiring', 5],
  ['life', 2],
  ['management', 26],
  ['security', 24],
  ['vendors', 24],
  ['work', 39],
];
const PROJ = ['PLAT', 'DATA', 'SEC', 'CUST', 'FIN'];
const REPO = ['acme/api', 'acme/infra', 'acme/web'];

const sentence = (min = 6, max = 16) => {
  const n = int(min, max);
  const w = [];
  for (let i = 0; i < n; i++) w.push(pick([VERB, ADJ, NOUN, ['the', 'a', 'this', 'that', 'our']][i % 4]));
  return cap(w.join(' ')) + pick(['.', '.', '?', '!']);
};
const text = (len) => {
  let t = '';
  while (t.length < len) t += sentence() + ' ';
  return t.trim();
};
const title = (len) =>
  cap(
    `${pick(VERB)} ${pick(ADJ)} ${pick(NOUN)} ${pick(NOUN)} ${pick(['for', 'in', 'after', 'before'])} ${pick(NOUN)}`,
  ).slice(0, Math.max(10, len));

const addDays = (day, n) => new Date(Date.parse(day + 'T00:00:00Z') + n * 864e5).toISOString().slice(0, 10);
const addMin = (at, n) => new Date(Date.parse(at + 'Z') + n * 6e4).toISOString().slice(0, 19);
const FIRST_DAY = '2026-09-28';
const TODAY = '2026-10-26';
const NOW = `${TODAY}T10:52:00`;
const HORIZON = '2026-12-24';
const DAYS = Array.from({ length: 29 }, (_, i) => addDays(FIRST_DAY, i));
const FUTURE = Array.from({ length: 59 }, (_, i) => addDays(TODAY, i + 1)).filter((d) => d <= HORIZON);
const QUIET_DAYS = 7;
const quietBefore = addDays(TODAY, -QUIET_DAYS);
const weekOf = (day) => Math.floor((Date.parse(day) - Date.parse(FIRST_DAY)) / (7 * 864e5));
const WEEKS = weekOf(HORIZON) + 1;
const weekday = (day) => (new Date(day + 'T00:00:00Z').getUTCDay() + 6) % 7;
const WEEKDAY_SCALE = [0.85, 1.0, 1.15, 1.2, 0.95, 0.07, 0.04];

const names = shuffle(FIRST.flatMap((f) => LAST.map((l) => `${f} ${l}`)));
const person = (id, name, domain, extra = {}) => {
  const h = name.toLowerCase().replace(' ', '.');
  return { id, name, handle: h, email: `${h}@${domain}`, groups: [], ...extra };
};
const ME = person(0, 'Sam Rivera', 'example.com');
const people = names.slice(0, 600).map((n, i) => person(i + 1, n, 'example.com'));
const externals = names.slice(600, 660).map((n, i) => {
  const org = pick(ORGS);
  return person(601 + i, n, `${org.toLowerCase()}.example`, { external: true, org });
});
const all = [ME, ...people, ...externals];
const byId = new Map(all.map((p) => [p.id, p]));
const baseWeight = new Map(people.map((p, i) => [p.id, 1 / (i + 4) ** 1.15]));
externals.forEach((p) => baseWeight.set(p.id, 0.004));
const personWeek = new Map(all.map((p) => [p.id, Array.from({ length: WEEKS }, () => lognormal(0.55))]));
const weightOn = (day) => (id) => baseWeight.get(id) * personWeek.get(id)[weekOf(day)];

const topicNames = new Set();
const topics = Array.from({ length: 184 }, (_, i) => {
  let name;
  do
    name =
      `${cap(pick(ADJ))} ${pick(NOUN)} ${pick(['', 'v2', 'cleanup', 'Q4', 'rollout', 'review', 'migration'])}`.trim();
  while (topicNames.has(name));
  topicNames.add(name);
  const active = i < 83;
  return {
    id: i + 1,
    name,
    category: wpick(CATS),
    active,
    lastDay: active ? TODAY : addDays(FIRST_DAY, int(0, 28 - QUIET_DAYS - 2)),
    weight: 1 / (1 + (i % 83)) ** 0.6,
  };
});

const GROUP_KINDS = [
  ['team', 14],
  ['project', 16],
  ['cross', 8],
  ['social', 4],
];
const groups = Array.from({ length: 42 }, (_, i) => {
  const kind = wpick(GROUP_KINDS);
  const size = kind === 'social' ? int(40, 120) : kind === 'cross' ? int(15, 40) : int(4, 18);
  const prefix = kind === 'social' ? 'fun' : kind === 'team' ? 'team' : 'proj';
  return { id: i + 1, name: `${prefix}-${pick(NOUN)}-${i + 1}`, kind, size, members: [], topicIds: [], crews: [] };
});
const groupWeek = new Map(groups.map((g) => [g.id, Array.from({ length: WEEKS }, () => lognormal(0.4))]));
shuffle(people.map((p) => p.id)).forEach((id, i) => groups[i % groups.length].members.push({ id }));
for (const g of groups) {
  const have = new Set(g.members.map((m) => m.id));
  const more = wsample(
    people.map((p) => p.id).filter((id) => !have.has(id)),
    Math.max(0, g.size - g.members.length),
    (id) => baseWeight.get(id),
  );
  g.members.push(...more.map((id) => ({ id })));
  if (g.kind !== 'social') {
    for (const m of g.members) {
      const r = rnd();
      if (r < 0.04) m.joined = addDays(FIRST_DAY, int(7, 27));
      else if (r < 0.08) m.left = addDays(FIRST_DAY, int(5, 24));
    }
    g.members.push({ id: ME.id });
  }
  for (const m of g.members) byId.get(m.id).groups.push(g.id);
}
const activeOn = (g, day) =>
  g.members
    .filter((m) => m.id !== ME.id && (!m.joined || m.joined <= day) && (!m.left || day < m.left))
    .map((m) => m.id);
const memberOn = (g, id, day) => id === ME.id || activeOn(g, day).includes(id);
topics.forEach((t, i) => {
  groups[i % groups.length].topicIds.push(t.id);
  if (rnd() < 0.35) pick(groups).topicIds.push(t.id);
});
for (const g of groups) {
  const core = wsample(
    g.members.filter((m) => m.id !== ME.id && !m.left).map((m) => m.id),
    10,
    (id) => baseWeight.get(id),
  );
  for (let c = 0; c < int(2, 4); c++)
    g.crews.push(wsample(core, Math.min(core.length, int(3, 5)), (id) => baseWeight.get(id)).sort((a, b) => a - b));
}

const LOCAL = [
  [17, 1],
  [18, 1],
  [19, 1],
  [20, 1],
  [21, 1],
  [22, 1],
  [23, 1],
  [1, 1],
  [2, 1],
  [3, 1],
  [4, 1],
  [5, 2],
  [6, 3],
  [7, 5],
  [8, 8],
  [9, 8],
  [10, 10],
  [11, 9],
  [12, 10],
  [13, 9],
  [14, 5],
  [15, 5],
  [16, 3],
];
const AGENT_HOURS = [
  [9, 2],
  [10, 4],
  [11, 4],
  [12, 2],
  [13, 4],
  [14, 5],
  [15, 5],
  [16, 4],
  [17, 3],
  [18, 2],
];
const ts = (day, h, m = int(0, 59)) => `${day}T${String(h).padStart(2, '0')}:${String(m).padStart(2, '0')}:00`;
const THREAD_SIZE = [
  [1, 30],
  [2, 28],
  [3, 14],
  [4, 10],
  [5, 6],
  [6, 4],
  [7, 3],
  [8, 2],
  [9, 1],
  [12, 2],
];
const GAP_MIN = [
  [[0, 1], 25],
  [[1, 3], 25],
  [[3, 40], 25],
  [[40, 700], 15],
  [[700, 2000], 10],
];

// [source, kind, per weekday, hour shape, mean msg_count, title len, body len, owned share, due share]
const MIX = [
  ['slack', 'message', 64, LOCAL, 1, 17, 261, 0, 0],
  ['slack', 'thread', 59, LOCAL, 3.7, 30, 745, 0, 0],
  ['agents', 'approval', 51, AGENT_HOURS, 1, 28, 356, 0, 0],
  ['gmail', 'email', 30, LOCAL, 1.4, 71, 1221, 0, 0],
  ['jira', 'issue', 13, LOCAL, 1.9, 72, 983, 0.75, 0.13],
  ['agents', 'turn', 11, AGENT_HOURS, 1, 32, 365, 0, 0],
  ['agents', 'ask', 6, AGENT_HOURS, 1, 29, 423, 0, 0],
  ['obsidian', 'note', 2, LOCAL, 0, 14, 195, 0, 0],
  ['github', 'pr', 0.4, LOCAL, 1, 111, 1158, 1, 0],
];

const msgCount = (mean, atLeast) => {
  if (mean <= 1) return mean;
  if (atLeast < 2 && rnd() > (mean - 1) / 3.5 + 0.1) return 1;
  return Math.max(atLeast, Math.min(60, 2 + Math.floor(-Math.log(1 - rnd()) * mean * 1.8)));
};
const topicFor = (g, day) => {
  const open = (g ? g.topicIds.map((id) => topics[id - 1]) : topics).filter((t) => t.lastDay >= day);
  return open.length ? wpick(open.map((t) => [t, t.weight])) : null;
};
const groupPick = (kinds, day) =>
  wpick(
    groups
      .filter((g) => kinds.includes(g.kind) && activeOn(g, day).length)
      .map((g) => [g, Math.sqrt(g.size) * groupWeek.get(g.id)[weekOf(day)]]),
  );
const participantsIn = (g, source, day) => {
  const pool = activeOn(g, day);
  const n = source === 'slack' && rnd() > 0.5 ? wpick(THREAD_SIZE) : source === 'jira' ? int(1, 3) : 1;
  if (n >= 3 && rnd() < 0.3) {
    const crew = pick(g.crews).filter((id) => pool.includes(id));
    if (crew.length >= 3) return crew;
  }
  return wsample(pool, n, weightOn(day));
};
const threadTimes = (start, n) => {
  const at = [start];
  while (at.length < n) {
    const [a, b] = wpick(GAP_MIN);
    const next = addMin(at.at(-1), a + rnd() * (b - a));
    if (next > NOW) break;
    at.push(next);
  }
  return at;
};

const items = [];
const events = [];
let id = 0;
for (const day of DAYS) {
  const scale = WEEKDAY_SCALE[weekday(day)];
  for (const [source, kind, per, hours, mean, tl, bl, owned, due] of MIX) {
    const want = per * scale * (0.85 + rnd() * 0.3);
    const n = Math.floor(want) + (rnd() < want % 1 ? 1 : 0);
    for (let k = 0; k < n; k++) {
      const at = ts(day, wpick(hours));
      if (at > NOW) continue;
      const noPeople = source === 'agents' || source === 'obsidian';
      const external = source === 'gmail' && rnd() < 0.25;
      const g =
        noPeople || external
          ? null
          : groupPick(kind === 'message' ? ['team', 'project', 'cross', 'social'] : ['team', 'project', 'cross'], day);
      let parts = [];
      let to = [];
      let cc = [];
      if (external) {
        parts = [wpick(externals.map((p) => [p.id, weightOn(day)(p.id)]))];
        to = [ME.id];
      } else if (source === 'gmail') {
        const pool = activeOn(g, day);
        parts = wsample(pool, 1, weightOn(day));
        const rest = pool.filter((p) => p !== parts[0]);
        to = [ME.id, ...wsample(rest, int(0, 2), weightOn(day))];
        cc = wsample(
          rest.filter((p) => !to.includes(p)),
          wpick([
            [0, 45],
            [1, 20],
            [2, 15],
            [3, 10],
            [5, 10],
          ]),
          weightOn(day),
        );
      } else if (!noPeople) parts = participantsIn(g, source, day);
      const topic =
        external && rnd() < 0.6
          ? wpick(
              topics
                .filter((t) => ['vendors', 'customers'].includes(t.category) && t.lastDay >= day)
                .map((t) => [t, t.weight]),
            )
          : !external && rnd() < 0.8
            ? topicFor(rnd() < 0.2 ? null : g, day)
            : null;
      const count = msgCount(source === 'slack' && kind === 'message' ? 1 : mean, parts.length);
      const it = {
        id: ++id,
        source,
        kind,
        title:
          source === 'jira'
            ? `${pick(PROJ)}-${int(100, 9999)}: ${title(tl)}`
            : title(int(Math.max(8, tl - 10), tl + 10)),
        summary: parts.length ? sentence(8, 18) : null,
        body: text(Math.round(bl * (0.4 + rnd() * 1.2))),
        authorId: parts[0] ?? null,
        participantIds: [...new Set([...parts, ...to, ...cc])],
        groupId: g?.id ?? null,
        topicId: topic?.id ?? null,
        occurredAt: at,
        status: 'open',
        msgCount: count,
        seen: !(day >= addDays(TODAY, -2) && rnd() < 0.1),
        owner: rnd() < owned ? 'you' : null,
        dueDay: rnd() < due ? addDays(day, int(1, 10)) : null,
      };
      if (source === 'gmail') Object.assign(it, { to, cc });
      if (source === 'slack') it.channel = `#${g.name}`;
      if (source === 'github') it.repo = pick(REPO);
      if (source === 'jira') it.assigneeId = it.owner ? ME.id : pick(parts);
      if (count > 1) {
        const meIn = rnd() < 0.25;
        const times = threadTimes(at, count);
        it.msgCount = times.length;
        it.endedAt = times.at(-1);
        const replyPool = it.participantIds.filter((p) => p !== ME.id);
        it.messages = times.slice(0, 6).map((t, m) => ({
          at: t,
          authorId: m === 0 ? parts[0] : m < parts.length ? parts[m] : meIn && rnd() < 0.75 ? ME.id : pick(replyPool),
          text: text(int(40, 320)),
        }));
        if (meIn && !it.messages.some((m) => m.authorId === ME.id) && it.messages.length > 1)
          it.messages.at(-1).authorId = ME.id;
        if (it.messages.some((m) => m.authorId === ME.id)) it.participantIds.push(ME.id);
        if (it.msgCount === 1) (delete it.messages, delete it.endedAt);
      }
      if ((source === 'jira' || source === 'github') && rnd() < 0.3) it.status = 'done';
      items.push(it);
    }
  }
}

const series = [];
const addSeries = (n, every, size, kinds, minutes, hours) => {
  for (let i = 0; i < n; i++) {
    const g = groupPick(kinds, TODAY);
    const pool = g.members.filter((m) => m.id !== ME.id).map((m) => m.id);
    const w = (id) => baseWeight.get(id);
    const others =
      size === 2
        ? wsample(pool, 1, w)
        : wsample(pool, every === 'monthly' ? Math.min(pool.length, int(25, 37)) : int(3, 9), w);
    series.push({
      id: series.length + 1,
      every,
      guestIds: [ME.id, ...others],
      groupId: g.id,
      topicId: topicFor(g, TODAY)?.id ?? null,
      title: title(int(12, 30)),
      minutes,
      hour: pick(hours),
      weekday: int(0, 4),
      phase: int(0, every === 'monthly' ? 3 : 1),
    });
  }
};
addSeries(12, 'weekly', 2, ['team', 'project'], 30, [8, 9, 10, 11, 13, 14]);
addSeries(8, 'weekly', 0, ['team', 'project'], 60, [9, 10, 12, 13]);
addSeries(14, 'biweekly', 0, ['team', 'project', 'cross'], 45, [8, 10, 11, 13, 14]);
addSeries(2, 'daily', 0, ['team'], 15, [9]);
addSeries(2, 'monthly', 0, ['cross'], 60, [14]);
const pushEvent = (day, hour, minutes, guestIds, ttl, topicId, seriesId, groupId) => {
  const start = ts(day, hour, pick([0, 0, 0, 30]));
  const past = start < NOW;
  const it = {
    id: ++id,
    source: 'gcal',
    kind: 'event',
    title: ttl,
    summary: null,
    body: text(int(40, 240)),
    authorId: guestIds[1] ?? ME.id,
    participantIds: guestIds,
    groupId,
    topicId,
    occurredAt: start,
    status: past ? 'done' : 'open',
    msgCount: 0,
    seen: true,
    owner: null,
    dueDay: null,
  };
  items.push(it);
  events.push({
    id: events.length + 1,
    itemId: it.id,
    seriesId,
    title: ttl,
    start,
    end: addMin(start, minutes),
    tentative: rnd() < 0.1,
    guestIds,
    topicId,
    category: topicId ? topics[topicId - 1].category : wpick(CATS),
  });
};
for (const day of [...DAYS, ...FUTURE]) {
  const wd = weekday(day);
  const week = weekOf(day);
  if (wd > 4) continue;
  for (const sr of series) {
    const on =
      sr.every === 'daily' ||
      (sr.every === 'weekly' && sr.weekday === wd) ||
      (sr.every === 'biweekly' && sr.weekday === wd && week % 2 === sr.phase) ||
      (sr.every === 'monthly' && sr.weekday === wd && week % 4 === sr.phase);
    if (on && rnd() > 0.05) {
      const g = groups[sr.groupId - 1];
      const guests = sr.guestIds.filter((p) => memberOn(g, p, day));
      pushEvent(day, sr.hour, sr.minutes, guests, sr.title, sr.topicId, sr.id, sr.groupId);
    }
  }
  const oneOffs = day <= TODAY ? int(1, 3) : day <= addDays(TODAY, 14) ? int(0, 2) : int(0, 1);
  for (let k = 0; k < oneOffs; k++) {
    const g = groupPick(['team', 'project', 'cross'], day > TODAY ? TODAY : day);
    const guests = [ME.id, ...wsample(activeOn(g, day), int(1, 5), weightOn(day))];
    const topic = topicFor(g, day > TODAY ? TODAY : day);
    pushEvent(day, int(8, 14), pick([30, 30, 45, 60]), guests, title(int(12, 30)), topic?.id ?? null, null, g.id);
  }
}
items.sort((x, y) => x.occurredAt.localeCompare(y.occurredAt));

const nextMeeting = new Map();
const nextFor = (k, e) => {
  const cur = nextMeeting.get(k);
  if (!cur || e.start < cur.start) nextMeeting.set(k, { eventId: e.id, start: e.start, title: e.title });
};
for (const e of events)
  if (e.start >= NOW) {
    if (e.topicId) nextFor(`t${e.topicId}`, e);
    for (const g of e.guestIds) if (g !== ME.id) nextFor(`p${g}`, e);
  }

const ranked = [...people].sort((a, b) => baseWeight.get(b.id) - baseWeight.get(a.id));
const noted = new Set([...ranked.slice(0, 75), ...wsample(ranked.slice(75), 7, () => 1)].map((p) => p.id));
for (const p of people) p.notePath = noted.has(p.id) ? `People/${p.name}.md` : null;
for (const p of externals) p.notePath = null;

const copies = [];
const copyOf = new Map();
const EXTRA_ROWS = [
  [1, 107],
  [2, 41],
  [3, 9],
];
const COPY_SOURCE = [
  ['gmail', 50],
  ['slack', 35],
  ['jira', 15],
];
const activity = tally(
  items.filter((i) => i.occurredAt <= NOW).flatMap((i) => i.participantIds),
  (p) => p,
);
const dupHumans = wsample(
  people.filter((p) => !noted.has(p.id) && activity[p.id]),
  90,
  (p) => Math.sqrt(activity[p.id]),
);
const used = new Set(all.flatMap((p) => [p.handle, p.email]));
const unique = (make) => {
  for (let n = 1; ; n++) {
    const v = make(n === 1 ? '' : String(n));
    if (!used.has(v)) return (used.add(v), v);
  }
};
shuffle(dupHumans).forEach((human, n) => {
  const extra = wpick(EXTRA_ROWS);
  for (let k = 0; k < extra; k++) {
    const theirs = COPY_SOURCE.filter(([s]) =>
      items.some((i) => i.source === s && i.occurredAt <= NOW && i.participantIds.includes(human.id)),
    );
    const source = wpick(theirs.length ? theirs : COPY_SOURCE);
    const [first, last] = human.name.toLowerCase().split(' ');
    const alias = pick([`${first[0]}${last}`, `${first}.${last}`, `${first}${last[0]}`]);
    const domain = pick(['example.com', 'mail.example.com', 'example.org']);
    const c = {
      id: 661 + copies.length,
      name: human.name,
      handle:
        source === 'slack'
          ? unique((n) => `${alias}${n}`)
          : source === 'jira'
            ? unique((n) => `jira-${alias}${n}`)
            : null,
      email: source === 'gmail' ? unique((n) => `${alias}${n}@${domain}`) : null,
      source,
      groups: human.groups,
      notePath: null,
      copyOf: human.id,
    };
    copies.push(c);
    copyOf.set(c.id, human.id);
  }
});
const rowsFor = (human, source) => copies.filter((c) => c.copyOf === human && c.source === source);
const swap = (id, source, groupId) => {
  const rows = rowsFor(id, source).map((c) => c.id);
  const options = source === 'gmail' ? [id, ...rows] : rows;
  return options.length ? options[(groupId ?? id) % options.length] : id;
};
for (const it of items) {
  if (!['gmail', 'slack', 'jira'].includes(it.source)) continue;
  const map = new Map(it.participantIds.map((p) => [p, swap(p, it.source, it.groupId)]));
  const to = (p) => map.get(p) ?? p;
  it.authorId = it.authorId == null ? null : to(it.authorId);
  it.participantIds = it.participantIds.map(to);
  if (it.to) it.to = it.to.map(to);
  if (it.cc) it.cc = it.cc.map(to);
  if (it.assigneeId != null) it.assigneeId = to(it.assigneeId);
  for (const m of it.messages || []) m.authorId = to(m.authorId);
}
const relabel = (it, from, to) => {
  const r = (p) => (p === from ? to : p);
  it.authorId = it.authorId == null ? null : r(it.authorId);
  it.participantIds = it.participantIds.map(r);
  if (it.to) it.to = it.to.map(r);
  if (it.cc) it.cc = it.cc.map(r);
  if (it.assigneeId != null) it.assigneeId = r(it.assigneeId);
  for (const m of it.messages || []) m.authorId = r(m.authorId);
};
const holds = (id) => items.some((i) => i.occurredAt <= NOW && i.participantIds.includes(id));
for (const c of copies)
  if (!holds(c.id) && rnd() < 0.85) {
    const theirs = items.filter(
      (i) => i.source === c.source && i.occurredAt <= NOW && i.participantIds.includes(c.copyOf),
    );
    if (theirs.length) relabel(pick(theirs), c.copyOf, c.id);
  }
const SYNC_OF = { gmail: 'gmail', gcal: 'gmail', slack: 'slack', jira: 'jira', github: 'github' };
const variant = (name) => {
  const [first, last] = name.split(' ');
  return pick([`${first} ${last[0]}.`, first, `${first.toLowerCase()}.${last.toLowerCase()}`, `${first[0]}. ${last}`]);
};
for (const n of people.filter((p) => noted.has(p.id))) {
  const bySource = {};
  for (const it of items) {
    const s = SYNC_OF[it.source];
    if (s && it.participantIds.includes(n.id)) bySource[s] = (bySource[s] || 0) + (it.occurredAt <= NOW ? 1 : 0.01);
  }
  const rows = Object.entries(bySource)
    .sort((a, b) => b[1] - a[1])
    .map(([source]) => {
      const row = {
        id: 661 + copies.length,
        name: rnd() < 0.45 ? variant(n.name) : n.name,
        handle:
          source === 'slack'
            ? n.handle
            : source === 'jira' || source === 'github'
              ? unique((k) => `${source}-${n.handle}${k}`)
              : null,
        email: source === 'gmail' ? n.email : null,
        source,
        groups: n.groups,
        notePath: null,
        copyOf: n.id,
      };
      copies.push(row);
      copyOf.set(row.id, n.id);
      return row;
    });
  for (const it of items) {
    const row = rows.find((r) => r.source === SYNC_OF[it.source]);
    if (row) relabel(it, n.id, row.id);
  }
  n.noteAccounts = rows.filter((_, k) => k === 0 || rnd() < 0.25).map((r) => r.id);
  n.handle = null;
  n.email = null;
}
const eventItem = new Map(items.map((i) => [i.id, i]));
for (const e of events) e.guestIds = eventItem.get(e.itemId).participantIds;
const human = (id) => copyOf.get(id) ?? id;

const SERVICES = [
  ['Northwind', 32],
  ['Chatterbox', 13],
  ['Trackwell', 9],
  ['Codehub', 8],
];
const LOCAL_PARTS =
  'noreply notifications billing security team news support alerts digest invoices accounts hello updates events no-reply mailer'.split(
    ' ',
  );
const services = SERVICES.flatMap(([name, n]) => {
  const domain = `${name.toLowerCase()}.example`;
  const sub = ['', 'mail.', 'email.', 'notify.', 'billing.'];
  return Array.from({ length: n }, (_, k) => ({
    name,
    handle: null,
    email: `${LOCAL_PARTS[k % LOCAL_PARTS.length]}@${sub[Math.floor(k / LOCAL_PARTS.length)]}${domain}`,
    source: 'gmail',
    groups: [],
    notePath: null,
    service: true,
  }));
}).map((row, k) => ({ id: 661 + copies.length + k, ...row }));
for (const s of services) byId.set(s.id, s);
const serviceMail = shuffle(
  items.filter((i) => i.source === 'gmail' && byId.get(i.authorId)?.external && !i.messages),
).slice(0, services.length * 2);
serviceMail.forEach((it, k) => {
  const row = k < services.length ? services[k] : wpick(services.map((s) => [s, s.name === 'Northwind' ? 3 : 1]));
  it.authorId = row.id;
  it.participantIds = [row.id, ...it.participantIds.filter((p) => p === ME.id)];
  it.topicId = null;
});
for (const it of items)
  if (it.topicId && it.occurredAt <= NOW) {
    const t = topics[it.topicId - 1];
    t.itemCount = (t.itemCount || 0) + 1;
    if (!t.lastSignalAt || it.occurredAt > t.lastSignalAt) t.lastSignalAt = it.occurredAt;
  }

const past = items.filter((i) => i.occurredAt <= NOW);
const shape = (() => {
  const multi = past.filter((i) => i.source === 'slack' && i.participantIds.filter((p) => p !== ME.id).length >= 3);
  const sets = tally(multi, (i) => key(i.participantIds.filter((p) => p !== ME.id)));
  const pt = new Map();
  for (const i of past)
    for (const p of i.participantIds)
      if (i.topicId && p !== ME.id) {
        const m = pt.get(p) || new Map();
        m.set(i.topicId, (m.get(i.topicId) || 0) + 1);
        pt.set(p, m);
      }
  const buckets = {};
  for (const m of pt.values()) {
    const total = [...m.values()].reduce((a, b) => a + b, 0);
    const top3 =
      [...m.values()]
        .sort((a, b) => b - a)
        .slice(0, 3)
        .reduce((a, b) => a + b, 0) / total;
    const b = total < 5 ? '1-4' : total < 20 ? '5-19' : total < 60 ? '20-59' : '60+';
    (buckets[b] ||= []).push([m.size, top3]);
  }
  const pastEvents = events.filter((e) => e.start < NOW && e.guestIds.length >= 3);
  const lists = Object.values(tally(pastEvents, (e) => key(e.guestIds)));
  const msgs = past.flatMap((i) => (i.messages || []).map((m) => ({ ...m, i })));
  const outOfOrder = past.filter((i) => i.messages?.some((m, k) => m.at < (k ? i.messages[k - 1].at : i.occurredAt)));
  const perWeek = new Map();
  for (const i of past)
    if (i.authorId && i.authorId !== ME.id && i.occurredAt < TODAY) {
      const w = perWeek.get(i.authorId) || [0, 0, 0, 0];
      w[weekOf(i.occurredAt.slice(0, 10))]++;
      perWeek.set(i.authorId, w);
    }
  const top = [...perWeek.values()].sort((a, b) => b.reduce((x, y) => x + y) - a.reduce((x, y) => x + y));
  const cv = top.slice(0, 30).map((w) => {
    const m = w.reduce((a, b) => a + b) / 4;
    return Math.sqrt(w.reduce((a, b) => a + (b - m) ** 2, 0) / 4) / m;
  });
  const top100 = tally(
    top.slice(0, 100),
    (w) => `${w[0] > 0 ? 'in' : 'out'} week 1, ${w[3] > 0 ? 'in' : 'out'} week 4`,
  );
  const quiet = topics.filter((t) => !t.lastSignalAt || t.lastSignalAt < quietBefore).length;
  if (outOfOrder.length) throw new Error(`${outOfOrder.length} threads out of order`);
  if (new Set(all.map((p) => p.name)).size !== all.length) throw new Error('duplicate person names');
  if (new Set(topics.map((t) => t.name)).size !== topics.length) throw new Error('duplicate topic names');
  if (past.some((i) => i.authorId === ME.id && i.source !== 'gcal')) throw new Error('me as signal author');
  const addrs = [...all, ...copies, ...services].flatMap((p) => [p.email, p.handle && `${p.source}:${p.handle}`]);
  const shared = addrs.filter((a, k) => a && addrs.indexOf(a) !== k);
  if (shared.length) throw new Error(`addresses on two rows: ${shared.slice(0, 5)}`);
  for (const t of topics) {
    const mine = past.filter((i) => i.topicId === t.id);
    if ((t.itemCount || 0) !== mine.length) throw new Error(`topic ${t.id} counts ${t.itemCount}, has ${mine.length}`);
  }
  const strayGuests = events.filter((e) => {
    const g = e.seriesId && groups[series[e.seriesId - 1].groupId - 1];
    return g && e.guestIds.some((p) => !memberOn(g, human(p), e.start.slice(0, 10)));
  });
  if (strayGuests.length) throw new Error(`${strayGuests.length} meetings with guests outside the group that day`);
  if (items.some((i) => i.participantIds.some((p) => noted.has(p)))) throw new Error('a note row holds a signal');
  const mail = past.filter((i) => i.source === 'gmail' && i.messages);
  const mailReplies = mail.flatMap((i) => i.messages.slice(1));
  return {
    slackGroupRepeatShare: +(
      multi.filter((i) => sets[key(i.participantIds.filter((p) => p !== ME.id))] > 1).length / multi.length
    ).toFixed(2),
    personTopics: Object.fromEntries(
      Object.entries(buckets).map(([b, r]) => [
        b,
        {
          people: r.length,
          medianTopics: median(r.map((x) => x[0])),
          medianTop3Share: +median(r.map((x) => x[1])).toFixed(2),
        },
      ]),
    ),
    me: {
      threadMessageShare: +(msgs.filter((m) => m.authorId === ME.id).length / msgs.length).toFixed(2),
      threadsWithMe: +(
        past.filter((i) => i.messages?.some((m) => m.authorId === ME.id)).length / past.filter((i) => i.messages).length
      ).toFixed(2),
    },
    notes: {
      people: noted.size,
      threadMessageShare: +(msgs.filter((m) => noted.has(human(m.authorId))).length / msgs.length).toFixed(2),
    },
    duplicates: {
      rows: all.length - 1 + copies.length + services.length,
      namesWithCopies: new Set(copies.map((c) => c.copyOf)).size + SERVICES.length,
      services: Object.fromEntries(SERVICES),
      extraRows: copies.length,
      notesWithUnclaimedRow: people.filter(
        (n) =>
          n.noteAccounts &&
          copies.some((c) => c.copyOf === n.id && c.name === n.name && !n.noteAccounts.includes(c.id)),
      ).length,
      noteSameNameRows: (() => {
        const same = copies.filter((c) => noted.has(c.copyOf) && c.name === byId.get(c.copyOf).name);
        const claimed = same.filter((c) => byId.get(c.copyOf).noteAccounts.includes(c.id)).length;
        return { rows: same.length, claimed, notesWithNone: noted.size - new Set(same.map((c) => c.copyOf)).size };
      })(),
      noteRowsWithSignals: past.filter((i) => i.participantIds.some((p) => noted.has(p))).length,
      copiesBySource: tally(copies, (c) => c.source),
      signalsByCopies: past.filter((i) => copyOf.has(i.authorId)).length,
      emptyCopies: copies.filter((c) => !holds(c.id)).length,
    },
    emailReplies: tally(mailReplies, (m) =>
      m.authorId === ME.id
        ? 'me'
        : mail.find((i) => i.messages.includes(m)).authorId === m.authorId
          ? 'sender'
          : 'to/cc',
    ),
    meetingGuestLists: { distinct: lists.length, usedTwicePlus: lists.filter((c) => c > 1).length },
    upcomingMeetings: events.filter((e) => e.start >= NOW).length,
    topAuthorsByWeek: top.slice(0, 6),
    topAuthorsWeeklyCv: +median(cv).toFixed(2),
    top100Authors: top100,
    topics: {
      active: topics.length - quiet,
      quiet,
      withNextMeeting: topics.filter((t) => nextMeeting.has(`t${t.id}`)).length,
    },
  };
})();

const data = {
  about: [
    `Invented data at the density of a real account: four weeks of signals (${FIRST_DAY} to ${TODAY}), now is ${NOW} (ALETHIOMETER_FAKE_NOW=10:52), and calendar events through ${HORIZON}. Nothing here comes from a real person or message. Times are local wall-clock.`,
    "People (id 0 is me) belong to groups: teams, projects, cross-team and social channels. A group member can have joined or left during the four weeks (members[].joined / left), and recurring meetings only include guests who are in the group that day. crews are the small sets inside a group who often talk together. Signals draw their people and topic from one group, so the same people keep showing up together and people have home topics. People are busier some weeks than others. About 80 people have a People note (notePath), mostly the busiest, as on the real account (82 notes, whose people write about half the thread messages); everyone else is an account with no note. As in the real store, a note row holds no signals: each source has its own row for that person (a Gmail row, a Slack row, a Jira row, same name, no note), the signals point at those rows, and the note's noteAccounts lists the rows it claims (the app shows claimed rows as the note's person). Same-name rows a note doesn't claim are merge candidates, as are the bare copies about 90 people without a note have: an extra Gmail address, Slack account or Jira account, holding that source's signals; about one in seven has none, as on the real store. copyOf points every such row at its person (for checking; the app never sees it, only matching names and noteAccounts). Groups, crews, series and nextMeeting use the person's id (the note row for noted people). The biggest same-name groups are service senders (service: true, e.g. Northwind with 32 rows), one bare Gmail row per sending address (noreply@, billing@, \u2026), as on the real store, where the largest are company and tool senders (32, 13, 9 and 8 rows).",
    'I never author a signal (my own posts are not signals); I write some thread messages. Email: authorId is the sender, to and cc are recipients from the same group, and anyone on the mail can reply; about a quarter of mail comes from external people (external: true) with no group. Jira has an assigneeId.',
    'messages holds at most the first 6 of a thread, in time order from occurredAt; msgCount is the full length and endedAt the last message. Meetings come from recurring series (seriesId, fixed guest lists) plus one-offs; topics and people carry nextMeeting. Future meetings are items too (status open, after now), as in the real store. A topic is quiet with no signal in 7 days.',
    'Triage is not modelled: on the real account about half of the signals triaged in Incoming are dismissed and half kept (Slack 147/165, Gmail 78 dismissed + 16 trashed/43 kept, Jira 25/18).',
    'shape reports the measured density. Regenerate with node app/scripts/gen-fake-account.mjs.',
  ].join(' '),
  shape,
  now: NOW,
  me: { id: ME.id, name: ME.name, handle: ME.handle, email: ME.email },
  days: DAYS,
  countsBySourceKind: tally(past, (i) => `${i.source}/${i.kind}`),
  countsByDay: tally(past, (i) => i.occurredAt.slice(0, 10)),
  people: [...all.slice(1), ...copies, ...services].map((p) => ({
    ...p,
    nextMeeting: p.copyOf == null ? (nextMeeting.get(`p${p.id}`) ?? null) : null,
  })),
  groups: groups.map(({ size: _, ...g }) => g),
  topics: topics.map(({ weight: _, lastDay: __, active: ___, ...t }) => ({
    ...t,
    itemCount: t.itemCount || 0,
    lastSignalAt: t.lastSignalAt || null,
    quiet: !t.lastSignalAt || t.lastSignalAt < quietBefore,
    nextMeeting: nextMeeting.get(`t${t.id}`) ?? null,
  })),
  series: series.map(({ hour: _, weekday: __, phase: ___, ...sr }) => sr),
  items,
  events,
};
mkdirSync(out, { recursive: true });
writeFileSync(join(out, 'fake-account.json'), JSON.stringify(data));
writeFileSync(join(out, 'fake-account.js'), `window.FAKE_ACCOUNT = ${JSON.stringify(data)};\n`);
console.log(JSON.stringify(shape, null, 1), items.length, 'items', events.length, 'events');
