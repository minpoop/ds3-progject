/* Tenno Field Manual - page logic. Needs art.js, data.js and companions.js loaded first. */
"use strict";

const CHAPTERS = [
  ["home", "Overview"], ["weapons", "Weapons"], ["warframes", "Warframes"], ["modding", "Modding"],
  ["systems", "Systems"], ["materials", "Materials"], ["currencies", "Currencies"], ["platinum", "Platinum"],
  ["steelpath", "Steel Path"], ["roadmap", "Roadmap"], ["tips", "Field Notes"],
];
const LINE_KEY = { roadmap: "progression" };
const ROLES = ["all", "tank", "support", "dps", "melee", "control", "defense", "loot", "stealth", "speed"];
const SYSTEM_ART = { "Archon Shards": "Archon Shard", "Operator, Focus & Amps": "Intact Sentient Core", "Kuva Liches & Sisters": "Kuva", "Railjack & Archwing": "Corrupted Holokey", "Incarnon weapons": "Primary Arcane Adapter" };
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
  return `
  <section class="hero">
    <div class="panel hero-copy">
      <div class="case">Unofficial fan guide · Warframe, October 2026</div>
      <h1 class="title">Build stronger.<br><em>Farm smarter.</em></h1>
      <p class="lede">A field manual from your first Star Chart mission to Steel Path: the best weapons at every Mastery Rank, every Warframe and what it's good at, mods, farming routes, Platinum and endgame. Six survivors from other nightmares ride along with tips.</p>
      <div class="facts"><span><b>${weaponPicks}</b>weapon picks</span><span><b>${FRAMES.length}</b>Warframes</span><span><b>${MATERIALS.length}</b>material routes</span><span><b>${SP_STEPS.length}</b>Steel Path steps</span></div>
    </div>
    <article class="panel featured">
      <div class="case">Frame of the day</div>
      ${art(f[0], "big")}
      <h3 class="sub">${esc(f[0])}</h3>
      <div class="pills"><span class="chip lamp">${esc(f[1])}</span><span class="chip">${esc(f[2])}</span></div>
      <p class="muted small">${esc(f[3])}</p>
      <button class="btn" type="button" data-go="warframes" data-q="${esc(f[0])}">Open in Warframes</button>
    </article>
  </section>
  <section class="head"><div class="case">The survivors</div><h3 class="sub">Pick who talks to you</h3>
    <p class="muted small">Tap a portrait to bring them on the radio in the corner. Each one has tips for every chapter.</p></section>
  <div class="cast">${COMPANIONS.map(c => `
    <button class="panel cast-card" type="button" data-cast="${c.id}" aria-pressed="false">${portrait(c.id, "cast")}
      <span class="who"><b>${esc(c.name)}</b><span>${esc(c.game)}</span></span></button>`).join("")}</div>
  <section class="head"><div class="case">Where to go</div><h3 class="sub">Jump to a chapter</h3></section>
  <div class="quick">
    ${[["weapons", "Arsenal", "Best weapons", "Two picks per slot for every MR 0-30, with why and how to get them."],
       ["warframes", "Roster", "Every Warframe", `All ${FRAMES.length} frames: what each is good at, where to farm it.`],
       ["modding", "Power", "Modding school", "Build skeletons, priority mods, damage types."],
       ["materials", "Resources", "Find materials", "Best mission for each resource, plus alternatives."],
       ["platinum", "Economy", "Earn Platinum", "Safe trading and what actually sells."],
       ["steelpath", "Endgame", "Enter Steel Path", "From unlock to Arcanes and Incarnons."]]
      .map(([id, c, t, d]) => `<button class="panel" type="button" data-go="${id}"><span class="case">${c}</span><b>${t}</b><span>${d}</span></button>`).join("")}
  </div>
  <section class="panel checklist">
    <div class="check-head"><div><div class="case">Progress tracker</div><h3 class="sub">Your power checklist</h3></div><span class="memo muted" id="task-count"></span></div>
    <div class="meter" role="progressbar" aria-label="Checklist progress" aria-valuemin="0" aria-valuemax="${CHECKLIST.length}"><i id="task-fill"></i></div>
    <div class="tasks">${CHECKLIST.map(([k, t]) => `<label class="task"><input type="checkbox" id="task-${k}" data-task="${k}"><span>${esc(t)}</span></label>`).join("")}</div>
    <p class="status">Saved in this browser only.</p>
  </section>`;
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

