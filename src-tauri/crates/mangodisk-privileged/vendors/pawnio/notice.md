# Bundled PawnIO components

These are unmodified official vendor binaries embedded in the Windows
`MangoDiskPrivileged` helper. The Windows desktop executable embeds that signed
helper, so installed and single-file portable builds also carry these binaries.
macOS/Linux builds do not include them. Bundling does not install or enable them:
installation is an explicit user action requiring administrator approval.

| Component | Version | File | Bytes | SHA-256 |
| --- | --- | --- | --- | --- |
| PawnIO official installer | 2.2.0 | `2.2.0/PawnIO_setup.exe` | 3410960 | `1f519a22e47187f70a1379a48ca604981c4fcf694f4e65b734aaa74a9fba3032` |
| Intel MSR signed module | 0.2.11 | `modules/0.2.11/IntelMSR.bin` | 5324 | `d6ed85d65ab17a22f813ef98207d6d537155ee2ded5976a21cb48413c9b92e5f` |
| AMD Zen signed module | 0.2.11 | `modules/0.2.11/AMDFamily17.bin` | 10652 | `dae74615761b78bdf064dfb3e136252ddcc6fc727d88f14738d0e5800d427a91` |

Upstream author: namazso.

- Installer release: https://github.com/namazso/PawnIO.Setup/releases/tag/2.2.0
- Module release: https://github.com/namazso/PawnIO.Modules/releases/tag/0.2.11
- Module source: https://github.com/namazso/PawnIO.Modules/tree/0.2.11
- Integration and redistribution guidance:
  https://github.com/namazso/PawnIO.Modules/wiki/Using-PawnIO-Modules

The official installer is a proprietary distribution whose redistribution is
allowed by upstream. Its original bytes, signatures and embedded notices are
preserved. This does not relicense it under MangoDisk's license.

The modules are distributed under GNU LGPL 2.1. Their unmodified upstream license
is in `modules/0.2.11/COPYING`; the exact corresponding source is available at
the versioned module source link above. MangoDisk embeds the license text in both
installed and single-file portable builds. About > Third-party licenses displays
component attribution and the unchanged license offline, without external license
files. This engineering provenance record remains in the source repository.

When updating a vendor release, replace the original binary, its pinned digest
in `src/artifacts.rs`, size assertions in `src/vendor_payloads.rs`, provenance
and applicable notices together. Verify the installer's official Authenticode
signature and the module's compatibility with the signed PawnIO driver. Never
patch vendor binaries or substitute an unsigned driver/module.
