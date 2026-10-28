/* The six horror/action companions who give tips in the bottom-right corner.
   All dialogue is original fan writing (only short nods to each character's game),
   Portraits are official character renders (assets/portraits.js); portraitSVG draws a fallback bust. */
"use strict";

const COMPANIONS = [
  { id: "james", name: "James Sunderland", game: "Silent Hill 2", accent: "#9aa36b",
    tagline: "Quiet, uneasy, oddly helpful.",
    scare: "There was a farm route here. It's gone now." },
  { id: "simon", name: "Simon Henriksson", game: "Cry of Fear", accent: "#8ea3bb",
    tagline: "Sarcastic, exhausted, still farming.",
    scare: "My phone just buzzed. It says you sold a Prime part for 2 Platinum." },
  { id: "dante", name: "Dante", game: "Devil May Cry 5", accent: "#d04a3c",
    tagline: "Cocky, loud, all about style.",
    scare: "Boo. Ha! Gotcha. Now go rank your mods." },
  { id: "vergil", name: "Vergil", game: "Devil May Cry 5", accent: "#5f8fd6",
    tagline: "Cold, focused, wants more power.",
    scare: "Your build... is weak." },
  { id: "leon", name: "Leon S. Kennedy", game: "Resident Evil", accent: "#c79a5c",
    tagline: "Dry humour, survival instincts.",
    scare: "Something's in the vents. ...It's an Infested Charger. Of course it is." },
  { id: "torque", name: "Torque", game: "The Suffering", accent: "#b8562f",
    tagline: "Grim, blunt, survived Carnate.",
    scare: "Abbott State Penitentiary was quieter than your trade chat." },
];

