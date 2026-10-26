// Invented work weeks for design mocks. Run: node app/scripts/gen-busy-week.mjs
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
const cap = (t) => t[0].toUpperCase() + t.slice(1);

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

const WEEKS = 4;
const DAYS = Array.from({ length: WEEKS * 7 }, (_, i) =>
  new Date(Date.UTC(2026, 8, 28 + i)).toISOString().slice(0, 10),
);
const TODAY = DAYS.at(-1);
const QUIET_DAYS = 7;
const WEEKDAY_SCALE = [0.85, 1.0, 1.15, 1.2, 0.95, 0.07, 0.04];

const ME = { id: 0, name: 'Sam Rivera', handle: 'sam.rivera', email: 'sam.rivera@example.com', weight: 40, groups: [] };
const names = shuffle(FIRST.flatMap((f) => LAST.map((l) => `${f} ${l}`))).slice(0, 600);
const people = names.map((name, i) => {
  const h = name.toLowerCase().replace(' ', '.');
  return { id: i + 1, name, handle: h, email: `${h}@example.com`, weight: 1 / (i + 4) ** 1.15, groups: [] };
});
const byId = new Map([ME, ...people].map((p) => [p.id, p]));
const weight = (id) => byId.get(id).weight;

const topics = Array.from({ length: 184 }, (_, i) => ({
  id: i + 1,
  name: `${cap(pick(ADJ))} ${pick(NOUN)} ${pick(['', 'v2', 'cleanup', 'Q4', 'rollout', 'review', 'migration'])}`.trim(),
  category: wpick(CATS),
  active: i < 83,
  lastDay: i < 83 ? TODAY : DAYS[int(0, DAYS.length - QUIET_DAYS - 2)],
  weight: 1 / (1 + (i % 83)) ** 0.6,
}));

const GROUP_KINDS = [
  ['team', 14],
  ['project', 16],
  ['cross', 8],
  ['social', 4],
];
const groups = Array.from({ length: 42 }, (_, i) => {
  const kind = wpick(GROUP_KINDS);
  const size = kind === 'social' ? int(40, 120) : kind === 'cross' ? int(15, 40) : int(4, 18);
  return {
    id: i + 1,
    name: `${kind === 'social' ? 'fun' : kind === 'team' ? 'team' : 'proj'}-${pick(NOUN)}-${i + 1}`,
    kind,
    size,
    members: [],
    topicIds: [],
    crews: [],
  };
});
shuffle(people.map((p) => p.id)).forEach((id, i) => groups[i % groups.length].members.push(id));
for (const g of groups) {
  const more = wsample(
    people.map((p) => p.id).filter((id) => !g.members.includes(id)),
    Math.max(0, g.size - g.members.length),
    weight,
  );
  g.members.push(...more);
  if (g.kind !== 'social') g.members.push(ME.id);
  for (const id of g.members) byId.get(id).groups.push(g.id);
  g.size = g.members.length;
}
topics.forEach((t, i) => {
  const home = groups[i % groups.length];
  home.topicIds.push(t.id);
  if (rnd() < 0.35) pick(groups).topicIds.push(t.id);
});
for (const g of groups) {
  const core = wsample(g.members, Math.min(g.members.length, 10), weight);
  for (let c = 0; c < int(2, 4); c++) g.crews.push(wsample(core, Math.min(core.length, int(3, 5)), weight));
}

