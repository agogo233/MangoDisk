# System-surface icons

`public/mangodisk.svg` is the approved vector source. Regenerate the compact
variants from the repository root with:

```sh
node scripts/generate-resident-icons.mjs
```

- `tray-template.png`: 36 px monochrome mask for macOS. Tauri/AppKit renders it
  at 18 points and supplies the menu-bar foreground color.
- `tray-color.png`: 64 px transparent color artwork for the system tray.
- `windows/icon.ico`: multiple native sizes for Windows taskbar, executable and
  installer icons, selected by `tauri.windows.conf.json`. The largest entry is
  first because Tauri embeds that entry for live windows; Windows can still
  choose from all sizes for executable and shortcut icons.

Color variants also use 15% horizontal optical compensation and about 1.5%
vertical padding to balance the narrow mango against square Windows icons.
The macOS template keeps its original proportions.

These variants remove the Dock tile and reduce source whitespace. The existing
macOS application/Dock icons remain separate. Do not manually edit the derived
PNG/ICO files; adjust the source or the documented framing in the generator.
