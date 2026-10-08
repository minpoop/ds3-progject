ASHEN MARINE - private test kit 5 ("names, a bolt pistol that holds three, and a look at the models")
==================================================================================================

What this is
  Your Dark Souls III character with Space Marine 2's chainsword and bolt pistol sounds - and, new in this kit, the
  right NAMES for the two test weapons. It is still a private test kit, so it also writes logs that tell me how to make
  it better.

  Everything runs on your PC. Nothing is uploaded: you send me the files yourself at the end. Dark Souls III runs as an
  OFFLINE COPY of your save (your real save is backed up first and checked afterwards). Space Marine 2 and the
  Dark Souls III game files are only READ, never started by this kit's tools and never changed.

What is new since kit 4
  1. The weapons get their real names. Prepare now also reads the item text out of Dark Souls III's own files and
     makes a copy of it with "Chainsword", "Bolt Pistol" and "Bolt Rounds" (and short descriptions). The game loads that
     copy instead of its own. (Nothing in the game's folder is changed.)
  2. The bolt pistol is now the Avelyn - the crossbow that sends THREE bolts with one pull. Each bolt plays the
     Space Marine 2 shot, a little apart. (Avelyn wants 16 STR / 14 DEX; with less it still works, just weaker.)
  3. Firing the crossbow no longer plays the sword sound. Sword sounds only play for a melee weapon in the hand you
     attacked with. Strong attack = Shift + left mouse button (or the right trigger on a controller).
  4. Two versions of the sounds. Prepare makes the ones you already know (set A) and, when it can read the game's
     sound bank properly now, a second set (set B) with the volumes and delays the game itself uses. Press F9 in the
     game to switch between them while you swing and fire - and tell me which you like better.
  5. A look at the model files (no change you can see yet). Prepare also writes a report about how Space Marine 2 stores
     the chainsword and bolt pistol models, and how Dark Souls III stores its weapon models. This is the first step
     toward showing the real Space Marine 2 weapons in your hands.

How to do it (about 25 minutes)
  1. Unzip this whole folder anywhere (e.g. Desktop). Steam must be running and signed in; Dark Souls III must NOT be
     running.
  2. Double-click  Prepare-AshenMarine.bat  and wait for "Done". It does three things, one after the other:
       - makes the sound files from your Space Marine 2 (a minute or two),
       - makes the item-name file from your Dark Souls III (about a minute),
       - writes the model reports (a minute).
     If the second part says it could not find something it needs, it tells you what to do (for example "start the
     game once with Play-AshenMarine.bat, close it, and run this again").
  3. Double-click  Play-AshenMarine.bat . Dark Souls III starts; the title bar reads
     "DARK SOULS III - Ashen Marine (offline copy)".  (A question about connecting online is expected: it is blocked
     on purpose.)  Load your normal character, then:
       a. Stand still (not in a menu) and press  F8  ONCE. Three test items go into your inventory (in the offline
          copy only): the Chainsword (a Shortsword inside), the Bolt Pistol (an Avelyn inside) and 60 Bolt Rounds.
          Open the inventory - Weapons and Ammunition. WHAT ARE THEY CALLED? What does the description say?
       b. Equip the Chainsword in your right hand, the Bolt Pistol in your left hand, the Bolt Rounds in a bolt slot.
          Close the menu.
       c. Swing the sword four times with the left mouse button (controller: RB). Each swing should sound like a
          chainsword, a different one each time. Then one strong attack (Shift + left button, controller: RT).
          Roll twice: rolls stay silent.
       d. Fire the pistol (right mouse button, controller: LB) five times. Each pull should play THREE shots and
          NO sword sound. Do you hear three? Does it feel right?
       e. While swinging and firing, press  F9  a few times. The game says nothing on screen, but you hear a
          swing from the set you just switched to. Which set sounds better, A (first) or B (second)?
       f. F7 starts and stops a chainsword engine idling. F5 / F6 make everything a little quieter / louder.
       g. Quit to the desktop from the in-game menu as usual.
  4. Double-click  Send-Logs.bat . It puts  AshenMarine-logs.zip  on your Desktop. Attach it to the chat and tell me:
       - what were the three items called, and the descriptions?
       - did the pistol play three shots per pull, and was the sword quiet while you fired?
       - set A or set B (F9)?
       - did Dark Souls III stay stable?

What it records (you can read every file yourself)
  ashenmarine\prepare-sm2\prepare-report.txt   what was read from Space Marine 2 and how it became sound files
  ashenmarine\ds3-prepare\ds3-report.txt       what was read from Dark Souls III's files and what the name file contains
                                               (key fingerprints only, no keys)
  ashenmarine\probe-sm2-mesh\mesh-report.txt   how Space Marine 2 stores the chainsword / bolt pistol models
  ashenmarine\logs\sfx.txt, sfx-trace.csv      every sound that played, what was in your hands, stamina / buttons /
                                               ammunition (this is how I tune what counts as a swing or a shot)
  ashenmarine\logs\probe-ds3.txt               game build, parameter tables, the beeps, the archive-key scan
  ashenmarine\logs\hook.log, launcher.log      what the safety helper did
  ashenmarine\mod\ashenmarine-msg.json         which text the name file was made from (hashes and names only)
  Your Windows user name and Steam id are masked in the copies Send-Logs makes. Your character's NAME is not logged.
  The sound files and the name file themselves stay on your PC (they are made from your copies of the games).

If something goes wrong
  - A message box appears: read it, tell me what it says, send the logs.
  - "Dark Souls III ran, but Ashen Marine's protection never started": Steam was probably not running. Start Steam,
    wait until it is ready, close Dark Souls III, press Play again.
  - Dark Souls III crashes: that is useful information - run Send-Logs.bat anyway and tell me roughly what you were
    doing. Your real save was not changed (it is backed up and checked each time).
  - No sound at all: check that Prepare-AshenMarine.bat said it prepared the sounds, then look in
    ashenmarine\logs\sfx.txt (it says why sounds are off). Tell me what it says.
  - The weapons still have their old names: run Send-Logs.bat and tell me; ashenmarine\ds3-prepare\ds3-report.txt says
    exactly what the name step found or could not find. If it says the keys are missing, start Play once, quit, and run
    Prepare again.
  - Windows Defender / antivirus complains: do NOT turn protection off. Tell me the exact message; the files are
    unsigned test builds. SHA-256 hashes are in the chat so you can check they are the ones I sent.
  - To remove everything: close the game and delete this folder. Backups of your real save are in ashenmarine\backups.

Where things are
  ashenmarine\assets\sounds        the sound files made from your Space Marine 2, set A (private; safe to delete)
  ashenmarine\assets\sounds-exact  set B (the game's own volumes and delays), if it could be made
  ashenmarine\mod                  the name file the game loads instead of its own (made from your Dark Souls III)
  ashenmarine\save                 the private copy of your save (what the test plays; F8's items live only here)
  ashenmarine\backups              backups of your REAL save (original = first ever, session-* = newest three)
  ashenmarine\logs                 launcher.log, hook.log, sfx files and probe files
  modengine2\                      ModEngine2 2.1.0 (MIT license) - the loader Melty will install for you in the real release

Good to know
  - If it cannot find Dark Souls III by itself, put the game's folder (the one that contains Game\DarkSoulsIII.exe)
    on the first line of a new text file  ashenmarine\game-folder.txt  and run it again.
  - If it cannot find Space Marine 2, put its folder on the first line of  ashenmarine\sm2-folder.txt .
  - The private save copy is a snapshot of your character taken the first time you press Play. To start a fresh
    copy of your current real character later, close the game and delete the  ashenmarine\save  folder.
  - Keyboard and mouse: the mouse buttons only count while the game window is in front. The sounds assume the game's
    default layout (left button = right-hand attack, Shift + left button = strong attack, right button = left-hand
    weapon). If yours is different, tell me which buttons - that is easy to change.
  - This kit never goes online and never modifies any game file. The only thing it adds to the game's world is the
    name file in ashenmarine\mod, which Dark Souls III loads through ModEngine2.
  - It is an offline sandbox. Do not use it to play online.