function viewTips() {
  return `${head(caseNo("tips"), "Field notes", "How to use this manual, where to double-check things, and the fine print.")}
  <div class="grid two">
    <article class="panel card"><h3>How to use this guide</h3><p>Start with the checklist on the Overview, open your current MR in Weapons, and visit Modding as soon as enemies start feeling tanky. Cards expand with a tap for acquisition details.</p></article>
    <article class="panel card"><h3>Check before you farm</h3><p>Drop tables, vendors, vaulted Primes and prices change with updates. Cross-check targeted farms here:</p>
      <p class="small"><a href="https://www.warframe.com/droptables" target="_blank" rel="noopener">Official drop tables</a> · <a href="https://wiki.warframe.com/w/Resources" target="_blank" rel="noopener">Wiki: resources</a> · <a href="https://warframe.market/" target="_blank" rel="noopener">warframe.market</a></p></article>
    <article class="panel card"><h3>The companions</h3><p>James, Simon, Dante, Vergil, Leon and Torque give original, fan-written tips. Use the radio's tone button to switch between funny, serious or mixed lines, and turn off scares in the sidebar if you'd rather not be startled.</p></article>
    <article class="panel card"><h3>Fine print</h3><p>Unofficial fan guide, not affiliated with Digital Extremes or the companions' publishers. Item art comes from the community warframe-items image set. Companion portraits are official character art from each game, via its fan wiki.</p></article>
  </div>`;
}

const VIEWS = { home: viewHome, weapons: viewWeapons, warframes: viewWarframes, modding: viewModding, systems: viewSystems, materials: viewMaterials, currencies: viewCurrencies, platinum: viewPlatinum, steelpath: viewSteelPath, roadmap: viewRoadmap, tips: viewTips };

/* ------------------------------------------------------------ state & navigation */