const HOURS = [
  [0, 1],
  [1, 1],
  [2, 1],
  [3, 1],
  [4, 1],
  [5, 1],
  [6, 1],
  [8, 1],
  [9, 1],
  [10, 1],
  [11, 1],
  [12, 2],
  [13, 3],
  [14, 5],
  [15, 8],
  [16, 8],
  [17, 10],
  [18, 9],
  [19, 10],
  [20, 9],
  [21, 5],
  [22, 5],
  [23, 3],
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

// [source, kind, per weekday, hour shape, mean msg_count, title len, body len, owned share, due share]
const MIX = [
  ['slack', 'message', 64, HOURS, 1, 17, 261, 0, 0],
  ['slack', 'thread', 59, HOURS, 3.7, 30, 745, 0, 0],
  ['agents', 'approval', 51, AGENT_HOURS, 1, 28, 356, 0, 0],
  ['gmail', 'email', 30, HOURS, 1.4, 71, 1221, 0, 0],
  ['jira', 'issue', 13, HOURS, 1.9, 72, 983, 0.75, 0.13],
  ['agents', 'turn', 11, AGENT_HOURS, 1, 32, 365, 0, 0],
  ['agents', 'ask', 6, AGENT_HOURS, 1, 29, 423, 0, 0],
  ['obsidian', 'note', 2, HOURS, 0, 14, 195, 0, 0],
  ['github', 'pr', 0.4, HOURS, 1, 111, 1158, 1, 0],
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
const groupPick = (kinds) => wpick(groups.filter((g) => kinds.includes(g.kind)).map((g) => [g, Math.sqrt(g.size)]));
const participantsIn = (g, source) => {
  if (source === 'gmail') return [wpick(people.map((p) => [p.id, p.weight + 0.002]))];
  const n = source === 'slack' && rnd() > 0.5 ? wpick(THREAD_SIZE) : source === 'jira' ? int(1, 3) : 1;
  if (n >= 3 && rnd() < 0.2) return [...pick(g.crews)];
  return wsample(g.members, n, weight);
};

const items = [];
const events = [];
let id = 0;
DAYS.forEach((day, di) => {
  const scale = WEEKDAY_SCALE[di % 7];
  for (const [source, kind, per, hours, mean, tl, bl, owned, due] of MIX) {
    const want = per * scale * (0.85 + rnd() * 0.3);
    const n = Math.floor(want) + (rnd() < want % 1 ? 1 : 0);
    for (let k = 0; k < n; k++) {
      const at = ts(day, wpick(hours));
      const people_ = source === 'agents' || source === 'obsidian';
      const g = people_
        ? null
        : groupPick(kind === 'message' ? ['team', 'project', 'cross', 'social'] : ['team', 'project', 'cross']);
      const parts = people_ ? [] : participantsIn(g, source);
      const count = msgCount(source === 'slack' && kind === 'message' ? 1 : mean, parts.length);
      const topic = rnd() < 0.8 ? topicFor(g, day) : null;
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
        participantIds: parts,
        groupId: g?.id ?? null,
        topicId: topic?.id ?? null,
        occurredAt: at,
        status: 'open',
        msgCount: count,
        seen: !(di >= DAYS.length - 3 && rnd() < 0.1),
        owner: rnd() < owned ? 'you' : null,
        dueDay: rnd() < due ? DAYS[Math.min(DAYS.length - 1, di + int(1, 6))] : null,
        channel: source === 'slack' ? `#${g.name}` : undefined,
        repo: source === 'github' ? pick(REPO) : undefined,
      };
      if (count > 1) {
        it.messages = Array.from({ length: Math.min(count, 6) }, (_, m) => ({
          at: ts(day, int(+at.slice(11, 13), 23)),
          authorId: m < parts.length ? parts[m] : rnd() < 0.2 ? ME.id : pick(parts),
          text: text(int(40, 320)),
        }));
      }
      if ((source === 'jira' || source === 'github') && rnd() < 0.3) it.status = 'done';
      items.push(it);
    }
  }
});

const series = [];
const addSeries = (n, every, size, kinds, minutes, hourChoices) => {
  for (let i = 0; i < n; i++) {
    const g = groupPick(kinds);
    const guests =
      size === 2
        ? [ME.id, wpick(g.members.filter((m) => m !== ME.id).map((m) => [m, weight(m)]))]
        : [
            ME.id,
            ...wsample(
              g.members.filter((m) => m !== ME.id),
              size - 1,
              weight,
            ),
          ];
    series.push({
      id: series.length + 1,
      every,
      guests,
      groupId: g.id,
      topicId: topicFor(g, TODAY)?.id ?? null,
      title: title(int(12, 30)),
      minutes,
      hour: pick(hourChoices),
      weekday: int(0, 4),
      startWeek: every === 'monthly' ? int(0, 3) : every === 'biweekly' ? int(0, 1) : 0,
    });
  }
};
addSeries(12, 'weekly', 2, ['team', 'project'], 30, [9, 10, 11, 14, 15, 16]);
addSeries(8, 'weekly', 0, ['team', 'project'], 60, [10, 11, 13, 14]);
addSeries(14, 'biweekly', 0, ['team', 'project', 'cross'], 45, [9, 11, 13, 15, 16]);
addSeries(2, 'daily', 0, ['team'], 15, [9, 10]);
addSeries(2, 'monthly', 0, ['cross'], 60, [16]);
for (const sr of series) {
  if (sr.guests.length === 1) {
    const g = groups[sr.groupId - 1];
    sr.guests = [
      ME.id,
      ...wsample(
        g.members.filter((m) => m !== ME.id),
        sr.every === 'monthly' ? Math.min(g.size - 1, int(25, 37)) : int(3, 9),
        weight,
      ),
    ];
  }
}
const pushEvent = (day, hour, minutes, guests, ttl, topicId, seriesId, groupId) => {
  const start = ts(day, hour, pick([0, 0, 0, 30]));
  const it = {
    id: ++id,
    source: 'gcal',
    kind: 'event',
    title: ttl,
    summary: null,
    body: text(int(40, 240)),
    authorId: guests[1] ?? ME.id,
    participantIds: guests,
    groupId,
    topicId,
    occurredAt: start,
    status: day < TODAY ? 'done' : 'open',
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
    end: new Date(Date.parse(start + 'Z') + minutes * 60000).toISOString().slice(0, 19),
    tentative: rnd() < 0.1,
    guestIds: guests,
    category: topicId ? topics[topicId - 1].category : wpick(CATS),
  });
};
DAYS.forEach((day, di) => {
  const wd = di % 7;
  const week = Math.floor(di / 7);
  if (wd > 4) return;
  for (const sr of series) {
    const on =
      sr.every === 'daily' ||
      (sr.every === 'weekly' && sr.weekday === wd) ||
      (sr.every === 'biweekly' && sr.weekday === wd && week % 2 === sr.startWeek) ||
      (sr.every === 'monthly' && sr.weekday === wd && week === sr.startWeek);
    if (on && rnd() > 0.05) pushEvent(day, sr.hour, sr.minutes, sr.guests, sr.title, sr.topicId, sr.id, sr.groupId);
  }
  for (let k = 0; k < int(1, 3); k++) {
    const g = groupPick(['team', 'project', 'cross']);
    pushEvent(
      day,
      int(9, 16),
      pick([30, 30, 45, 60]),
      [
        ME.id,
        ...wsample(
          g.members.filter((m) => m !== ME.id),
          int(1, 5),
          weight,
        ),
      ],
      title(int(12, 30)),
      topicFor(g, day)?.id ?? null,
      null,
      g.id,
    );
  }
});
items.sort((x, y) => x.occurredAt.localeCompare(y.occurredAt));

