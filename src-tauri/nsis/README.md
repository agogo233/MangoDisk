# Windows installer hooks

Automatic updates preserve the selected directory when it contains
`MangoDisk.exe`. Legacy publisher registry records are consulted only when that
directory has no executable, checking the URL publisher before the original
publisher. Invalid legacy directories are skipped. Interactive installations
always keep the selected directory.

This prevents new redirects into historical directories. It cannot infer the
active copy when an earlier update has already left two installations and
changed the current publisher registry record. Repair that state with a full
installer targeting the directory used by the old shortcut, then verify the
shortcut and login entry before subsequent automatic updates.

Run the regression scenarios on Windows with NSIS 3 installed:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File src-tauri/nsis/test-installer-hooks.ps1 -MakeNsis 'C:\Program Files (x86)\NSIS\makensis.exe'
```

The script compiles and executes the production hook in a silent test installer.
It uses temporary directories and unique product registry keys, verifies both
the installation and output directories, and removes its fixtures afterward.
It does not install or launch MangoDisk. The full packaged upgrade and reboot
flow still requires separate application testing.
