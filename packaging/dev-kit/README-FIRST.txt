ASHEN MARINE - private test kit 4 ("swing it first")
====================================================

What this is
  The first version where you can HEAR the mashup: your Dark Souls III character swinging a sword with Space Marine 2's
  real chainsword sounds, and firing a "bolt pistol" with Space Marine 2's real bolt pistol shot. It is still a private
  test kit, so it also writes logs that tell me how to make it better.

  Everything runs on your PC. Nothing is uploaded: you send me the files yourself at the end. Dark Souls III runs as an
  OFFLINE COPY of your save (your real save is backed up first and checked afterwards). Space Marine 2 is only READ,
  never started and never changed.

  Step 1 - Prepare-AshenMarine.bat   reads your Space Marine 2 files and makes the sound files (a minute or two).
  Step 2 - Play-AshenMarine.bat      starts Dark Souls III as the offline copy, with the sounds switched on.
  Step 3 - Send-Logs.bat             collects the text logs into one zip on your Desktop.

How to do it (about 20 minutes)
  1. Unzip this whole folder anywhere (e.g. Desktop). Steam must be running and signed in; Dark Souls III must NOT be
     running.
  2. Double-click  Prepare-AshenMarine.bat  and wait for "Done".
     Then open the folder  ashenmarine\assets\sounds  and double-click a few of the .wav files to listen, for example
        chainsword_swing_1_1.wav   chainsword_idle_1.wav   boltpistol_fire_1.wav
     Do they sound like a chainsword swing, a chainsword engine idling and a bolt pistol shot? (Your answer helps me
     more than anything else in this test.)
  3. Double-click  Play-AshenMarine.bat . Dark Souls III starts; the title bar reads
     "DARK SOULS III - Ashen Marine (offline copy)".  (A question about connecting online is expected: it is blocked
     on purpose.)  Load your normal character, then:
       a. Swing your weapon four times in a row with the normal attack button (controller: RB, mouse: left button).
          Each swing should play a chainsword sound, and the sound should change from swing to swing.
          Then one strong attack (controller: RT, mouse: right button) - a different sound.
          Then roll twice: rolls should be SILENT.
       b. Press  F7  - a chainsword engine starts idling. Press F7 again to stop it.
          (F5 makes all the mashup's sounds a little quieter, F6 a little louder - press them until the volume
          feels right next to the game's own sound, and tell me how many presses it took.)
       c. Stand still (not in a menu) and press  F8  ONCE. This puts three test items in your inventory (in the
          offline copy only): a sword, a crossbow and 60 bolts, and renames them in memory to "Chainsword",
          "Bolt Pistol" and "Bolt Rounds". Open the inventory (Weapons / Ammunition) and look: what are they called?
       d. Equip the new sword in your right hand and swing it a few times.
       e. Equip the new crossbow in a hand slot and the bolts in a bolt slot, close the menu and shoot 5 times. Each
          shot should play the bolt pistol sound.
       f. Quit to the desktop from the in-game menu as usual.
  4. Double-click  Send-Logs.bat . It puts  AshenMarine-logs.zip  on your Desktop. Attach it to the chat and tell me:
       - did the .wav files sound right (step 2)?
       - which of the actions in step 3 made a sound, and did the sound fit?  Too loud? Too quiet? Late?
       - what were the three items called after F8?
       - did Dark Souls III stay stable?

What it records (you can read every file yourself)
  ashenmarine\prepare-sm2\prepare-report.txt   what was read from Space Marine 2 and how it became sound files
  ashenmarine\logs\sfx.txt, sfx-trace.csv      every sound that played, and a trace of stamina / buttons / ammunition
                                               (this is how I tune what counts as a swing)
  ashenmarine\logs\probe-ds3.txt               game build, how the game stores item names, the beeps
  ashenmarine\logs\probe-ds3-weapons.csv       the game's weapon table (ids and row names), probe-ds3-goods.csv likewise
  ashenmarine\logs\probe-ds3-inventory.csv     your inventory item ids (not your name)
  ashenmarine\logs\hook.log, launcher.log      what the safety helper did
  Your Windows user name and Steam id are masked in the copies Send-Logs makes. Your character's NAME is not logged.
  The sound files themselves stay on your PC (they are made from your copy of Space Marine 2).

If something goes wrong
  - A message box appears: read it, tell me what it says, send the logs.
  - "Dark Souls III ran, but Ashen Marine's protection never started": Steam was probably not running. Start Steam,
    wait until it is ready, close Dark Souls III, press Play again.
  - Dark Souls III crashes: that is useful information - run Send-Logs.bat anyway and tell me roughly what you were
    doing. Your real save was not changed (it is backed up and checked each time).
  - No sound at all: check that Prepare-AshenMarine.bat said it prepared the sounds, then look in
    ashenmarine\logs\sfx.txt (it says why sounds are off). Tell me what it says.
  - Windows Defender / antivirus complains: do NOT turn protection off. Tell me the exact message; the files are
    unsigned test builds. SHA-256 hashes are in the chat so you can check they are the ones I sent.
  - To remove everything: close the game and delete this folder. Backups of your real save are in ashenmarine\backups.

Where things are
  ashenmarine\assets\sounds   the sound files made from your Space Marine 2 (private; safe to delete)
  ashenmarine\save            the private copy of your save (what the test plays; F8's items live only here)
  ashenmarine\backups         backups of your REAL save (original = first ever, session-* = newest three)
  ashenmarine\logs            launcher.log, hook.log, sfx files and probe files
  modengine2\                 ModEngine2 2.1.0 (MIT license) - the loader Melty will install for you in the real release

Good to know
  - If it cannot find Dark Souls III by itself, put the game's folder (the one that contains Game\DarkSoulsIII.exe)
    on the first line of a new text file  ashenmarine\game-folder.txt  and press Play again.
  - If it cannot find Space Marine 2, put its folder on the first line of  ashenmarine\sm2-folder.txt .
  - The private save copy is a snapshot of your character taken the first time you press Play. To start a fresh
    copy of your current real character later, close the game and delete the  ashenmarine\save  folder.
  - Keyboard and mouse: the mouse buttons only count while the game window is in front. If your attack buttons are
    different from the defaults, tell me which ones - that is easy to change.
  - This kit never goes online, never launches Space Marine 2, and never modifies any game file.
  - It is an offline sandbox. Do not use it to play online.
