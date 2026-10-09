ASHEN MARINE - private test kit 6 ("the weapon names, for real this time")
===========================================================================

What this is
  Your Dark Souls III character with Space Marine 2's chainsword and bolt pistol sounds - and, since kit 5, the right
  NAMES for the two test weapons. It is still a private test kit, so it also writes logs that tell me how to make it
  better.

  Everything runs on your PC. Nothing is uploaded: you send me the files yourself at the end. Dark Souls III runs as an
  OFFLINE COPY of your save (your real save is backed up first and checked afterwards). Space Marine 2 and the
  Dark Souls III game files are only READ, never changed.

Why the names did not change in kit 5 (and what is new)
  Dark Souls III keeps its item names inside big encrypted archive files (Game\Data0.bhd ... DLC2.bhd). To change a
  name, the kit first has to READ the game's own text out of them, and the unlock keys are not in the game's program
  file. Kit 5 had no other way, so it could only say "cannot read".
  Kit 6 learns what it needs from the running game itself. While Dark Souls III starts, a small helper inside the game
  LOOKS (it only reads, it changes nothing) through the game's memory for the keys and tables the game has just used to
  open those archives, checks every find against your own files, and saves what is right into ashenmarine\cache. When
  you quit the game, the same window makes the item-name file from it. So the names appear from the SECOND time you
  press Play. (The first start of kit 6 may take a few seconds longer to load; the helper runs at low priority and stops
  by itself, at the latest after 15 minutes. If it finds everything, it stops within seconds.)
  It is a test: it may find everything, part of it, or nothing. The logs tell me which, and what to do next.

How to do it (about 25 minutes)
  1. Unzip this whole folder anywhere (e.g. Desktop). Steam must be running and signed in; Dark Souls III must NOT be
     running.
  2. Double-click  Prepare-AshenMarine.bat  and wait for "Done". It does three things, one after the other:
       - makes the sound files from your Space Marine 2 (a minute or two),
       - tries to make the item-name file from your Dark Souls III. THE FIRST TIME THIS SAYS IT CANNOT YET: that is
         expected (the game has not been started with kit 6 yet). Nothing is wrong, go on,
       - writes the model reports (a minute).
  3. Double-click  Play-AshenMarine.bat . Dark Souls III starts; the title bar reads
     "DARK SOULS III - Ashen Marine (offline copy)".  (A question about connecting online is expected: it is blocked
     on purpose.)  Load your normal character and play as in kit 5:
       a. Stand still (not in a menu) and press  F8  ONCE. Three test items go into your inventory (in the offline
          copy only). In THIS first session they may still carry the old names (Shortsword, Avelyn, Standard Bolt).
       b. Equip the first in your right hand, the second in your left hand, the bolts in a bolt slot.
       c. Swing four times (left mouse button; controller RB), one strong attack (Shift + left button; controller RT),
          roll twice (silent), fire the pistol five times (right mouse button; controller LB: three shots each).
          F9 switches between sound set A and set B while you do this. F7 idles a chainsword engine, F5 / F6 change the
          volume.
       d. Quit to the desktop from the in-game menu as usual.
  4. The Play window now says "Making the item names ..." and works for up to a minute. Read what it says:
       - "wrote msg\ENGLISH\item.msgbnd.dcx" = the names are ready. Press  Play-AshenMarine.bat  AGAIN and open the
         inventory: the items should now be called Chainsword, Bolt Pistol and Bolt Rounds, with new descriptions.
       - anything else = it could not. Do not worry; go on with step 5.
  5. Double-click  Send-Logs.bat . It puts  AshenMarine-logs.zip  on your Desktop. Attach it to the chat and tell me:
       - what did the Play window say at the end of step 4 (and, after the second Play, what are the three items called)?
       - did the sounds still work as before, and did the game feel the same as kit 5 (no new stutter at the start)?
       - set A or set B (F9)? (only if you have a favourite)

What it records (you can read every file yourself)
  ashenmarine\logs\harvest.txt                 what the helper looked for in the game's memory and what it found
                                               (key fingerprints and places only - never the keys themselves)
  ashenmarine\ds3-prepare\ds3-report.txt       what was read from Dark Souls III's files and what the name file contains
  ashenmarine\prepare-sm2\prepare-report.txt   what was read from Space Marine 2 and how it became sound files
  ashenmarine\probe-sm2-mesh\mesh-report.txt   how Space Marine 2 stores the chainsword / bolt pistol models
  ashenmarine\logs\sfx.txt, sfx-trace.csv      every sound that played, what was in your hands, stamina / buttons /
                                               ammunition (this is how I tune what counts as a swing or a shot)
  ashenmarine\logs\probe-ds3.txt               game build, parameter tables, the beeps
  ashenmarine\logs\hook.log, launcher.log      what the safety helper did
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
  - The weapons still have their old names after the second Play: run Send-Logs.bat and tell me. The files
    ashenmarine\logs\harvest.txt and ashenmarine\ds3-prepare\ds3-report.txt say exactly what was found and what was not.
    You can also run  Prepare-AshenMarine.bat  again at any time: it uses what the helper saved.
  - Windows Defender / antivirus complains: do NOT turn protection off. Tell me the exact message; the files are
    unsigned test builds. SHA-256 hashes are in the chat so you can check they are the ones I sent.
  - To remove everything: close the game and delete this folder. Backups of your real save are in ashenmarine\backups.

Where things are
  ashenmarine\assets\sounds        the sound files made from your Space Marine 2, set A (private; safe to delete)
  ashenmarine\assets\sounds-exact  set B (the game's own volumes and delays), if it could be made
  ashenmarine\cache                what the helper saved from the running game (keys, archive tables); private, safe to delete
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