for (const it of items)
  if (it.topicId) {
    const t = topics[it.topicId - 1];
    t.itemCount = (t.itemCount || 0) + 1;
    if (!t.lastSignalAt || it.occurredAt > t.lastSignalAt) t.lastSignalAt = it.occurredAt;
  }
const quietBefore = DAYS[DAYS.length - QUIET_DAYS];

const tally = (f) => items.reduce((m, i) => ((m[f(i)] = (m[f(i)] || 0) + 1), m), {});
const check = () => {
  const names = new Set(people.map((p) => p.name));
  if (names.size !== people.length) throw new Error('duplicate names');
  const multi = items.filter((i) => i.source === 'slack' && i.participantIds.length >= 3);
  const key = (ids) => [...ids].sort().join();
  const sets = multi.reduce((m, i) => ((m[key(i.participantIds)] = (m[key(i.participantIds)] || 0) + 1), m), {});
  const repeatShare = multi.filter((i) => sets[key(i.participantIds)] > 1).length / multi.length;
  const pt = new Map();
  for (const i of items)
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
  const med = (a) => a.sort((x, y) => x - y)[Math.floor(a.length / 2)];
  const guestLists = Object.values(
    events
      .filter((e) => e.guestIds.length >= 3)
      .reduce((m, e) => ((m[key(e.guestIds)] = (m[key(e.guestIds)] || 0) + 1), m), {}),
  );
  const quiet = topics.filter((t) => !t.lastSignalAt || t.lastSignalAt < quietBefore).length;
  return {
    slackGroupRepeatShare: +repeatShare.toFixed(2),
    personTopics: Object.fromEntries(
      Object.entries(buckets).map(([b, r]) => [
        b,
        {
          people: r.length,
          medianTopics: med(r.map((x) => x[0])),
          medianTop3Share: +med(r.map((x) => x[1])).toFixed(2),
        },
      ]),
    ),
    meetingGuestLists: { distinct: guestLists.length, usedTwicePlus: guestLists.filter((c) => c > 1).length },
    topics: { active: topics.length - quiet, quiet },
  };
};

const shape = check();
const data = {
  about: `Invented data at the density of a real account: ${WEEKS} weeks (${DAYS[0]} to ${TODAY}, today is ${TODAY}). Nothing here comes from a real person or message. People belong to groups (teams, projects, cross-team, social channels); signals and meetings draw their participants from a group, so the same people keep showing up together, and topics belong to groups, so people have home topics. Meetings come from recurring series (seriesId) with fixed guest lists plus one-offs. A topic is quiet with no signal in ${QUIET_DAYS} days. msgCount is the full thread length; messages holds at most the first 6. Ids in authorId, participantIds and guestIds point at people (0 is me). Regenerate with node app/scripts/gen-busy-week.mjs.`,
  shape,
  me: { id: ME.id, name: ME.name, handle: ME.handle, email: ME.email },
  days: DAYS,
  countsBySourceKind: tally((i) => `${i.source}/${i.kind}`),
  countsByDay: tally((i) => i.occurredAt.slice(0, 10)),
  people: people.map(({ weight: _, ...p }) => p),
  groups: groups.map(({ crews: _, size: __, ...g }) => g),
  topics: topics.map(({ weight: _, lastDay: __, active: ___, ...t }) => ({
    ...t,
    itemCount: t.itemCount || 0,
    lastSignalAt: t.lastSignalAt || null,
    quiet: !t.lastSignalAt || t.lastSignalAt < quietBefore,
  })),
  series: series.map(({ hour: _, weekday: __, startWeek: ___, ...sr }) => sr),
  items,
  events,
};
mkdirSync(out, { recursive: true });
writeFileSync(join(out, 'busy-week.json'), JSON.stringify(data));
writeFileSync(join(out, 'busy-week.js'), `window.BUSY_WEEK = ${JSON.stringify(data)};\n`);
console.log(JSON.stringify(shape, null, 1), items.length, 'items', events.length, 'events');