// LINES[section][companion] = [[label, line, tone]]; tone "f" = funny, "s" = serious.
const LINES = {
  home: {
    james: [
      ["A MAP, FINALLY", "The Star Chart actually shows you Junction objectives. In Silent Hill I would have killed for one working street sign.", "f"],
      ["IN MY RESTLESS DREAMS", "In my restless dreams, I see the Foundry. Everything has finally finished crafting.", "f"],
      ["WHERE TO START", "Start with planet Junctions. They unlock more of the Star Chart and lead you to the main quests.", "s"],
      ["SAVE YOUR PLATINUM", "That starting Platinum is tempting. Warframe and weapon slots are more useful than rushing a Foundry timer.", "s"],
    ],
    simon: [
      ["PHONE CHECK", "At least my phone is showing an actual plan for once instead of leading me deeper into a nightmare.", "f"],
      ["STOCKHOLM IS MORE ORGANISED", "This game has enough side objectives to make Stockholm at night look organised.", "f"],
      ["STAY ON TASK", "Pick one quest and finish it. Then pick the next one. That's the whole trick.", "s"],
      ["ALWAYS BE BUILDING", "Keep something in the Foundry at all times. Levelling new gear to 30 is how Mastery goes up.", "s"],
    ],
    dante: [
      ["JACKPOT, TENNO", "Quests and Junctions first, shopping spree later. The fun part comes after the paperwork.", "f"],
      ["STYLE COUNTS", "Slide, bullet jump, look cool, and try not to land in a laser grid. Movement is half the battle.", "f"],
      ["DON'T SKIP THE STORY", "Main quests unlock the best toys. Play them in order and the game opens up fast.", "s"],
    ],
    vergil: [
      ["FOCUS", "Power is accumulated deliberately. Clear the next Junction before scattering your efforts.", "s"],
      ["DISCIPLINE", "Ten unfinished grinds provide less strength than one completed build. Choose.", "s"],
      ["I NEED MORE POWER", "Mastery Rank alone is not power. Mods, Forma and patience are.", "f"],
    ],
    leon: [
      ["RACCOON CITY HAD FEWER MENUS", "Start with Junctions and quests. At least this star chart tells you which doors are locked.", "f"],
      ["SUPPLY MANAGEMENT", "Spend starting Platinum on inventory slots. That's the kind of supply management I can get behind.", "s"],
      ["ONE CRISIS AT A TIME", "Finish one objective before chasing five side activities. That's how you survive an outbreak.", "s"],
    ],
    torque: [
      ["CARNATE ISLAND RULE", "Clear the way in front of you before you worry about the next gate. Junctions first.", "s"],
      ["A CELL WITH A VIEW", "The Origin System has a lot of locked doors. At least these ones come with a checklist.", "f"],
      ["KEEP MOVING", "Find the next quest, prep the gear, get through it. Standing still never did me any favours.", "s"],
    ],
  },
  weapons: {
    james: [
      ["THE PLANK OF WOOD", "I got pretty far with a plank of wood. That does not mean you should take an unmodded Skana into Steel Path.", "f"],
      ["ANOTHER LOCKED DOOR", "The blueprint says MR8, the vendor says Rank 5 and the quest says \"not yet\". A familiar series of locked doors.", "f"],
      ["MODS FIRST", "A ranked damage mod and a good elemental setup often beat an unmodded higher-MR weapon.", "s"],
      ["KEEP THE GOOD ONES", "Before selling a weapon, check if it's a crafting ingredient or gets an Incarnon upgrade later.", "s"],
    ],
    simon: [
      ["CRY OF FORMA", "Five Forma and the gun still tickles enemies? Rank the core mods before you buy more hope.", "f"],
      ["DON'T PANIC-FIRE", "One well-built shotgun beats a whole backpack of badly modded weapons.", "s"],
      ["MASTERY FODDER", "Level everything for Mastery, then sell it. Only keep and Forma the ones you actually like.", "s"],
    ],
    dante: [
      ["SHOWTIME", "A weapon is worth keeping if clearing a room with it feels like a combo video.", "f"],
      ["DOUBLE THE FUN", "Multishot, crit and elemental mods turn ordinary guns into the good kind of excessive.", "s"],
      ["SWORD GUY APPROVES", "Glaive Prime, Nikana Prime, Praedos. Sharp, stylish, effective. My kind of shopping list.", "f"],
    ],
    vergil: [
      ["THE CUT", "Remove redundant mods. Every slot must contribute to the weapon's purpose.", "s"],
      ["TRUE POWER", "Incarnon evolutions and weapon Arcanes separate an ordinary blade from a perfected one.", "s"],
      ["A KATANA IS ENOUGH", "You carry three weapons. I carried one. Choose the one you'll actually master.", "f"],
    ],
    leon: [
      ["AMMO DISCIPLINE", "A well-modded gun beats a shiny one that barely scratches armour. Damage and multishot first.", "s"],
      ["KNIFE WORK", "A good melee build can carry you. I learned that the hard way. So did my knife.", "f"],
      ["RELOAD CHECK", "Try weapons for Mastery, but save Catalysts and Forma for the ones worth keeping.", "s"],
    ],
    torque: [
      ["PRISON YARD ARSENAL", "A weapon's only as good as what you put into it. Upgrade core mods before hunting another blueprint.", "s"],
      ["NO EMPTY HANDS", "Bring something for crowds and something for tough targets. Carnate taught me not to rely on one trick.", "s"],
      ["BLUNT SOLUTION", "If the numbers stop going up, check mods, damage type and capacity. Hitting harder is a plan. Hoping isn't.", "f"],
    ],
  },
  warframes: {
    james: [
      ["MESMER SKIN", "Revenant ignores enough danger that even I might survive a hallway encounter.", "f"],
      ["FROST MAKES SENSE", "A snow globe around the Defense target. Finally, a room I can lock from the inside for a good reason.", "f"],
      ["EVERYONE HAS A ROLE", "Not every frame needs to be meta. Ivara is great for stealth, Nekros for loot and Frost for defence.", "s"],
    ],
    simon: [
      ["I'LL TAKE THE TANK", "If enemies are ruining your day, try Revenant, Rhino or another frame that forgives mistakes.", "s"],
      ["NEKROS JOB", "Apparently raising the dead is easier than getting Polymer Bundles. Bring Nekros.", "f"],
      ["PRIME ISN'T EVERYTHING", "A well-modded regular frame can clear the Star Chart and Steel Path. Prime is a bonus.", "s"],
    ],
    dante: [
      ["PICK YOUR STYLE", "Mesa shreds with guns, Gauss outruns everything and Kullervo turns melee into a highlight reel.", "s"],
      ["HEY, NICE NAME", "There's a Warframe called Dante? Overguard for the whole team? Yeah, he's got taste.", "f"],
      ["TEAM PLAY", "Wisp buffs the squad and Dante stacks Overguard. Some heroes make the whole party look good.", "s"],
    ],
    vergil: [
      ["A WORTHY VESSEL", "A Warframe is an instrument. Choose one suited to the mission, then refine it.", "s"],
      ["QUIET DOMINANCE", "Ivara excels at stealth and objective work. Not every victory needs a spectacle.", "s"],
      ["BROTHERLY ADVICE", "A frame named after my brother. It survives everything and never shuts up. Accurate.", "f"],
    ],
    leon: [
      ["TACTICAL PICK", "Pick a frame for the objective. Defence, stealth and survival all want different tools.", "s"],
      ["SOME COVER WOULD BE NICE", "Revenant and Rhino make hard missions less punishing. I could've used that in a certain Spanish village.", "f"],
      ["A GOOD PARTNER", "Wisp is excellent support. Keeping the squad moving saves revives.", "s"],
    ],
    torque: [
      ["SURVIVOR FIRST", "A sturdy Warframe is a good first investment. You can't clear a mission from the floor.", "s"],
      ["SHADOWS", "Nekros raises shadows from the dead. I've seen worse things crawl out of the dark.", "f"],
      ["CONTROL THE ROOM", "Frost and Vauban are useful when the objective needs protecting more than another damage race.", "s"],
    ],
  },
  modding: {
    james: [
      ["THE REAL MONSTER", "It wasn't the monster. It was the unranked Serration I forgot to upgrade.", "f"],
      ["CAPACITY MATTERS", "Catalysts, Reactors and Forma are precious. Use them on gear you plan to keep.", "s"],
      ["A HEAVY BARGAIN", "More Strength for less Efficiency? A deal with a terrible downside. I have experience with those.", "f"],
    ],
    simon: [
      ["MODDING FEAR", "An empty mod slot in Steel Path is basically an invitation to get chased again.", "f"],
      ["ELEMENTAL CHAOS", "Stop combining damage types at random. Read what each status effect actually does.", "s"],
      ["ENDO PRIORITIES", "Don't spread Endo over fifty rank-zero mods. Rank multishot, damage and survivability first.", "s"],
    ],
    dante: [
      ["BUILD A COMBO", "Damage, multishot, crit and elements are your stylish little combo chain.", "s"],
      ["POWER UP", "Forma is just training. Painful, repetitive training with excellent results.", "f"],
    ],
    vergil: [
      ["OPTIMISE", "The strongest mod is not always the right one. Remove inefficiency.", "s"],
      ["NO WASTED MOTION", "Build for the weapon's strengths rather than forcing every weapon into one configuration.", "s"],
      ["MOTIVATION", "Galvanized mods. Arcanes. More power. Yes.", "f"],
    ],
    leon: [
      ["UPGRADE THE GEAR", "Damage, multishot and elements are the essentials. No fancy gear fixes an empty build.", "s"],
      ["INVENTORY TETRIS", "A Catalyst doubles capacity. Forma makes room for the loadout you actually want. Like an attaché case.", "f"],
      ["AIM FOR WEAK POINTS", "Check crit and status stats instead of installing every mod you own.", "s"],
    ],
    torque: [
      ["THE REAL MONSTERS ARE BAD BUILDS", "If enemies shrug off everything, rank the mods before blaming the gun.", "f"],
      ["USE WHAT WORKS", "You don't need the most expensive setup. You need one that keeps working when it gets ugly.", "s"],
      ["STAY SHARP", "Health, shields, armour or Overguard: have a way to survive while you deal damage.", "s"],
    ],
  },
  systems: {
    james: [
      ["A CHAIR THAT EATS FRAMES", "The Helminth is a living thing in your ship that eats Warframes. I've seen stranger rooms. Not many.", "f"],
      ["SHARDS", "Archon Shards are permanent boosts. Put them on the frame you play most, not the one you might play someday.", "s"],
    ],
    simon: [
      ["NEMESIS, AGAIN", "A Kuva Lich hunting me across the system? Great. Just like old times.", "f"],
      ["PICK THE WEAPON FIRST", "Look at the Larvling's weapon before you mercy-kill it. You're stuck with that Lich's weapon.", "s"],
    ],
    dante: [
      ["PET ADVICE", "Bring a companion with Vacuum. Picking up loot by hand is not stylish.", "f"],
      ["ARCHON HUNT", "Weekly boss hunt, guaranteed shard. That's my kind of job board.", "s"],
    ],
    vergil: [
      ["FOCUS", "Zenurik for energy, Madurai for damage. Choose a school and commit.", "s"],
      ["SUBSUME", "Feed your weakest frames to the Helminth. Strength comes from sacrifice.", "f"],
    ],
    leon: [
      ["PREP THE RIDE", "The New War needs a Railjack and a Necramech. Start both farms before the quest stops you.", "s"],
      ["NEMESIS PROTOCOL", "Getting stalked by someone who keeps coming back? Been there. Guess the Requiem order and end it.", "f"],
    ],
    torque: [
      ["THE THING INSIDE", "The Helminth. Something living in the walls that changes you. Yeah. I know how that goes.", "f"],
      ["INCARNON", "Old starter guns become monsters with an Incarnon adapter. Even the weak can turn into something dangerous.", "s"],
    ],
  },
  materials: {
    james: [
      ["SOMETHING IN THE WATER", "Ophelia on Uranus is a great resource farm. The name alone makes me uneasy.", "f"],
      ["PLASTIDS, AGAIN", "I've been searching for Mary. You've been searching for Plastids. We're both tired of Uranus.", "f"],
      ["ARGON ANXIETY", "Argon Crystals decay daily. Farm them when you're ready to craft.", "s"],
    ],
    simon: [
      ["ALWAYS POLYMER", "All these planets and I'm still short on Polymer Bundles. Incredible.", "f"],
      ["A FRIEND HELPS", "Loot frames and boosters cut the grind. Bring a squad when the farm is awful.", "s"],
    ],
    dante: [
      ["LOOT PINATA", "Nekros, Khora and loot boosters make the resource grind much less tragic.", "f"],
      ["KEEP MOVING", "Pick a node, clear it fast and grab enough to fund your next build.", "s"],
    ],
    vergil: [
      ["PREPARATION", "Know the drop source, objective and amount required before you deploy.", "s"],
      ["FARM WITH PURPOSE", "One deliberate route is worth more than aimless repetition.", "s"],
    ],
    leon: [
      ["CHECK EVERY ROOM", "Know which node drops your resource. Searching random missions is like hunting a key in the wrong building.", "f"],
      ["STOCK UP", "Need Orokin Cells for several blueprints? Farm in one batch and skip the extra trips.", "s"],
    ],
    torque: [
      ["SUPPLY RUN", "Pick the resource and go to the right mission. An aimless search wastes time and lives.", "s"],
      ["FIND THE EXIT", "Once you have what you came for, extract. You don't owe the mission another twenty minutes.", "f"],
    ],
  },
  currencies: {
    james: [
      ["CURRENCY NIGHTMARE", "Credits, Endo, Kuva, standing. Another pocket full of keys to doors I haven't found yet.", "f"],
      ["DAILY LIMIT", "Standing caps reset daily. Better to plan the grind than hit an invisible wall.", "s"],
    ],
    simon: [
      ["ENDO AGAIN", "I had a fortune in Credits. Two mod upgrades later I live in poverty.", "f"],
      ["TINY TOKENS", "Pathos Clamps and Holokeys look innocent until the shop asks for forty.", "f"],
      ["KNOW THE GOAL", "Choose the item first, then farm the currency that buys it.", "s"],
    ],
    dante: [
      ["PAYDAY", "Credits are for upgrades, Platinum is for slots and smart trades. Don't mix them up.", "s"],
      ["DEBT COLLECTOR", "Trust me, I know what being broke feels like. Check your wallet before you farm.", "f"],
    ],
    vergil: [
      ["RESOURCE CONTROL", "Spend scarce currencies on upgrades that give lasting strength.", "s"],
      ["RESTRAINT", "Daily standing caps reward consistency over impatience.", "s"],
    ],
    leon: [
      ["DIFFERENT KIND OF MERCHANT", "Endo, Credits and standing all do different jobs. Spend what you need, not whatever's in your pocket.", "s"],
      ["NO REFUNDS", "Check what the vendor sells before burning rare currency on an upgrade you won't use.", "f"],
    ],
    torque: [
      ["EVERYTHING HAS A PRICE", "Credits, Endo and Steel Essence run out at bad times. Know what the next upgrade costs.", "s"],
      ["COUNT THE DAYS", "Faction caps mean the grind has a pace. Plan around it, like a sentence.", "f"],
    ],
  },
  platinum: {
    james: [
      ["NO MORE RUSHING", "Paying Platinum to finish a three-day build instantly is a decision I'd make at 2 a.m. and regret by sunrise.", "f"],
      ["THE MYSTERY OFFER", "A stranger offers a rare Prime set for almost nothing. Even I know to double-check the trade window.", "s"],
      ["ONE LAST RELIC", "\"One more relic and I'll get the rare part.\" A phrase best said five hours before bed.", "f"],
    ],
    simon: [
      ["MARKET HORROR", "Selling a valuable Prime part for two Platinum is a jump scare with no soundtrack.", "f"],
      ["DO YOUR HOMEWORK", "Check recent Warframe Market listings before you trade.", "s"],
    ],
    dante: [
      ["LET'S MAKE A DEAL", "Prime parts, mods and Arcanes turn a little farming into Platinum. Jackpot.", "s"],
      ["SPEND WITH STYLE", "New outfits are fun, but buy enough slots for weapons first.", "f"],
    ],
    vergil: [
      ["VALUE", "Verify the market price. A warrior who trades blindly surrenders advantage.", "s"],
      ["PATIENCE", "Do not accept the first offer merely because it appeared.", "s"],
    ],
    leon: [
      ["WHAT ARE YA BUYIN'", "I've met a merchant or two. Trade spare Prime parts for Platinum, but check prices first.", "f"],
      ["CONFIRM THE TRADE", "Check item, rank, quantity and price. You can't reload a save after you click accept.", "s"],
    ],
    torque: [
      ["CELL BLOCK ECONOMICS", "Sell what you don't need, buy slots before luxuries, check the price twice.", "s"],
      ["NO BAD BARGAINS", "A suspiciously cheap offer deserves a second look. In prison, so does everything else.", "f"],
    ],
  },
  steelpath: {
    james: [
      ["WELCOME BACK", "The same maps, but everything wants you dead harder. Like revisiting a certain town with the fog turned up.", "f"],
      ["THE RADIO IS LOUD", "If an Acolyte drops you instantly, your defences are sending a pretty clear message.", "f"],
      ["SLOW AND STEADY", "Start with a sturdy frame, ranked mods and a reliable weapon. Survive first, speed later.", "s"],
    ],
    simon: [
      ["HARD MODE", "Steel Path is what happens when the game catches you being overconfident.", "f"],
      ["ARBITRATION FIRST", "Get Galvanized mods and improve your weapon before insisting the enemies are unfair.", "s"],
    ],
    dante: [
      ["NOW WE'RE TALKING", "Steel Path is where the combo meter finally gets interesting. Bring a real build.", "f"],
      ["DEVIL TRIGGER ENERGY", "Arcanes and Incarnon upgrades are your power-up sequence. Use them.", "s"],
    ],
    vergil: [
      ["POWER IS EVERYTHING", "Steel Path rewards perfected equipment, not wishful thinking.", "s"],
      ["PERFECT DEFENCE", "Shields, Overguard, damage reduction or control: rely on one survival system.", "s"],
      ["MOTIVATED", "Harder enemies. More rewards. Finally, something worth my time.", "f"],
    ],
    leon: [
      ["PROFESSIONAL MODE", "Steel Path punishes sloppy builds. Galvanized mods, survivability and reliable damage first.", "s"],
      ["BOSS ROOM RULES", "If Acolytes wipe you, fix your defences and single-target damage before going back in.", "s"],
      ["NO HEROICS", "Use movement and abilities. Tanking every shot isn't a plan. Trust me, I've tried.", "f"],
    ],
    torque: [
      ["CARNATE HAD TEETH", "So does Steel Path. Build for survival before you go looking for a bigger fight.", "s"],
      ["STAY ALIVE", "Pick a frame that gives you breathing room. A dead Tenno doesn't deal damage.", "s"],
      ["THE BEAST WITHIN", "Feels like letting the monster out, doesn't it? Good. Now point it at the Acolyte.", "f"],
    ],
  },
  progression: {
    james: [
      ["THE STAR CHART DOOR", "You need a quest to unlock a node to farm a part to build a thing that unlocks a quest. Yes. Perfectly clear.", "f"],
      ["PREP FOR THE NEW WAR", "The New War needs a Railjack and a Necramech. Start those farms before the quest is next.", "s"],
      ["FOLLOW THE SIGNS", "Main quests unlock major systems. Even when the route gets strange, follow the objectives.", "s"],
    ],
    simon: [
      ["ONE THING", "You don't need a Necramech, an Amp and five Warframes tonight. Pick one.", "s"],
      ["WEEKLY NIGHTMARE", "Nightwave, Circuit, hunts, standing. Congratulations, your hobby now has a schedule.", "f"],
    ],
    dante: [
      ["LEVEL UP", "Quests unlock the toys. Build them, rank them, then go pick bigger fights.", "s"],
      ["SIDE QUESTS", "Keep one fun goal going next to the serious grind. Pizza breaks optional.", "f"],
    ],
    vergil: [
      ["THE PATH", "Complete the systems in order. Each milestone removes the next obstacle.", "s"],
      ["NO DISTRACTIONS", "The next useful unlock matters more than an unrelated collection goal.", "s"],
    ],
    leon: [
      ["FOLLOW THE OBJECTIVE", "Quests unlock the major systems. Start there, then work toward the gear you want.", "s"],
      ["DON'T SPLIT THE TEAM", "Finish one goal before starting six more. Splitting up never ends well.", "f"],
    ],
    torque: [
      ["ONE GATE AT A TIME", "The next quest or Junction is usually the next lock worth opening.", "s"],
      ["ESCAPE PLAN", "Write down the upgrade you need, what it costs and where to get it. Then move.", "s"],
    ],
  },
  news: {
    james: [
      ["A LETTER ARRIVED", "Someone sent news from a place I thought I'd left behind. At least these letters link to patch notes.", "f"],
      ["READ THE NOTES", "Hotfixes change drop tables and fix bugs. Skim them before you plan a long farm.", "s"],
    ],
    simon: [
      ["PHONE NOTIFICATION", "Another hotfix. My phone buzzes more for Warframe than for actual people.", "f"],
      ["EVENTS EXPIRE", "Limited events and Baro visits end. Check the dates so you don't miss the reward.", "s"],
    ],
    dante: [
      ["NEW TOYS", "New update? New weapons? Sounds like a party. Read what dropped and go break it.", "f"],
      ["DEVSTREAMS", "Devstreams show what's coming. Save your resources if your favourite thing is getting buffed.", "s"],
    ],
    vergil: [
      ["INTELLIGENCE", "Knowledge of the next update is power. Plan your Forma accordingly.", "s"],
      ["PATIENCE", "Do not invest in what the next patch will change. Read first.", "s"],
    ],
    leon: [
      ["FIELD REPORT", "Always read the briefing. Known Issues lists save you from bugs you'd swear were your fault.", "s"],
      ["HQ CHECKING IN", "Good to get updates from HQ that aren't telling me to go somewhere worse.", "f"],
    ],
    torque: [
      ["WORD FROM OUTSIDE", "News from outside the walls. Read it. Things change fast out there.", "s"],
      ["NEW SENTENCE", "Fresh update, fresh grind. Like getting another sentence, but with better loot.", "f"],
    ],
  },
  tips: {
    james: [
      ["THE DROP TABLE ISN'T HAUNTED", "The odds are just cruel. Verify the mission and try again.", "f"],
      ["THE QUIET ROUTE", "Play in squads when it saves time. Sometimes not being alone is the best strategy.", "s"],
    ],
    simon: [
      ["PLEASE READ", "The Codex and wiki exist. Stop spending forty minutes in the wrong node.", "s"],
      ["RNG JUMPSCARE", "A rare drop isn't \"due\". Probability has never cared how tired you are.", "f"],
    ],
    dante: [
      ["TRICKSTER", "Try a new weapon or frame now and then. A stylish Tenno has options.", "s"],
      ["ENJOY THE FIGHT", "If the grind feels like work, switch missions. The best farm is one you can stand repeating.", "f"],
    ],
    vergil: [
      ["KNOWLEDGE", "Check acquisition and reward rotations before committing time.", "s"],
      ["EFFICIENT PRACTICE", "Improve one weak part of your loadout each session. Small refinements compound.", "s"],
    ],
    leon: [
      ["CHECK THE FILES", "Verify drop tables before farming. Running the wrong mission is an avoidable horror story.", "f"],
      ["TAKE INVENTORY", "Before building something expensive, check what blueprints and resources you already own.", "s"],
    ],
    torque: [
      ["READ THE WALL", "Check the mission rewards before you throw yourself into another grind.", "s"],
      ["NO MORE WASTED RUNS", "The official drop tables are there for a reason. Use them.", "f"],
    ],
  },
};

