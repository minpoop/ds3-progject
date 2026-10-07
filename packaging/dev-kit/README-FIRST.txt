ASHEN MARINE - private test kit (milestone 1: the safe sandbox)
================================================================

What this is
  A first test of the SAFETY part of the mashup, before any Space Marine content is added. It starts Dark Souls III
  as an OFFLINE COPY of your game save and proves your real save and online account are never touched.
  Nothing from Space Marine 2 is in it yet. You should just see Dark Souls III running normally, with
  " - Ashen Marine (offline copy)" added to the window title.

What it does, in order (all logged)
  1. Refuses to start if Dark Souls III is already running.
  2. Backs up your REAL save (a permanent first-ever backup + the 3 newest session backups) and fingerprints it.
  3. Makes its own private COPY of your save (first run only; your character is carried over).
  4. Starts Dark Souls III through ModEngine2 with a small helper (ashenmarine_hook.dll) that
       - sends every save-file access to the private copy,
       - refuses every network connection except to this PC itself (so the game stays offline),
       - proves both of those work inside the game BEFORE the game can touch anything, and closes the game if not.
  5. When the game closes, checks your real save is byte-identical to before, and puts it back from the backup if not.

How to test (about 5 minutes)
  1. Make sure Steam is running and Dark Souls III is NOT running. Unzip this whole folder anywhere (e.g. Desktop).
  2. Double-click  Play-AshenMarine.bat
  3. Dark Souls III starts. The title bar should read "DARK SOULS III - Ashen Marine (offline copy)" (a screenshot of
     it helps). Load your character, walk around for a minute, then quit from the in-game menu as usual.
     (If it asks about connecting online, that is expected: it is blocked on purpose.)
  4. Double-click  Send-Logs.bat  - it puts AshenMarine-logs.zip on your Desktop. Attach it to the chat and tell me:
       - did the game start, and did it look and feel normal (load times, menus, anything odd)?
       - could you see your character, and was your progress as expected?

If something goes wrong
  - A message box appears: read it, tell me what it says, send the logs.
  - Windows Defender / antivirus complains: do NOT turn protection off. Tell me the exact message; the files are
    unsigned test builds. SHA-256 hashes are in the chat so you can check they are the ones I sent.
  - To remove everything: close the game and delete this folder. Your real save was never changed, and the
    backups are in  ashenmarine\backups  if you ever want them.

Where things are
  ashenmarine\save       the private copy of your save (what the mashup plays)
  ashenmarine\backups    backups of your REAL save (original = first ever, session-* = newest three)
  ashenmarine\logs       launcher.log and hook.log (what happened, in plain text)
  modengine2\            ModEngine2 2.1.0 (MIT license) - the loader Melty will install for you in the real release

Good to know
  - The private copy is a snapshot of your character taken the first time you press Play. If you later want a fresh
    copy of your current real character, close the game and delete the  ashenmarine\save  folder.
  - If it cannot find Dark Souls III by itself, put the game's folder (the one that contains Game\DarkSoulsIII.exe)
    on the first line of a new text file  ashenmarine\game-folder.txt  and press Play again.

Safety notes
  - This kit never goes online, never touches Space Marine 2, and never modifies Dark Souls III's own files.
  - It is an offline sandbox. Do not use it to play online.
