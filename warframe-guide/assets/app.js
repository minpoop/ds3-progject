/* Tenno Field Manual - page logic. Needs art.js, data.js, news.js, portraits.js and companions.js loaded first. */
"use strict";

const CHAPTERS = [
  ["home", "Overview"], ["news", "News"], ["weapons", "Weapons"], ["warframes", "Warframes"], ["modding", "Modding"],
  ["systems", "Systems"], ["materials", "Materials"], ["currencies", "Currencies"], ["platinum", "Platinum"],
  ["steelpath", "Steel Path"], ["roadmap", "Roadmap"], ["tips", "Field Notes"],
];
const LINE_KEY = { roadmap: "progression" };
const ROLES = ["all", "tank", "support", "dps", "melee", "control", "defense", "loot", "stealth", "speed"];
const SYSTEM_ART = { "Archon Shards": "Archon Shard", "Operator, Focus & Amps": "Intact Sentient Core", "Kuva Liches & Sisters": "Kuva", "Railjack & Archwing": "Corrupted Holokey", "Incarnon weapons": "Primary Arcane Adapter" };
// Live in-game news: Digital Extremes' world-state feed, served as JSON (with CORS) by api.warframestat.us.
const NEWS_FEED = "https://api.warframestat.us/pc/news?language=en";
const FEATURED = ["Excalibur", "Revenant", "Rhino", "Saryn", "Wisp", "Kullervo", "Dante", "Nekros", "Mesa", "Frost", "Citrine", "Nezha"];