// Official portrait when assets/portraits.js has one, otherwise the drawn bust below.
function portrait(id, uid) {
  const c = COMPANIONS.find(x => x.id === id);
  if (typeof PORTRAITS !== "undefined" && PORTRAITS[id]) return `<img class="portrait" src="${PORTRAITS[id]}" alt="${c.name} portrait" decoding="async">`;
  return portraitSVG(id, uid);
}

// Hand-drawn bust portraits. `uid` keeps gradient ids unique when a portrait appears twice.
function portraitSVG(id, uid) {
  const g = `${id}-${uid}`;
  const skin = { james: "#d9b08f", simon: "#d7b39a", dante: "#e1bb9c", vergil: "#e3c0a3", leon: "#dcb08c", torque: "#b68262" }[id];
  const bg = { james: "#3b3f2a", simon: "#2a3240", dante: "#4a1414", vergil: "#16243f", leon: "#3d2c1b", torque: "#3d1d10" }[id];
  const eyes = (y = 82, c = "#1b1512") =>
    `<path d="M64 ${y} q6 -3 11 0" stroke="${c}" stroke-width="2.2" fill="none" stroke-linecap="round"/>` +
    `<path d="M86 ${y} q6 -3 11 0" stroke="${c}" stroke-width="2.2" fill="none" stroke-linecap="round"/>` +
    `<ellipse cx="69.5" cy="${y + 3}" rx="2.6" ry="2" fill="${c}"/><ellipse cx="91.5" cy="${y + 3}" rx="2.6" ry="2" fill="${c}"/>`;
  const head = (cx = 80) =>
    `<rect x="${cx - 11}" y="104" width="22" height="26" fill="${skin}"/>` +
    `<path d="M${cx - 11} 118 q11 8 22 0 v6 q-11 8 -22 0z" fill="#000" opacity=".18"/>` +
    `<ellipse cx="${cx}" cy="82" rx="26" ry="31" fill="${skin}"/>` +
    `<path d="M${cx - 26} 82 a26 31 0 0 0 26 31 a22 31 0 0 1 -18 -31z" fill="#000" opacity=".12"/>` +
    `<path d="M${cx - 2} 86 l-3 12 h7" stroke="#00000040" stroke-width="1.6" fill="none"/>`;
  const frame =
    `<defs><radialGradient id="bg-${g}" cx="50%" cy="38%" r="70%"><stop offset="0" stop-color="${bg}"/><stop offset="1" stop-color="#050404"/></radialGradient>` +
    `<linearGradient id="lit-${g}" x1="0" x2="1"><stop offset="0" stop-color="#fff" stop-opacity=".16"/><stop offset=".55" stop-color="#fff" stop-opacity="0"/><stop offset="1" stop-color="#000" stop-opacity=".35"/></linearGradient></defs>` +
    `<rect width="160" height="200" fill="url(#bg-${g})"/>`;
  const shade = `<rect width="160" height="200" fill="url(#lit-${g})"/>`;
  let body = "";
  switch (id) {
    case "james":
      body =
        `<path d="M16 200 C20 152 44 134 80 130 C116 134 140 152 144 200Z" fill="#4c5636"/>` +
        `<path d="M64 130 L80 160 L96 130 L92 200 H68Z" fill="#c9c4b4"/>` +
        `<path d="M56 132 L80 166 L70 200 H44 C46 170 50 148 56 132Z" fill="#5d6943"/><path d="M104 132 L80 166 L90 200 H116 C114 170 110 148 104 132Z" fill="#5d6943"/>` +
        `<rect x="104" y="160" width="16" height="12" rx="2" fill="#2a2a24"/><circle cx="116" cy="163" r="1.8" fill="#ff3b2f"/>` +
        `<path d="M38 176 l26 -8 l3 9 l-26 8z" fill="#222"/><path d="M64 168 L160 120 L160 176 L67 177z" fill="#ffe8a6" opacity=".18"/>` +
        head() +
        `<path d="M54 76 C52 50 66 44 82 45 C99 46 108 56 106 78 C102 64 96 58 84 58 C72 58 64 62 58 76Z" fill="#a98a5a"/>` +
        `<path d="M54 76 C55 66 60 60 66 58 C62 66 60 72 58 80Z" fill="#8c7046"/>` +
        eyes(83) + `<path d="M72 102 q8 2 16 0" stroke="#6b4a3a" stroke-width="2" fill="none" stroke-linecap="round"/>`;
      break;
    case "simon":
      body =
        `<path d="M50 70 C48 40 112 40 110 70 L112 120 C100 112 60 112 48 120Z" fill="#3e4247"/>` +
        `<path d="M14 200 C18 150 42 132 80 128 C118 132 142 150 146 200Z" fill="#5b5f66"/>` +
        `<path d="M58 128 C66 140 94 140 102 128 C98 120 62 120 58 128Z" fill="#474b51"/>` +
        `<path d="M72 138 v24 M88 138 v24" stroke="#d8d8d8" stroke-width="2"/>` +
        head() +
        `<path d="M54 80 C50 48 72 42 86 46 C104 50 110 62 106 82 C100 66 94 60 80 62 C68 62 60 68 56 84Z" fill="#2b2119"/>` +
        `<path d="M60 64 l8 12 l2 -14 l8 10 l2 -12 l8 10 l4 -10 l6 12" fill="#2b2119"/>` +
        eyes(84, "#191512") + `<path d="M64 90 q6 3 11 0 M86 90 q6 3 11 0" stroke="#7a5a50" stroke-width="1.4" fill="none" opacity=".7"/>` +
        `<path d="M73 103 h14" stroke="#6b4a3a" stroke-width="2" stroke-linecap="round"/>` +
        `<rect x="98" y="150" width="20" height="34" rx="3" fill="#111"/><rect x="100" y="153" width="16" height="27" rx="1" fill="#bfe3ff"/>` +
        `<path d="M108 150 L80 60 L140 60z" fill="#bfe3ff" opacity=".08"/>`;
      break;
    case "dante":
      body =
        `<path d="M112 20 l10 -6 l6 10 l-8 6z" fill="#b9b9b9"/><rect x="113" y="26" width="8" height="70" transform="rotate(14 117 60)" fill="#555"/>` +
        `<path d="M12 200 C16 150 42 132 80 128 C118 132 144 150 148 200Z" fill="#8e1b1b"/>` +
        `<path d="M62 130 L80 200 L98 130 C92 128 68 128 62 130Z" fill="#1c1c1c"/>` +
        `<path d="M52 134 L76 200 H40 C42 172 46 150 52 134Z M108 134 L84 200 H120 C118 172 114 150 108 134Z" fill="#a52626"/>` +
        `<path d="M40 150 h80" stroke="#6a1212" stroke-width="2" opacity=".6"/>` +
        head() +
        `<path d="M50 90 C44 52 64 38 84 40 C108 42 116 60 110 92 C108 76 104 66 98 62 L96 78 L88 60 L80 76 L74 60 L66 78 L62 64 C56 70 54 80 50 90Z" fill="#e8e6e2"/>` +
        `<path d="M50 90 C52 104 54 112 58 116 L56 92Z M110 92 C108 104 106 112 102 116 L104 92Z" fill="#d6d3cd"/>` +
        eyes(84, "#2a2420") + `<path d="M70 104 q10 4 20 -2" stroke="#6b4a3a" stroke-width="2" fill="none" stroke-linecap="round"/>` +
        `<path d="M62 98 q18 18 36 0" stroke="#9a8a7a" stroke-width="3" fill="none" opacity=".35"/>`;
      break;
    case "vergil":
      body =
        `<rect x="30" y="18" width="7" height="92" transform="rotate(-18 34 64)" fill="#141414"/><rect x="26" y="100" width="18" height="5" rx="2" transform="rotate(-18 34 64)" fill="#c8a04a"/>` +
        `<path d="M12 200 C16 150 42 132 80 128 C118 132 144 150 148 200Z" fill="#1d3768"/>` +
        `<path d="M58 126 L80 150 L102 126 L106 112 L80 132 L54 112Z" fill="#24427c"/>` +
        `<path d="M66 134 L80 200 L94 134Z" fill="#111827"/><path d="M60 160 h40" stroke="#c8a04a" stroke-width="2" opacity=".7"/>` +
        head() +
        `<path d="M52 82 C48 50 66 40 84 40 C104 40 114 52 108 82 C104 66 98 58 90 54 L120 46 L92 50 L118 36 L86 46 C74 46 64 50 58 64Z" fill="#e9e8e6"/>` +
        `<path d="M52 82 C52 66 56 56 64 50 L58 70Z" fill="#d2d0cc"/>` +
        eyes(84, "#1f2a3a") + `<path d="M64 80 l11 -3 M86 77 l11 3" stroke="#1b1512" stroke-width="2.4" stroke-linecap="round"/>` +
        `<path d="M72 104 h16" stroke="#6b4a3a" stroke-width="2" stroke-linecap="round"/>`;
      break;
    case "leon":
      body =
        `<path d="M12 200 C16 150 42 132 80 128 C118 132 144 150 148 200Z" fill="#5a3a22"/>` +
        `<path d="M44 140 C56 126 70 124 80 126 C90 124 104 126 116 140 C108 148 94 146 80 140 C66 146 52 148 44 140Z" fill="#cdb48a"/>` +
        `<path d="M66 142 L80 200 L94 142Z" fill="#2c2c2e"/><path d="M98 146 L72 200" stroke="#1e1e1e" stroke-width="6"/>` +
        head() +
        `<path d="M52 84 C46 52 64 40 84 41 C106 42 114 58 108 84 C104 70 98 62 90 60 C96 74 92 90 82 98 C86 84 84 70 76 62 C68 64 60 72 52 84Z" fill="#7a5530"/>` +
        `<path d="M76 62 C86 70 88 86 82 98 C78 90 76 78 72 70Z" fill="#8c6439"/>` +
        `<path d="M64 84 q6 -3 11 0" stroke="#1b1512" stroke-width="2.2" fill="none"/><ellipse cx="69.5" cy="87" rx="2.6" ry="2" fill="#1b1512"/>` +
        `<path d="M72 104 q8 1 16 -1" stroke="#6b4a3a" stroke-width="2" fill="none" stroke-linecap="round"/>`;
      break;
    case "torque":
      body =
        `<path d="M6 200 C8 150 34 128 80 126 C126 128 152 150 154 200Z" fill="${skin}"/>` +
        `<path d="M40 200 C42 160 56 136 80 134 C104 136 118 160 120 200Z" fill="#cfc8b8"/>` +
        `<path d="M58 134 C64 150 96 150 102 134" stroke="${skin}" stroke-width="10" fill="none"/>` +
        `<path d="M18 160 c6 -8 14 -6 16 2 c2 8 -6 14 -12 10 M128 156 c8 -6 16 0 14 8 c-2 8 -12 8 -14 2 M22 182 l12 -6 l-4 12 M134 178 l10 4 l-10 6" stroke="#2c3a48" stroke-width="2.4" fill="none" opacity=".8"/>` +
        `<path d="M60 176 l10 0 M75 176 l10 0 M90 176 l10 0" stroke="#7d7466" stroke-width="5" stroke-linecap="round" opacity=".55"/>` +
        `<rect x="66" y="104" width="28" height="26" fill="${skin}"/>` +
        `<ellipse cx="80" cy="80" rx="28" ry="31" fill="${skin}"/>` +
        `<path d="M54 74 C54 52 66 46 80 46 C94 46 106 52 106 74 C100 60 92 56 80 56 C68 56 60 60 54 74Z" fill="#5b3f2e" opacity=".45"/>` +
        `<path d="M52 80 a28 31 0 0 0 28 31 a24 31 0 0 1 -20 -31z" fill="#000" opacity=".14"/>` +
        `<path d="M62 76 l13 3 M98 76 l-13 3" stroke="#1b1512" stroke-width="3" stroke-linecap="round"/>` +
        `<ellipse cx="69" cy="84" rx="2.6" ry="2" fill="#1b1512"/><ellipse cx="91" cy="84" rx="2.6" ry="2" fill="#1b1512"/>` +
        `<path d="M78 86 l-3 12 h7" stroke="#00000045" stroke-width="1.6" fill="none"/>` +
        `<path d="M68 100 C70 112 90 112 92 100 C88 106 72 106 68 100Z" fill="#3a2a20"/><path d="M72 103 h16" stroke="#5a3a2a" stroke-width="2"/>` +
        `<path d="M96 64 l8 14" stroke="#8a4a3a" stroke-width="2" opacity=".7"/>`;
      break;
  }
  return `<svg viewBox="0 0 160 200" preserveAspectRatio="xMidYMid slice" xmlns="http://www.w3.org/2000/svg" role="img" aria-label="${(COMPANIONS.find(c => c.id === id) || {}).name || id} portrait">${frame}${body}${shade}</svg>`;
}
