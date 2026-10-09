ASHEN MARINE - private test kit 7 ("opening the last locked drawer")
====================================================================

What this is
  Your Dark Souls III character with Space Marine 2's chainsword and bolt pistol sounds - and the right NAMES for the
  two test weapons, which is what kits 5 and 6 were about. It is still a private test kit, so it also writes logs that
  tell me how to make it better.

  Everything runs on your PC. Nothing is uploaded: you send me the files yourself at the end. Dark Souls III runs as an
  OFFLINE COPY of your save (your real save is backed up first and checked afterwards). Space Marine 2 and the
  Dark Souls III game files are only READ, never changed.

Where we are with the names (thank you for the logs of kit 6)
  Kit 6's helper worked on your game: while Dark Souls III started, it found the unlock keys for 6 of the game's 8 big
  archive files (Data1 to Data5 and DLC2) and the kit could read their tables of contents. The item text was not under
  the name it was looked for in any of those six. Two archives were left: Data0 (it turns out Data0 is not locked at
  all - it is a plain file, so no key could ever match it) and DLC1.
  Kit 7 does three things about it: it opens Data0 without a key; it takes over the keys kit 6 already collected (so
  you do NOT need to start the game once for that); and if the item text is still not found under its usual name, it
  now looks at the first few KB of every file in the archives to find the item text by what it contains (so a
  different file name does not matter). That search can take from a few seconds to a minute or two. Whatever happens,
  it writes a fuller report that lists what is in the archives. If the item text is found, the names should work the
  first time you press Prepare; if not, the report tells me where to look next.

How to do it (about 10 minutes)
  1. Unzip this whole folder into the SAME place as the kit 6 folder (e.g. both in Downloads), as a new folder next to
     it. Do not delete the kit 6 folder yet. Steam must be running and signed in; Dark Souls III must NOT be running.
  2. Double-click  Prepare-AshenMarine.bat  and wait for "Done". It does four things, one after the other:
       - makes the sound files from your Space Marine 2 (a minute or two),
       - makes the item-name file from your Dark Souls III. It first says "Took over the key cache of an older kit
         folder". It may say "Looking at the start of every file in the archives": that is the search described
         above, please let it run. LOOK AT THE LINES OF STEP 2: "wrote msg\ENGLISH\item.msgbnd.dcx" means the names
         are ready,
       - writes a report about where Dark Souls III keeps its files (no copies of them; it repeats the search above),
       - writes the model reports (a minute).
  3. Double-click  Play-AshenMarine.bat . Dark Souls III starts; the title bar reads
     "DARK SOULS III - Ashen Marine (offline copy)".  (A question about connecting online is expected: it is blocked
     on purpose.)  Load your normal character, then:
       a. Stand still (not in a menu) and press  F8  ONCE. Three test items go into your inventory (this kit folder has
          its own private copy of your save, so the items from kit 6 are not there; this is the offline copy only).
          Open the inventory - Weapons and Ammunition. WHAT ARE THEY CALLED? What do the descriptions say?
       b. Equip them as before (the first in your right hand, the second in your left hand, the bolts in a bolt slot),
          swing and fire for a minute: the sounds should work as in kit 6. F9 switches between sound sets A and B.
       c. Quit to the desktop from the in-game menu as usual.
  4. Double-click  Send-Logs.bat . It puts  AshenMarine-logs.zip  on your Desktop. Attach it to the chat and tell me:
       - what did step 2 of Prepare say (did it write the item text)?
       - what are the three items called in the inventory, and what do their descriptions say?
       - did the sounds and the game feel the same as kit 6?

What it records (you can read every file yourself)
  ashenmarine\ds3-prepare\ds3-report.txt       what was read from Dark Souls III's files for the names, and what was found
  ashenmarine\ds3-probe\ds3-report.txt         where Dark Souls III keeps the files the mod needs (file names and sizes only)
  ashenmarine\logs\harvest.txt                 what the helper looked for in the game's memory and what it found
                                               (key fingerprints and places only - never the keys themselves)
  ashenmarine\prepare-sm2\prepare-report.txt   what was read from Space Marine 2 and how it became sound files
  ashenmarine\probe-sm2-mesh\mesh-report.txt   how Space Marine 2 stores the chainsword / bolt pistol models
  ashenmarine\logs\sfx.txt, sfx-trace.csv      every sound that played, what was in your hands, stamina / buttons /
                                               ammunition (this is how I tune what counts as a swing or a shot)
  ashenmarine\logs\probe-ds3.txt               game build, parameter tables, the beeps
  ashenmarine\logs\hook.log, launcher.log      what the safety helper did (hook.log also says when the game loads the changed
                                               name file from the mod folder: "MOD FILE opened by the game")
  ashenmarine\mod\ashenmarine-msg.json         which text the name file was made from (hashes and names only)
  Send-Logs.bat also lists the NAMES and SIZES of the files in ashenmarine\cache (not their contents).
  Your Windows user name and Steam id are masked in the copies Send-Logs makes. Your character's NAME is not logged.
  The sound files, the name file and the cache stay on your PC (they are made from your copies of the games).

If something goes wrong
  - A message box appears: read it, tell me what it says, send the logs.
  - "Dark Souls III ran, but Ashen Marine's protection never started": Steam was probably not running. Start Steam,
    wait until it is ready, close Dark Souls III, press Play again.
  - Dark Souls III crashes: that is useful information - run Send-Logs.bat anyway and tell me roughly what you were
    doing (and whether it was in the first minute, when the helper is busy). Your real save was not changed (it is
    backed up and checked each time).
  - No sound at all: check that Prepare-AshenMarine.bat said it prepared the sounds, then look in
    ashenmarine\logs\sfx.txt (it says why sounds are off). Tell me what it says.
  - The weapons still have their old names: run Send-Logs.bat and tell me. The files ashenmarine\ds3-prepare\ds3-report.txt
    and ashenmarine\ds3-probe\ds3-report.txt say exactly what was found and what was not.
  - Windows Defender / antivirus complains: do NOT turn protection off. Tell me the exact message; the files are
    unsigned test builds. SHA-256 hashes are in the chat so you can check they are the ones I sent.
  - To remove everything: close the game and delete this folder. Backups of your real save are in ashenmarine\backups.

Where things are
  ashenmarine\assets\sounds        the sound files made from your Space Marine 2, set A (private; safe to delete)
  ashenmarine\assets\sounds-exact  set B (the game's own volumes and delays), if it could be made
  ashenmarine\cache                what the helper saved from the running game (keys for the archives); private, safe to delete
  ashenmarine\mod                  the name file the game loads instead of its own (made from your Dark Souls III)
  ashenmarine\save                 the private copy of your save (what the test plays; F8's items live only here)
  ashenmarine\backups              backups of your REAL save (original = first ever, session-* = newest three)
  ashenmarine\logs                 launcher.log, hook.log, harvest.txt, sfx files and probe files
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