const $ = (sel, root = document) => root.querySelector(sel);
const $$ = (sel, root = document) => Array.from(root.querySelectorAll(sel));
const esc = s => String(s).replace(/[&<>"']/g, c => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));
const wiki = name => "https://wiki.warframe.com/w/" + encodeURIComponent(name.replace(/ /g, "_"));
const store = {
  get(k, d) { try { const v = localStorage.getItem("tenno_" + k); return v === null ? d : JSON.parse(v); } catch (e) { return d; } },
  set(k, v) { try { localStorage.setItem("tenno_" + k, JSON.stringify(v)); } catch (e) { /* storage unavailable */ } },
};
const motionOK = !window.matchMedia("(prefers-reduced-motion: reduce)").matches;

function art(name, cls = "") {
  const src = typeof ART !== "undefined" && ART[name];
  const glyph = { credits: "¢", platinum: "◆" }[name] || esc(name.charAt(0));
  return `<span class="art ${cls}">${src ? `<img src="${src}" alt="" loading="lazy" decoding="async">` : `<span class="glyph">${glyph}</span>`}</span>`;
}
const head = (caseNo, title, lede) =>
  `<header class="head"><div class="case">${caseNo}</div><h2 class="title">${title}</h2>${lede ? `<p class="lede">${lede}</p>` : ""}</header>`;
const caseNo = id => `Case file ${String(CHAPTERS.findIndex(c => c[0] === id)).padStart(2, "0")} · ${CHAPTERS.find(c => c[0] === id)[1]}`;

/* ------------------------------------------------------------ views */

function viewHome() {
  const day = Math.floor(Date.now() / 864e5);
  const fname = FEATURED[day % FEATURED.length];
  const f = FRAMES.find(x => x[0] === fname);
  const weaponPicks = MR_PICKS.length * 6;
  const hero = typeof KEYART !== "undefined" ? KEYART.hero : "";
  return `
  <section class="hero-art">
    ${hero ? `<img class="hero-img" src="${hero}" alt="Warframe key art: Albrecht Entrati and a Warframe beneath a giant statue with glowing eyes">` : ""}
    <div class="hero-text">
      <div class="case">Unofficial fan guide · Warframe, October 2026</div>
      <h1 class="title burn">Build stronger.<br><em>Farm smarter.</em></h1>
      <p class="lede">A field manual from your first Star Chart mission to Steel Path: the best weapons at every Mastery Rank, every Warframe and what it's good at, mods, farming, Platinum and endgame. Six survivors from other nightmares ride along with tips.</p>
      <div class="hero-actions"><button class="btn primary" type="button" data-go="weapons">Best weapons for my MR</button><button class="btn" type="button" data-go="news">Latest news</button></div>
      <div class="facts"><span><b>${weaponPicks}</b>weapon picks</span><span><b>${FRAMES.length}</b>Warframes</span><span><b>${MATERIALS.length}</b>farm routes</span><span><b>${SP_STEPS.length}</b>Steel Path steps</span></div>
    </div>
    ${hero ? `<span class="credit">Key art: Whispers in the Walls · Digital Extremes</span>` : ""}
  </section>

  <section class="news-mag">
    <div class="mag-head"><div><div class="case">Latest from Digital Extremes</div><h3 class="sub">Warframe news</h3></div>
      <button class="btn" type="button" data-go="news">All news & patch notes</button></div>
    <div class="mag" id="home-news"></div>
  </section>

  <section class="lineup-wrap">
    <div class="head"><div class="case">The survivors</div><h3 class="sub">Pick who talks to you</h3>
      <p class="muted small">Six survivors from other nightmares. Tap one to put them on the radio in the corner; each has tips for every chapter.</p></div>
    <div class="lineup">
      <div class="ruler" aria-hidden="true">${["6'6", "6'0", "5'6", "5'0", "4'6", "4'0"].map((h, i) => `<span style="top:${(i + 1) * 48 - 7}px">${h}</span>`).join("")}</div>
      <div class="cast">${COMPANIONS.map((c, i) => `
        <button class="cast-card" type="button" data-cast="${c.id}" aria-pressed="false">${portrait(c.id, "cast")}
          <span class="placard"><b>${esc(c.name)}</b><span>${esc(c.game)} · No. ${String(i + 1).padStart(3, "0")}</span></span></button>`).join("")}</div>
    </div>
  </section>

  <div class="home-pair">
    <article class="panel featured">
      <div class="case">Frame of the day</div>
      ${art(f[0], "big")}
      <h3 class="sub">${esc(f[0])}</h3>
      <div class="pills"><span class="chip lamp">${esc(f[1])}</span><span class="chip">${esc(f[2])}</span></div>
      <p class="muted small">${esc(f[3])}</p>
      <button class="btn" type="button" data-go="warframes" data-q="${esc(f[0])}">Open in Warframes</button>
    </article>
    <section class="panel checklist">
      <div class="check-head"><div><div class="case">Progress tracker</div><h3 class="sub">Your power checklist</h3></div><span class="memo muted" id="task-count"></span></div>
      <div class="meter" role="progressbar" aria-label="Checklist progress" aria-valuemin="0" aria-valuemax="${CHECKLIST.length}"><i id="task-fill"></i></div>
      <div class="tasks">${CHECKLIST.map(([k, t]) => `<label class="task"><input type="checkbox" id="task-${k}" data-task="${k}"><span>${esc(t)}</span></label>`).join("")}</div>
      <p class="status">Saved in this browser only.</p>
    </section>
  </div>

  <section class="head"><div class="case">Where to go</div><h3 class="sub">Jump to a chapter</h3></section>
  <div class="quick">
    ${[["weapons", "Arsenal", "Best weapons", "Two picks per slot for every MR 0-30, with why and how to get them.", "Phenmor"],
       ["warframes", "Roster", "Every Warframe", `All ${FRAMES.length} frames: what each is good at, where to farm it.`, "Revenant"],
       ["modding", "Power", "Modding school", "Build skeletons, priority mods, damage types.", "Serration"],
       ["materials", "Resources", "Find materials", "Best mission for each resource, plus alternatives.", "Orokin Cell"],
       ["platinum", "Economy", "Earn Platinum", "Safe trading and what actually sells.", "Orokin Ducats"],
       ["steelpath", "Endgame", "Enter Steel Path", "From unlock to Arcanes and Incarnons.", "Steel Essence"]]
      .map(([id, c, t, d, img]) => `<button class="panel" type="button" data-go="${id}">${art(img)}<span class="q-text"><span class="case">${c}</span><b>${t}</b><span>${d}</span></span></button>`).join("")}
  </div>`;
}

function viewWeapons() {
  return `${head(caseNo("weapons"), "Weapons by Mastery Rank", "Every MR from 0 to 30 with two primaries, two secondaries and two melee picks. Each card says why it's good and how to get it.")}
  <div class="note"><b>MR 17-30 are endgame spotlights,</b> not new unlocks: almost every weapon is open by MR 16. Quests, syndicates and Incarnon adapters often matter more than the MR number.</div>
  <div class="panel card">
    <div class="toolbar">
      <input class="search" id="w-search" type="search" placeholder="Search weapons, strengths, how to get…" aria-label="Search weapons">
      <select class="select" id="w-rank" aria-label="Mastery Rank">${MR_PICKS.map((_, i) => `<option value="${i}">MR ${i}</option>`).join("")}</select>
      <button class="btn" type="button" id="w-all" aria-pressed="false">Show all ranks</button>
    </div>
    <div class="mr-strip" id="w-strip" role="group" aria-label="Choose Mastery Rank">${MR_PICKS.map((_, i) => `<button class="mr${i > 16 ? " endgame" : ""}" type="button" data-mr="${i}" aria-pressed="false">${i}</button>`).join("")}</div>
    <p class="status" id="w-status"></p>
  </div>
  <div id="w-list"></div>`;
}

function weaponCard(name, best) {
  const [slot, mr, why, how] = WEAPONS[name];
  return `<article class="panel card" data-text="${esc((name + " " + why + " " + how).toLowerCase())}">
    <div class="card-top">${art(name)}<div><h3>${esc(name)}</h3><span class="pills"><span class="chip ${best ? "lamp" : ""}">${best ? "Best" : "Alternative"}</span><span class="wmr">Needs MR ${mr}</span></span></div></div>
    <div class="why"><b>WHY IT'S GOOD</b>${esc(why)}</div>
    <details><summary>How to get it</summary><p class="small muted">${esc(how)}</p><p class="small"><a href="${wiki(name)}" target="_blank" rel="noopener">Wiki: drops, costs and crafting</a></p></details>
  </article>`;
}

function renderWeapons() {
  const q = $("#w-search").value.trim().toLowerCase();
  const sel = state.mr, all = state.allRanks;
  $("#w-rank").value = String(sel);
  $("#w-all").setAttribute("aria-pressed", String(all));
  $("#w-all").classList.toggle("on", all);
  $$("#w-strip .mr").forEach(b => b.setAttribute("aria-pressed", String(!all && !q && +b.dataset.mr === sel)));
  let html = "", cards = 0, ranks = 0;
  MR_PICKS.forEach((slots, rank) => {
    if (!q && !all && rank !== sel) return;
    let body = "", n = 0;
    slots.forEach((pair, s) => {
      const hits = pair.filter(w => !q || (w + " " + WEAPONS[w][2] + " " + WEAPONS[w][3]).toLowerCase().includes(q));
      if (!hits.length) return;
      n += hits.length;
      body += `<div class="slot"><div class="case">${["Primary", "Secondary", "Melee"][s]}</div>${hits.map(w => weaponCard(w, pair.indexOf(w) === 0)).join("")}</div>`;
    });
    if (!n) return;
    cards += n; ranks++;
    html += `<section class="rank"><div class="rank-head"><h3 class="title">MR ${rank}</h3>${rank > 16 ? `<span class="chip blood">Endgame pick</span>` : ""}</div><div class="slots">${body}</div></section>`;
  });
  $("#w-list").innerHTML = html || `<div class="panel empty">No weapon matches “${esc(q)}”. Try a weapon name or a word like “Incarnon”.</div>`;
  $("#w-status").textContent = q ? `${cards} cards match across ${ranks} ranks. Clear the search to go back to MR ${sel}.`
    : all ? `Showing all ${MR_PICKS.length} ranks (${cards} picks).` : `MR ${sel}: ${cards} picks. Open “How to get it” on any card.`;
}

function viewWarframes() {
  return `${head(caseNo("warframes"), "Every Warframe, and what it's good at", `All ${FRAMES.length} base Warframes, meta or not. Prime versions play the same and come from Void Relics, Prime Resurgence or trading.`)}
  <div class="panel card">
    <div class="toolbar"><input class="search" id="f-search" type="search" placeholder="Search a frame, a role, a planet…" aria-label="Search Warframes"></div>
    <div class="pills" id="f-roles" role="group" aria-label="Filter by role">${ROLES.map(r => `<button class="pill" type="button" data-role="${r}" aria-pressed="${r === "all"}">${r === "all" ? "All roles" : r === "dps" ? "DPS" : r[0].toUpperCase() + r.slice(1)}</button>`).join("")}</div>
    <p class="status" id="f-status"></p>
  </div>
  <div class="grid" id="f-grid">${FRAMES.map(([n, stage, role, good, weak, sp, how, build, tags]) => `
    <article class="panel card" data-tags="${tags}" data-text="${esc((n + " " + stage + " " + role + " " + good + " " + how + " " + tags).toLowerCase())}">
      <div class="card-top">${art(n)}<div><h3>${esc(n)}</h3><span class="pills"><span class="chip lamp">${esc(stage)}</span><span class="chip">${esc(role)}</span></span></div></div>
      <p><b>Good at:</b> ${esc(good)}</p>
      <p><b>Trade-off:</b> ${esc(weak)}</p>
      <p><b>Steel Path:</b> ${esc(sp)}</p>
      <details><summary>Where to farm + build tips</summary><p class="small muted"><b>Get it:</b> ${esc(how)}</p><p class="small muted"><b>Build:</b> ${esc(build)}</p><p class="small"><a href="${wiki(n)}" target="_blank" rel="noopener">Wiki: drops and blueprints</a></p></details>
    </article>`).join("")}</div>`;
}

function filterFrames() {
  const q = $("#f-search").value.trim().toLowerCase();
  let n = 0;
  $$("#f-grid > article").forEach(a => {
    const hit = (state.role === "all" || a.dataset.tags.split(" ").includes(state.role)) && (!q || a.dataset.text.includes(q));
    a.hidden = !hit; if (hit) n++;
  });
  $$("#f-roles .pill").forEach(p => p.setAttribute("aria-pressed", String(p.dataset.role === state.role)));
  $("#f-status").textContent = `${n} of ${FRAMES.length} Warframes shown.`;
}

function searchGrid(id, placeholder, cards) {
  return `<div class="toolbar"><input class="search" type="search" data-filter="${id}" placeholder="${placeholder}" aria-label="${placeholder}"></div>
  <div class="grid" id="${id}">${cards}</div><div class="panel empty" id="${id}-none" hidden>Nothing matches that search.</div>`;
}

function viewModding() {
  return `${head(caseNo("modding"), "Modding & build academy", "The biggest power spikes come from good mod combinations, not a higher-MR gun. Learn what to slot, where to get it, and when a Catalyst or Forma is worth it.")}
  <div class="steps">
    ${[["Make room", "Rank gear to 30, then install an Orokin Catalyst (weapons) or Reactor (Warframes) on gear you keep. Both double capacity. Forma changes slot polarities."],
       ["Multiply damage", "Mix damage + multishot + crit/status + elements instead of stacking one type. Read the weapon's base crit and status stats first."],
       ["Scale into endgame", "Add Corrupted Warframe mods, Galvanized weapon mods, weapon Arcanes and augments as you unlock their activities."]]
      .map(([t, d]) => `<div class="panel step"><div><h3>${t}</h3><p>${d}</p></div></div>`).join("")}
  </div>
  <section class="head"><div class="case">Priority mods</div><h3 class="sub">What to farm, in order</h3></section>
  ${searchGrid("mod-grid", "Search mods, Deimos, Arbitration…", MODS.map(([n, stage, why, how, img]) => `
    <article class="panel card" data-text="${esc((n + " " + stage + " " + why + " " + how).toLowerCase())}">
      <div class="card-top">${img ? art(img) : art(n)}<div><h3>${esc(n)}</h3><span class="chip ${stage === "Starter" ? "" : stage === "Midgame" ? "lamp" : "blood"}">${esc(stage)}</span></div></div>
      <p>${esc(why)}</p><details><summary>How to get it</summary><p class="small muted">${esc(how)}</p></details>
    </article>`).join(""))}
  <section class="head"><div class="case">Templates</div><h3 class="sub">Build skeletons you can copy</h3><p class="muted small">Starting points, not mandatory eight-slot builds. Swap mods to suit the weapon and the enemy.</p></section>
  <div class="grid two">${BUILDS.map(([tag, t, base, list, why]) => `
    <article class="panel card"><span class="chip lamp">${esc(tag)}</span><h3>${esc(t)}</h3><p class="memo small">${esc(base)}</p>
      <ol class="build">${list.map(l => `<li>${esc(l)}</li>`).join("")}</ol><p><b>Why:</b> ${esc(why)}</p></article>`).join("")}</div>
  <section class="head"><div class="case">Damage types</div><h3 class="sub">Elements in plain English</h3></section>
  <div class="panel table-wrap"><table><thead><tr><th>Element</th><th>Made from</th><th>What it does</th></tr></thead><tbody>
    ${ELEMENTS.map(([e, m, d]) => `<tr><td><b>${e}</b></td><td class="memo">${m}</td><td>${d}</td></tr>`).join("")}</tbody></table></div>
  <section class="head"><div class="case">Mistakes</div><h3 class="sub">Common build traps</h3></section>
  <div class="grid two">${MOD_TRAPS.map(([t, d]) => `<article class="panel card"><h3>${t}</h3><p>${d}</p></article>`).join("")}</div>`;
}

function viewSystems() {
  return `${head(caseNo("systems"), "Systems the Star Chart won't explain", "Helminth, Archon Shards, Operator and Focus, companions, nemeses, Railjack, Necramechs and Incarnons: what each does and when to start.")}
  <div class="grid two">${SYSTEMS.map(([t, unlock, what, tip]) => `
    <article class="panel card"><div class="card-top">${art(SYSTEM_ART[t] || t)}<h3>${t}</h3></div>
      <p><b>Unlock:</b> ${esc(unlock)}</p><p><b>What it does:</b> ${esc(what)}</p><p class="memo small" style="color:var(--lamp)">${esc(tip)}</p></article>`).join("")}</div>`;
}

function farmCard([n, where, how, alt, gate, cat, img]) {
  return `<article class="panel card" data-text="${esc([n, where, how, alt, gate, cat].join(" ").toLowerCase())}">
    <div class="card-top">${art(img || n)}<div><h3>${esc(n)}</h3><span class="chip">${esc(cat || gate)}</span></div></div>
    <p><b>Best place:</b> ${esc(where)}</p><p><b>How:</b> ${esc(how)}</p>
    <details><summary>Alternatives & unlock</summary><p class="small muted"><b>Alternative:</b> ${esc(alt)}</p><p class="small muted"><b>Access:</b> ${esc(gate)}</p></details></article>`;
}

function viewMaterials() {
  return `${head(caseNo("materials"), "Material farming", "A reliable mission for each resource, how to farm it, and where else it drops. “Best” means dependable, not a guaranteed top yield.")}
  ${searchGrid("mat-grid", "Search a resource, planet or mission…", MATERIALS.map(m => farmCard(m)).join(""))}
  <section class="head"><div class="case">Before you grind</div><h3 class="sub">Farming tips</h3></section>
  <div class="grid two">${FARM_TIPS.map(([t, a, b]) => `<article class="panel card"><h3>${t}</h3><p>${a}</p><p>${b}</p></article>`).join("")}</div>`;
}

function viewCurrencies() {
  return `${head(caseNo("currencies"), "Currencies & standing", "Where to earn each currency and faction standing, including trading, rotating rewards and quest-locked activities.")}
  ${searchGrid("cur-grid", "Search a currency, faction or mission…", CURRENCIES.map(([n, where, how, alt, gate, img]) => farmCard([n, where, how, alt, gate, gate.split(" ")[0] === "Early" ? "Early" : "", img])).join(""))}`;
}

function viewPlatinum() {
  return `${head(caseNo("platinum"), "Platinum farming & trading", "Platinum comes from trading with other players. Prices, demand and vaulted Primes change, so always check live listings.")}
  <div class="note"><b>Before your first trade:</b> ${esc(BEFORE_TRADE)} <a href="https://support.warframe.com/hc/en-us/articles/37999066923533-Trading-FAQ-Safe-Trading-Tips" target="_blank" rel="noopener">Official safe-trading rules</a></div>
  <section class="head"><div class="case">Methods</div><h3 class="sub">Repeatable ways to earn Platinum</h3></section>
  ${searchGrid("plat-grid", "Search relics, corrupted, arcane, riven…", PLAT_METHODS.map(([t, farm, sell, req, warn], i) => `
    <article class="panel card" data-text="${esc([t, farm, sell, req, warn].join(" ").toLowerCase())}">
      <div class="card-top">${art(["Aya", "Blind Rage", "Intensify", "Aya", "Vitus Essence", "Arcane Energize", "Riven Sliver", "Hunter Munitions"][i])}<h3>${esc(t)}</h3></div>
      <p><b>Farm:</b> ${esc(farm)}</p><p><b>Sell:</b> ${esc(sell)}</p>
      <details><summary>Requirements & warnings</summary><p class="small muted"><b>Needs:</b> ${esc(req)}</p><p class="small muted"><b>Watch out:</b> ${esc(warn)}</p></details></article>`).join(""))}
  <section class="head"><div class="case">Step by step</div><h3 class="sub">Making a sale</h3></section>
  <div class="steps">${PLAT_STEPS.map(s => `<div class="panel step"><div><p style="color:var(--bone)">${esc(s)}</p></div></div>`).join("")}</div>
  <div class="grid two">
    <article class="panel card"><h3>What to spend Platinum on</h3><p><b>First:</b> Warframe slots, weapon slots and the capacity you need. Catalysts and Reactors only for keepers you can't farm one for.</p><p><b>Avoid early:</b> rushing Foundry timers, buying easily farmed base frames, and random expensive Rivens. Cosmetics can wait.</p></article>
    <article class="panel card"><h3>Quick decision rule</h3><p>Farm what you can clear reliably and fast. A rare drop worth 100p after many hours can be worse than repeatable 10-20p items. Prices here are examples, not quotes.</p></article>
  </div>`;
}

function viewSteelPath() {
  return `${head(caseNo("steelpath"), "Steel Path: unlock to endgame", `An ${SP_STEPS.length}-step route through the jump to tougher enemies, the upgrade systems that come with it, and how to clear each objective.`)}
  <div class="steps">${SP_STEPS.map(([t, d, tip]) => `<div class="panel step"><div><h3>${t}</h3><p>${d}</p><p class="tip">${tip}</p></div></div>`).join("")}</div>
  <div class="grid two">
    <article class="panel card"><h3>Starter loadouts</h3><div class="table-wrap"><table><tbody>${SP_STARTS.map(([a, b]) => `<tr><td class="memo">${a}</td><td><b>${b}</b></td></tr>`).join("")}</tbody></table></div></article>
    <article class="panel card"><h3>Why does my damage feel low?</h3><p>Check for a Catalyst and enough capacity, ranked multishot and damage mods, crit/status that suits the weapon, elements that suit the enemy, and a way to deal with armour or shields. Incarnon evolutions and Arcanes come after the basics.</p>
      <p class="small"><a href="${wiki("The Steel Path")}" target="_blank" rel="noopener">Steel Path rules</a> · <a href="${wiki("Arbitrations")}" target="_blank" rel="noopener">Arbitrations</a> · <a href="${wiki("Incarnon")}" target="_blank" rel="noopener">Incarnon</a></p></article>
  </div>`;
}

function viewRoadmap() {
  return `${head(caseNo("roadmap"), "Whole-game roadmap", "A practical order of operations. Use the Weapons chapter to pick something strong for your current Mastery Rank.")}
  <div class="steps">${ROADMAP.map(([t, d, tip]) => `<div class="panel step"><div><h3>${t}</h3><p>${d}</p><p class="tip">${tip}</p></div></div>`).join("")}</div>`;
}

function viewNews() {
  return `${head(caseNo("news"), "Warframe news", "Official news from Digital Extremes: the in-game news feed and the PC update notes, newest first. Every item links to the official post.")}
  <div class="panel card news-bar"><p class="status" id="news-status"></p><button class="btn" type="button" id="news-refresh">Check for new posts</button></div>
  <section class="head"><div class="case">In-game news</div><h3 class="sub">Events, streams and announcements</h3></section>
  <div class="grid news-grid" id="news-items"></div>
  <section class="head"><div class="case">Official forums</div><h3 class="sub">PC update notes</h3><p class="muted small">Updates and hotfixes from the official Warframe forums.</p></section>
  <div class="notes" id="news-notes"></div>`;
}

function when(iso) {
  const d = new Date(iso), days = Math.floor((Date.now() - d) / 864e5);
  const ago = days <= 0 ? "today" : days === 1 ? "yesterday" : days < 30 ? `${days} days ago` : "";
  return d.toLocaleDateString(undefined, { day: "numeric", month: "short", year: "numeric" }) + (ago ? ` · ${ago}` : "");
}

function newsCard(n, cls = "") {
  const tag = { Update: "blood", "Prime Access": "lamp", Stream: "" }[n.kind] ?? "";
  return `<a class="panel news-card ${cls}" href="${esc(n.link)}" target="_blank" rel="noopener">
    ${n.img ? `<span class="news-img"><img src="${esc(n.img)}" alt="" loading="lazy" decoding="async"></span>` : `<span class="news-img none"><span>${esc(n.kind)}</span></span>`}
    <span class="news-body"><span class="pills"><span class="chip ${tag}">${esc(n.kind)}</span><span class="wmr">${when(n.date)}</span></span><b>${esc(n.title)}</b></span></a>`;
}

function renderNews() {
  const items = state.news.items;
  $("#news-items").innerHTML = items.length ? items.map(n => newsCard(n)).join("") : `<div class="panel empty">No news posts right now.</div>`;
  // The home page leads with the newest post that has art, then the next two newest.
  const lead = items.find(n => n.img) || items[0];
  $("#home-news").innerHTML = lead ? [newsCard(lead, "lead"), ...items.filter(n => n !== lead).slice(0, 2).map(n => newsCard(n, "mini"))].join("") : "";
  $("#news-notes").innerHTML = NEWS.notes.map(n => `<a class="panel note-row" href="${esc(n.link)}" target="_blank" rel="noopener">
    <span class="wmr">${when(n.date)}</span><b>${esc(n.title)}</b><span class="muted small">${esc(n.summary)}</span></a>`).join("");
  const snap = new Date(NEWS.built).toLocaleString(undefined, { day: "numeric", month: "short", year: "numeric", hour: "numeric", minute: "2-digit" });
  $("#news-status").textContent = state.news.live
    ? `Live: in-game news checked just now. Patch notes as of ${snap}.`
    : state.news.checking ? "Checking for new posts…" : `Showing the snapshot from ${snap}. Live news loads when this page can reach the internet.`;
}

async function refreshNews() {
  state.news.checking = true; renderNews();
  try {
    const ctrl = new AbortController(), timer = setTimeout(() => ctrl.abort(), 8000);
    const res = await fetch(NEWS_FEED, { signal: ctrl.signal });
    clearTimeout(timer);
    if (!res.ok) throw new Error("HTTP " + res.status);
    const feed = await res.json();
    const items = feed.filter(n => n.date && !n.date.startsWith("1970") && !n.mobileOnly)
      .sort((a, b) => b.date.localeCompare(a.date))
      .map(n => ({ date: n.date, title: n.message.trim(), link: n.link,
        kind: n.update ? "Update" : n.primeAccess ? "Prime Access" : n.stream ? "Stream" : "News",
        img: n.imageLink && !n.imageLink.includes("news-placeholder") ? n.imageLink : null }));
    if (items.length) state.news = { items, live: true };
  } catch (e) { /* offline or blocked: keep the snapshot */ }
  state.news.checking = false;
  renderNews();
}

function viewTips() {
  return `${head(caseNo("tips"), "Field notes", "How to use this manual, where to double-check things, and the fine print.")}
  <div class="grid two">
    <article class="panel card"><h3>How to use this guide</h3><p>Start with the checklist on the Overview, open your current MR in Weapons, and visit Modding as soon as enemies start feeling tanky. Cards expand with a tap for acquisition details.</p></article>
    <article class="panel card"><h3>Check before you farm</h3><p>Drop tables, vendors, vaulted Primes and prices change with updates. Cross-check targeted farms here:</p>
      <p class="small"><a href="https://www.warframe.com/droptables" target="_blank" rel="noopener">Official drop tables</a> · <a href="https://wiki.warframe.com/w/Resources" target="_blank" rel="noopener">Wiki: resources</a> · <a href="https://warframe.market/" target="_blank" rel="noopener">warframe.market</a></p></article>
    <article class="panel card"><h3>The companions</h3><p>James, Simon, Dante, Vergil, Leon and Torque give original, fan-written tips. Use the radio's tone button to switch between funny, serious or mixed lines, and turn off scares in the sidebar if you'd rather not be startled.</p></article>
    <article class="panel card"><h3>Fine print</h3><p>Unofficial fan guide, not affiliated with Digital Extremes or the companions' publishers. Item art comes from the community warframe-items image set. Background and hero art are official Digital Extremes key art (Albrecht's Laboratories; Whispers in the Walls) from the official wiki, colour-graded for this page. Companion portraits are official character art from each game, via its fan wiki.</p></article>
  </div>`;
}

const VIEWS = { home: viewHome, news: viewNews, weapons: viewWeapons, warframes: viewWarframes, modding: viewModding, systems: viewSystems, materials: viewMaterials, currencies: viewCurrencies, platinum: viewPlatinum, steelpath: viewSteelPath, roadmap: viewRoadmap, tips: viewTips };

/* ------------------------------------------------------------ state & navigation */

const state = {
  view: "home",
  mr: store.get("mr", 0),
  allRanks: false,
  role: "all",
  tone: store.get("tone", "mixed"),
  who: store.get("who", 0),
  open: store.get("radioOpen", false),
  news: { items: NEWS.items, live: false },
  scares: store.get("scares", true),
  ambience: false,
  cursor: {},
};

function go(id, focus) {
  if (!VIEWS[id]) id = "home";
  state.view = id;
  $$(".view").forEach(v => { v.hidden = v.id !== "v-" + id; });
  $$("[data-chapter]").forEach(b => { if (b.dataset.chapter === id) b.setAttribute("aria-current", "page"); else b.removeAttribute("aria-current"); });
  const strip = $(`.strip [data-chapter="${id}"]`);
  if (strip) strip.scrollIntoView({ block: "nearest", inline: "center" });
  if (location.hash.slice(1) !== id) history.replaceState(null, "", "#" + id);
  if (focus !== false) window.scrollTo({ top: 0 });
  document.body.dataset.view = id;
  speak(state.who, false);
}

function buildViews() {
  const page = $("#page");
  page.innerHTML = CHAPTERS.map(([id]) => `<section class="view" id="v-${id}" hidden>${VIEWS[id]()}</section>`).join("");
  const nav = CHAPTERS.map(([id, label], i) => `<button class="chapter" type="button" data-chapter="${id}"><span class="no">${String(i).padStart(2, "0")}</span><span>${label}</span></button>`).join("");
  $("#chapters").innerHTML = nav;
  $("#strip").innerHTML = nav;
}

function wire() {
  document.addEventListener("click", e => {
    const t = e.target.closest("button, [data-go]");
    if (!t) return;
    if (t.dataset.chapter) go(t.dataset.chapter);
    else if (t.dataset.go) {
      go(t.dataset.go);
      if (t.dataset.q && t.dataset.go === "warframes") { $("#f-search").value = t.dataset.q; filterFrames(); }
    } else if (t.dataset.mr) { state.mr = +t.dataset.mr; state.allRanks = false; store.set("mr", state.mr); $("#w-search").value = ""; renderWeapons(); }
    else if (t.dataset.role) { state.role = t.dataset.role; filterFrames(); }
    else if (t.dataset.cast) { speak(COMPANIONS.findIndex(c => c.id === t.dataset.cast), true); setOpen(true); }
    else if (t.dataset.dot) speak(+t.dataset.dot, true);
  });
  $("#w-rank").addEventListener("change", e => { state.mr = +e.target.value; state.allRanks = false; store.set("mr", state.mr); renderWeapons(); });
  $("#w-all").addEventListener("click", () => { state.allRanks = !state.allRanks; renderWeapons(); });
  $("#w-search").addEventListener("input", renderWeapons);
  $("#f-search").addEventListener("input", filterFrames);
  $$("[data-filter]").forEach(inp => inp.addEventListener("input", () => {
    const q = inp.value.trim().toLowerCase(), grid = $("#" + inp.dataset.filter);
    let n = 0;
    $$(":scope > article", grid).forEach(a => { a.hidden = q && !a.dataset.text.includes(q); if (!a.hidden) n++; });
    $("#" + inp.dataset.filter + "-none").hidden = n > 0;
  }));
  // checklist
  const done = new Set(store.get("tasks", []));
  const boxes = $$("[data-task]");
  const update = () => {
    const n = boxes.filter(b => b.checked).length;
    $("#task-count").textContent = `${n} / ${boxes.length}`;
    $("#task-fill").style.width = (n / boxes.length * 100) + "%";
    $(".meter").setAttribute("aria-valuenow", String(n));
    boxes.forEach(b => b.closest(".task").classList.toggle("done", b.checked));
  };
  boxes.forEach(b => {
    b.checked = done.has(b.dataset.task);
    b.addEventListener("change", () => { store.set("tasks", boxes.filter(x => x.checked).map(x => x.dataset.task)); update(); });
  });
  update();
  // atmosphere toggles
  $$("[data-toggle]").forEach(b => b.addEventListener("click", () => toggle(b.dataset.toggle)));
  syncToggles();
  window.addEventListener("hashchange", () => go(location.hash.slice(1)));
}

function toggle(what) {
  if (what === "ambience") { state.ambience = !state.ambience; state.ambience ? Sound.start() : Sound.stop(); }
  if (what === "scares") { state.scares = !state.scares; store.set("scares", state.scares); }
  if (what === "test") { scare(COMPANIONS[state.who]); return; }
  syncToggles();
}
function syncToggles() {
  $$('[data-toggle="ambience"]').forEach(b => { b.setAttribute("aria-pressed", String(state.ambience)); $("b", b).textContent = state.ambience ? "ON" : "OFF"; });
  $$('[data-toggle="scares"]').forEach(b => { b.setAttribute("aria-pressed", String(state.scares)); $("b", b).textContent = state.scares ? "ON" : "OFF"; });
  document.body.dataset.flicker = state.scares ? "on" : "off";
}

/* ------------------------------------------------------------ companion radio */

function linePool(who) {
  const sec = LINE_KEY[state.view] || state.view;
  const all = (LINES[sec] || LINES.home)[COMPANIONS[who].id] || LINES.home[COMPANIONS[who].id];
  const toned = state.tone === "mixed" ? all : all.filter(l => l[2] === (state.tone === "funny" ? "f" : "s"));
  return toned.length ? toned : all;
}

function speak(who, advance) {
  state.who = who;
  store.set("who", who);
  const c = COMPANIONS[who];
  const pool = linePool(who);
  const key = (LINE_KEY[state.view] || state.view) + ":" + c.id + ":" + state.tone;
  let i = state.cursor[key];
  i = i === undefined ? 0 : advance ? (i + 1) % pool.length : i;
  state.cursor[key] = i;
  const [tag, text] = pool[i];
  const r = $("#radio");
  r.style.setProperty("--accent", c.accent);
  $("#r-face").innerHTML = portrait(c.id, "radio");
  $("#r-name").textContent = c.name;
  $("#r-game").textContent = c.game;
  $("#r-tag").textContent = tag;
  $("#r-line").textContent = text;
  $("#r-tone").textContent = "Tone: " + state.tone[0].toUpperCase() + state.tone.slice(1);
  $$("#r-dots .dot").forEach((d, n) => d.setAttribute("aria-pressed", String(n === who)));
  $$("[data-cast]").forEach(b => b.setAttribute("aria-pressed", String(b.dataset.cast === c.id)));
  if (advance && motionOK) { r.classList.remove("talk"); void r.offsetWidth; r.classList.add("talk"); }
}

function nextHint() {
  const who = (state.who + 1) % COMPANIONS.length;
  speak(who, true);
  if (state.scares && Math.random() < 1 / 18) setTimeout(() => scare(COMPANIONS[who]), 450);
}

function setOpen(open) {
  state.open = open;
  store.set("radioOpen", open);
  $("#radio").classList.toggle("closed", !open);
  $("#r-face").setAttribute("aria-label", open ? "" : "Open companion tips");
  $("#r-face").tabIndex = open ? -1 : 0;
}

function scare(c) {
  const el = $("#scare");
  $("p", el).textContent = c.scare;
  el.classList.remove("go"); void el.offsetWidth; el.classList.add("go");
  if (state.ambience) Sound.sting();
}

function wireRadio() {
  $("#r-dots").innerHTML = COMPANIONS.map((c, i) => `<button class="dot" type="button" data-dot="${i}" aria-label="${esc(c.name)}" title="${esc(c.name)}" aria-pressed="false"></button>`).join("");
  $("#r-next").addEventListener("click", nextHint);
  $("#r-tone").addEventListener("click", () => { state.tone = { mixed: "funny", funny: "serious", serious: "mixed" }[state.tone]; store.set("tone", state.tone); speak(state.who, false); });
  $("#r-hide").addEventListener("click", () => setOpen(false));
  $("#r-face").addEventListener("click", () => { if (!state.open) setOpen(true); });
  $("#r-face").addEventListener("keydown", e => { if (!state.open && (e.key === "Enter" || e.key === " ")) { e.preventDefault(); setOpen(true); } });
  setOpen(state.open);
}

/* ------------------------------------------------------------ ambience (generated with Web Audio, no files) */

const Sound = (() => {
  let ctx, master, timer;
  function noiseBuffer(c) {
    const b = c.createBuffer(1, c.sampleRate * 3, c.sampleRate), d = b.getChannelData(0);
    let last = 0;
    for (let i = 0; i < d.length; i++) { last = (last + 0.02 * (Math.random() * 2 - 1)) / 1.02; d[i] = last * 3.5; }
    return b;
  }
  function clank() {
    if (!ctx) return;
    const t = ctx.currentTime, src = ctx.createBufferSource(), bp = ctx.createBiquadFilter(), g = ctx.createGain();
    src.buffer = noiseBuffer(ctx); bp.type = "bandpass"; bp.frequency.value = 900 + Math.random() * 1600; bp.Q.value = 18;
    g.gain.setValueAtTime(0.0001, t); g.gain.exponentialRampToValueAtTime(0.5, t + 0.01); g.gain.exponentialRampToValueAtTime(0.0001, t + 1.6);
    src.connect(bp).connect(g).connect(master); src.start(t); src.stop(t + 1.7);
    timer = setTimeout(clank, 7000 + Math.random() * 14000);
  }
  return {
    start() {
      ctx = ctx || new (window.AudioContext || window.webkitAudioContext)();
      ctx.resume();
      master = ctx.createGain(); master.gain.value = 0; master.connect(ctx.destination);
      master.gain.linearRampToValueAtTime(0.5, ctx.currentTime + 2);
      const wind = ctx.createBufferSource(), lp = ctx.createBiquadFilter(), wg = ctx.createGain(), lfo = ctx.createOscillator(), lfoG = ctx.createGain();
      wind.buffer = noiseBuffer(ctx); wind.loop = true; lp.type = "lowpass"; lp.frequency.value = 380; wg.gain.value = 0.22;
      lfo.frequency.value = 0.07; lfoG.gain.value = 220; lfo.connect(lfoG).connect(lp.frequency);
      wind.connect(lp).connect(wg).connect(master); wind.start(); lfo.start();
      [[55, "sine", 0.06], [82.6, "triangle", 0.025], [110.8, "sine", 0.012]].forEach(([f, type, v]) => {
        const o = ctx.createOscillator(), g = ctx.createGain(); o.type = type; o.frequency.value = f; g.gain.value = v; o.connect(g).connect(master); o.start();
      });
      timer = setTimeout(clank, 4000);
    },
    stop() {
      clearTimeout(timer);
      if (master) { const m = master; m.gain.linearRampToValueAtTime(0, ctx.currentTime + 0.6); setTimeout(() => m.disconnect(), 800); master = null; }
    },
    sting() {
      if (!ctx || !master) return;
      const t = ctx.currentTime, o = ctx.createOscillator(), g = ctx.createGain();
      o.type = "sawtooth"; o.frequency.setValueAtTime(880, t); o.frequency.exponentialRampToValueAtTime(140, t + 0.9);
      g.gain.setValueAtTime(0.25, t); g.gain.exponentialRampToValueAtTime(0.0001, t + 1.1);
      o.connect(g).connect(master); o.start(t); o.stop(t + 1.2);
    },
  };
})();

/* ------------------------------------------------------------ the haunted room (canvas background) */

function paintRoom() {
  // The room itself is official key art (#backdrop); the canvas adds slow drifting dust in the light.
  const back = $("#backdrop");
  if (back && typeof KEYART !== "undefined") back.style.backgroundImage = `url(${KEYART.room})`;
  const cv = $("#room"), cx = cv.getContext("2d");
  let W, H, dust = [];
  function size() {
    const dpr = Math.min(window.devicePixelRatio || 1, 2);
    W = innerWidth; H = innerHeight; cv.width = W * dpr; cv.height = H * dpr;
    cx.setTransform(dpr, 0, 0, dpr, 0, 0);
    dust = Array.from({ length: Math.round(W / 18) }, () => [Math.random() * W, Math.random() * H, 0.15 + Math.random() * 0.5, Math.random() * 6.28, 0.6 + Math.random() * 1.4]);
  }
  function frame(t) {
    cx.clearRect(0, 0, W, H);
    for (const d of dust) {
      d[1] -= d[2] * 0.3; d[0] += Math.sin(t / 2400 + d[3]) * 0.2;
      if (d[1] < -4) d[1] = H + 4;
      cx.fillStyle = `rgba(236,214,190,${0.18 + 0.2 * Math.sin(t / 900 + d[3]) ** 2})`;
      cx.beginPath(); cx.arc(d[0], d[1], d[4], 0, 7); cx.fill();
    }
  }
  let last = 0;
  function loop(t) { if (t - last > 40) { frame(t); last = t; } requestAnimationFrame(loop); }
  size(); frame(0);
  addEventListener("resize", () => { size(); frame(performance.now()); });
  if (motionOK) requestAnimationFrame(loop);
}

// Lights stutter and come back on, once per visit.
function powerOn() {
  let seen = false;
  try { seen = sessionStorage.getItem("tenno_lit") === "1"; sessionStorage.setItem("tenno_lit", "1"); } catch (e) { /* no storage */ }
  if (seen || !motionOK) return;
  document.body.classList.add("power-on");
  setTimeout(() => document.body.classList.remove("power-on"), 2600);
}

/* ------------------------------------------------------------ boot */

document.addEventListener("DOMContentLoaded", () => {
  buildViews();
  wireRadio();
  wire();
  renderWeapons();
  filterFrames();
  renderNews();
  refreshNews();
  $("#news-refresh").addEventListener("click", refreshNews);
  paintRoom();
  powerOn();
  const start = location.hash.slice(1);
  go(VIEWS[start] ? start : "home", false);
});