const state = {
  view: "home",
  mr: store.get("mr", 0),
  allRanks: false,
  role: "all",
  tone: store.get("tone", "mixed"),
  who: store.get("who", 0),
  open: store.get("radio", true),
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
  store.set("radio", open);
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
  const cv = $("#room"), cx = cv.getContext("2d");
  let W, H, base, dust = [], candles = [];
  let seed = 7;
  const rnd = () => (seed = (seed * 16807) % 2147483647) / 2147483647;
  function build() {
    const dpr = Math.min(window.devicePixelRatio || 1, 2);
    W = innerWidth; H = innerHeight; cv.width = W * dpr; cv.height = H * dpr;
    cx.setTransform(dpr, 0, 0, dpr, 0, 0);
    base = document.createElement("canvas"); base.width = W * dpr; base.height = H * dpr;
    const b = base.getContext("2d"); b.setTransform(dpr, 0, 0, dpr, 0, 0); seed = 7;
    // stone wall
    b.fillStyle = "#120d0b"; b.fillRect(0, 0, W, H);
    const bw = 74, bh = 34;
    for (let y = 0; y < H; y += bh) for (let x = -((y / bh) % 2) * bw / 2; x < W; x += bw) {
      const l = 14 + rnd() * 9; b.fillStyle = `rgb(${l + 6},${l},${l - 2})`; b.fillRect(x + 1, y + 1, bw - 2, bh - 2);
      if (rnd() < 0.08) { b.fillStyle = "rgba(90,20,14,.22)"; b.fillRect(x + 4, y + 4, bw * rnd(), bh - 8); }
    }
    // gothic arches with red light behind
    const n = Math.max(2, Math.round(W / 420)), aw = Math.min(190, W / (n * 1.8)), top = H * 0.16;
    for (let i = 0; i < n; i++) {
      const ax = (W / n) * (i + 0.5), ah = H * 0.52;
      const glow = b.createRadialGradient(ax, top + ah * 0.55, 10, ax, top + ah * 0.55, ah * 0.75);
      glow.addColorStop(0, "rgba(150,28,18,.55)"); glow.addColorStop(1, "rgba(150,28,18,0)");
      b.fillStyle = glow; b.fillRect(ax - ah, top - 40, ah * 2, ah * 1.6);
      b.beginPath(); b.moveTo(ax - aw / 2, top + ah); b.lineTo(ax - aw / 2, top + aw * 0.6);
      b.quadraticCurveTo(ax - aw / 2, top, ax, top - aw * 0.25); b.quadraticCurveTo(ax + aw / 2, top, ax + aw / 2, top + aw * 0.6);
      b.lineTo(ax + aw / 2, top + ah); b.closePath();
      const win = b.createLinearGradient(0, top, 0, top + ah); win.addColorStop(0, "rgba(70,14,10,.9)"); win.addColorStop(1, "rgba(25,8,6,.95)");
      b.fillStyle = win; b.fill(); b.lineWidth = 10; b.strokeStyle = "#1d1512"; b.stroke();
      b.strokeStyle = "rgba(20,12,10,.95)"; b.lineWidth = 4;
      for (let k = 1; k < 4; k++) { b.beginPath(); b.moveTo(ax - aw / 2, top + aw * 0.6 + k * ah / 4.2); b.lineTo(ax + aw / 2, top + aw * 0.6 + k * ah / 4.2); b.stroke(); }
      b.beginPath(); b.moveTo(ax, top - aw * 0.25); b.lineTo(ax, top + ah); b.stroke();
    }
    // iron beams
    b.fillStyle = "#0a0706"; b.fillRect(0, H * 0.05, W, 22); b.fillRect(0, H * 0.05 + 30, W, 6);
    for (let x = 20; x < W; x += 60) { b.fillStyle = "#2a1d18"; b.beginPath(); b.arc(x, H * 0.05 + 11, 3, 0, 7); b.fill(); }
    // chains
    for (let i = 0; i < Math.max(3, W / 300); i++) {
      const x = rnd() * W, len = H * (0.2 + rnd() * 0.45);
      b.strokeStyle = "rgba(60,44,36,.9)"; b.lineWidth = 2.5;
      for (let y = H * 0.05 + 30; y < H * 0.05 + 30 + len; y += 13) { b.beginPath(); b.ellipse(x, y, (y / 13) % 2 < 1 ? 4 : 1.6, 7, 0, 0, 7); b.stroke(); }
      if (rnd() < 0.5) { b.strokeStyle = "rgba(80,56,44,.9)"; b.lineWidth = 3; b.beginPath(); b.arc(x, H * 0.05 + 38 + len, 10, 0.2, Math.PI * 1.6); b.stroke(); }
    }
    // tattered banners
    for (let i = 0; i < Math.max(1, Math.floor(W / 700)); i++) {
      const x = W * (0.25 + i * 0.5) + rnd() * 40, w = 70, h = H * 0.3, y = H * 0.05 + 36;
      b.fillStyle = "rgba(92,18,14,.75)"; b.beginPath(); b.moveTo(x, y); b.lineTo(x + w, y); b.lineTo(x + w, y + h);
      for (let k = 6; k >= 0; k--) b.lineTo(x + (w * k) / 6, y + h - (k % 2 ? 18 : 0) - rnd() * 14);
      b.closePath(); b.fill();
      b.strokeStyle = "rgba(226,172,76,.22)"; b.lineWidth = 2; b.beginPath(); b.moveTo(x + w / 2, y + 30); b.lineTo(x + w / 2 + 16, y + 58); b.lineTo(x + w / 2, y + 86); b.lineTo(x + w / 2 - 16, y + 58); b.closePath(); b.stroke();
    }
    // floor ledge + candles
    b.fillStyle = "#0a0706"; b.fillRect(0, H * 0.86, W, H * 0.14);
    b.fillStyle = "#1c1411"; b.fillRect(0, H * 0.86, W, 6);
    candles = [];
    for (let x = 30 + rnd() * 50; x < W; x += 90 + rnd() * 160) {
      const h = 18 + rnd() * 34, y = H * 0.86;
      b.fillStyle = "#cbbca0"; b.fillRect(x - 4, y - h, 8, h); b.fillStyle = "rgba(0,0,0,.25)"; b.fillRect(x + 1, y - h, 3, h);
      candles.push([x, y - h - 6, rnd() * 6.28]);
    }
    dust = Array.from({ length: Math.round(W / 22) }, () => [rnd() * W, rnd() * H, 0.2 + rnd() * 0.6, rnd() * 6.28]);
  }
  function frame(t) {
    cx.drawImage(base, 0, 0, W, H);
    for (const [x, y, p] of candles) {
      const f = 1 + Math.sin(t / 180 + p) * 0.08 + Math.sin(t / 47 + p * 3) * 0.05;
      const g = cx.createRadialGradient(x, y, 1, x, y, 70 * f);
      g.addColorStop(0, "rgba(255,190,90,.55)"); g.addColorStop(0.25, "rgba(226,140,60,.18)"); g.addColorStop(1, "rgba(226,140,60,0)");
      cx.fillStyle = g; cx.fillRect(x - 80, y - 80, 160, 160);
      cx.fillStyle = "#ffd890"; cx.beginPath(); cx.ellipse(x, y, 2.4, 6 * f, 0, 0, 7); cx.fill();
    }
    cx.fillStyle = "rgba(220,200,180,.35)";
    for (const d of dust) {
      d[1] -= d[2] * 0.25; d[0] += Math.sin(t / 2000 + d[3]) * 0.15;
      if (d[1] < 0) d[1] = H;
      cx.fillRect(d[0], d[1], 1.4, 1.4);
    }
  }
  let last = 0;
  function loop(t) { if (t - last > 50) { frame(t); last = t; } requestAnimationFrame(loop); }
  build(); frame(0);
  addEventListener("resize", () => { build(); frame(performance.now()); });
  if (motionOK) requestAnimationFrame(loop);
}

/* ------------------------------------------------------------ boot */

document.addEventListener("DOMContentLoaded", () => {
  buildViews();
  wireRadio();
  wire();
  renderWeapons();
  filterFrames();
  paintRoom();
  const start = location.hash.slice(1);
  go(VIEWS[start] ? start : "home", false);
});
