// Invented work week for design mocks. Run: node app/scripts/gen-busy-week.mjs
import { writeFileSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const out = join(dirname(fileURLToPath(import.meta.url)), "../../project/fixtures");
let s = 20261019;
const rnd = () => (s = (Math.imul(s, 1664525) + 1013904223) >>> 0) / 2 ** 32;
const int = (a, b) => a + Math.floor(rnd() * (b - a + 1));
const pick = (a) => a[Math.floor(rnd() * a.length)];
const wpick = (pairs) => {
  let r = rnd() * pairs.reduce((t, p) => t + p[1], 0);
  for (const [v, w] of pairs) if ((r -= w) < 0) return v;
  return pairs[0][0];
};
const cap = (t) => t[0].toUpperCase() + t.slice(1);

const FIRST = "Ada Bram Cleo Dara Elio Faye Gus Hana Ivo Jun Kai Lena Milo Nora Omar Pia Quin Rae Sol Tess Uma Vik Wren Xan Yara Zed Alma Beck Cora Dov Edda Finn Gwen Hugo Ines Jory Kit Lux Mara Nico Opal Pax Remy Suri Taj Una Vera Wade Yuki Zora".split(" ");
const LAST = "Aldous Brandt Calloway Dunmore Estrada Fenwick Gallo Hargrove Imura Jessup Kowal Lindqvist Marlow Novak Okafor Pruitt Quarles Rinaldi Sato Thorne Ueda Voss Whitlock Yamada Zielinski Abara Bishop Corvin Delacroix Eckhart Farrow".split(" ");
const NOUN = "pipeline rollout migration dashboard cluster budget contract audit invoice roadmap schema ticket alert quota backup certificate runbook dependency forecast headcount vendor renewal incident postmortem rotation endpoint cache queue deploy release review proposal onboarding policy license benchmark config token webhook bucket index sprint estimate".split(" ");
const VERB = "update check fix review move drop add rename revert retry document approve schedule split merge close reopen escalate pin bump rotate triage".split(" ");
const ADJ = "flaky stale pending blocked urgent noisy slow duplicate missing legacy shared internal quarterly weekly external partial".split(" ");
const CHAN = "eng-platform eng-oncall data-infra security-review finance-ops hiring-loop design-sync cust-escalations vendor-mgmt random leads-private release-train sre-alerts ml-research it-help".split(" ");
const CATS = [["ai", 23], ["alert", 3], ["corporate", 11], ["customers", 20], ["engineering", 52], ["finance", 6], ["fun", 6], ["hiring", 5], ["life", 2], ["management", 26], ["security", 24], ["vendors", 24], ["work", 39]];
const PROJ = ["PLAT", "DATA", "SEC", "CUST", "FIN"];
const REPO = ["acme/api", "acme/infra", "acme/web"];

const sentence = (min = 6, max = 16) => {
  const n = int(min, max);
  const w = [];
  for (let i = 0; i < n; i++) w.push(pick([VERB, ADJ, NOUN, ["the", "a", "this", "that", "our"]][i % 4]));
  return cap(w.join(" ")) + pick([".", ".", "?", "!"]);
};
const text = (len) => {
  let t = "";
  while (t.length < len) t += sentence() + " ";
  return t.trim();
};
const title = (len) =>
  cap(`${pick(VERB)} ${pick(ADJ)} ${pick(NOUN)} ${pick(NOUN)} ${pick(["for", "in", "after", "before"])} ${pick(NOUN)}`).slice(0, Math.max(10, len));

const people = Array.from({ length: 140 }, (_, i) => {
  const name = `${pick(FIRST)} ${pick(LAST)}`;
  const h = name.toLowerCase().replace(" ", ".");
  return { id: i + 1, name, handle: h, email: `${h}@example.com` };
});
const ME = { id: 0, name: "Sam Rivera", handle: "sam.rivera", email: "sam.rivera@example.com" };
const author = () => people[Math.floor(rnd() ** 2 * people.length)];

const topics = Array.from({ length: 60 }, (_, i) => ({
  id: i + 1,
  name: `${cap(pick(ADJ))} ${pick(NOUN)} ${pick(["", "v2", "cleanup", "Q4", "rollout"])}`.trim(),
  category: wpick(CATS),
  itemCount: 0,
}));

const DAYS = ["2026-10-19", "2026-10-20", "2026-10-21", "2026-10-22", "2026-10-23", "2026-10-24", "2026-10-25"];
const DAY_SCALE = [0.85, 1.0, 1.15, 1.2, 0.95, 0.07, 0.04];
const HOURS = [[0, 1], [1, 1], [2, 1], [3, 1], [4, 1], [5, 1], [6, 1], [8, 1], [9, 1], [10, 1], [11, 1], [12, 2], [13, 3], [14, 5], [15, 8], [16, 8], [17, 10], [18, 9], [19, 10], [20, 9], [21, 5], [22, 5], [23, 3]];
const AGENT_HOURS = [[9, 2], [10, 4], [11, 4], [12, 2], [13, 4], [14, 5], [15, 5], [16, 4], [17, 3], [18, 2]];
const CAL_HOURS = [[9, 3], [10, 4], [11, 3], [13, 3], [14, 4], [15, 3], [16, 2]];
const ts = (day, h) => `${day}T${String(h).padStart(2, "0")}:${String(int(0, 59)).padStart(2, "0")}:00`;

// [source, kind, per weekday, hour shape, mean msg_count (0 = none), title len, body len, owned share, due share]
const MIX = [
  ["slack", "message", 64, HOURS, 1, 17, 261, 0, 0],
  ["slack", "thread", 59, HOURS, 3.7, 30, 745, 0, 0],
  ["agents", "approval", 51, AGENT_HOURS, 1, 28, 356, 0, 0],
  ["gmail", "email", 30, HOURS, 1.4, 71, 1221, 0, 0],
  ["jira", "issue", 13, HOURS, 1.9, 72, 983, 0.75, 0.13],
  ["agents", "turn", 11, AGENT_HOURS, 1, 32, 365, 0, 0],
  ["gcal", "event", 10, CAL_HOURS, 0, 21, 139, 0, 0],
  ["agents", "ask", 6, AGENT_HOURS, 1, 29, 423, 0, 0],
  ["obsidian", "note", 2, HOURS, 0, 14, 195, 0, 0],
  ["github", "pr", 0.4, HOURS, 1, 111, 1158, 1, 0],
];

const msgCount = (mean) => {
  if (mean <= 1) return mean;
  if (rnd() > (mean - 1) / 3.5 + 0.1) return 1;
  return Math.min(60, 2 + Math.floor(-Math.log(1 - rnd()) * mean * 1.8));
};

const items = [];
const events = [];
let id = 0;
DAYS.forEach((day, di) => {
  for (const [source, kind, per, hours, mean, tl, bl, owned, due] of MIX) {
    const want = per * DAY_SCALE[di] * (0.85 + rnd() * 0.3);
    const n = Math.floor(want) + (rnd() < want % 1 ? 1 : 0);
    for (let k = 0; k < n; k++) {
      const at = ts(day, wpick(hours));
      const count = msgCount(mean);
      const a = source === "agents" || source === "obsidian" ? null : author();
      const it = {
        id: ++id,
        source,
        kind,
        title: source === "jira" ? `${pick(PROJ)}-${int(100, 9999)}: ${title(tl)}` : title(int(Math.max(8, tl - 10), tl + 10)),
        summary: a ? sentence(8, 18) : null,
        body: text(Math.round(bl * (0.4 + rnd() * 1.2))),
        author: a && { id: a.id, name: a.name },
        occurredAt: at,
        status: "open",
        msgCount: count,
        seen: !(di >= 3 && rnd() < 0.08),
        owner: rnd() < owned ? "you" : null,
        dueDay: rnd() < due ? DAYS[Math.min(4, di + int(1, 4))] : null,
        channel: source === "slack" ? `#${pick(CHAN)}` : undefined,
        repo: source === "github" ? pick(REPO) : undefined,
      };
      if (source !== "gcal" && rnd() < 0.75) {
        const t = topics[Math.floor(rnd() ** 1.6 * topics.length)];
        it.topicId = t.id;
        t.itemCount++;
      }
      if (count > 1) {
        it.messages = Array.from({ length: Math.min(count, 6) }, (_, m) => ({
          at: ts(day, int(+at.slice(11, 13), 23)),
          author: (m === 0 && a ? a : rnd() < 0.2 ? ME : author()).name,
          text: text(int(40, 320)),
        }));
      }
      if (source === "gcal") {
        const len = pick([30, 30, 45, 60, 60, 90]);
        events.push({
          id: events.length + 1,
          itemId: it.id,
          title: it.title,
          start: at,
          end: new Date(Date.parse(at + "Z") + len * 60000).toISOString().slice(0, 19),
          tentative: rnd() < 0.15,
          attendees: Array.from({ length: int(1, 8) }, () => author().name),
          category: wpick(CATS),
        });
        if (di < 3) it.status = "done";
      }
      if ((source === "jira" || source === "github") && rnd() < 0.3) it.status = "done";
      items.push(it);
    }
  }
});
items.sort((x, y) => x.occurredAt.localeCompare(y.occurredAt));

const tally = (f) => items.reduce((m, i) => ((m[f(i)] = (m[f(i)] || 0) + 1), m), {});
const data = {
  about:
    "Invented data at the density of a real account's work week (Mon 2026-10-19 to Sun 2026-10-25). Nothing here comes from a real person or message. Regenerate with node app/scripts/gen-busy-week.mjs. msgCount is the full thread length; messages holds at most the first 6.",
  me: ME,
  days: DAYS,
  countsBySourceKind: tally((i) => `${i.source}/${i.kind}`),
  countsByDay: tally((i) => i.occurredAt.slice(0, 10)),
  people,
  topics: topics.filter((t) => t.itemCount),
  items,
  events,
};
mkdirSync(out, { recursive: true });
writeFileSync(join(out, "busy-week.json"), JSON.stringify(data));
writeFileSync(join(out, "busy-week.js"), `window.BUSY_WEEK = ${JSON.stringify(data)};\n`);
console.log(data.countsBySourceKind, data.countsByDay, items.length, "items", events.length, "events");
