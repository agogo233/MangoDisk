# Windows Explorer integration

The desktop embeds the architecture-matched COM DLL and a signed sparse identity
package. Enabling the setting stages content-keyed resources beneath the current
user's local application data and registers only that user. Windows 11 uses
`IExplorerCommand` with package identity, including when a user chooses the
classic menu. Windows 10 uses the classic per-user Explorer command registration.
Registering just one mechanism prevents duplicate entries. Neither path requests
elevation.

The packaged COM surrogate starts the desktop with the user's ordinary Explorer
process as its explicit parent. The desktop and its children retain ordinary process identity,
so disabling or replacing the menu package cannot terminate the running desktop.

On Windows 11, the sparse package's external directory also needs an executable
entry point. The desktop hard-links its existing executable there on the same volume, or
copies it for a portable executable on a different volume. Windows 10 stages
only the DLL. The command launches the current executable recorded in the user registration. Moving a portable
executable requires running it again to refresh that location. Disabling removes
the current-user menu registration; Windows may retain a loaded DLL until the
last Explorer reference is released.

Windows builds require the MSVC toolchain and Windows SDK `MakeAppx.exe`.
Release builds additionally require:

- `MANGODISK_EXPLORER_PUBLISHER`: the signing certificate's exact subject;
- `MANGODISK_EXPLORER_SIGN_COMMAND`: a JSON argv array whose `{file}` argument is
  replaced with each DLL/package path, following the existing privileged-helper
  signing convention. Supply credentials through the signing service rather
  than committed configuration.

The identity package must be signed by a certificate trusted on the destination.
Ordinary registration does not install certificates, enable developer mode, or
change machine policy. Unsigned debug packages may compile but cannot enable the
Windows 11 integration on an ordinary machine. Development validation can use a
test certificate explicitly trusted on the test machine; some Windows builds
require the machine Trusted People store even for per-user package registration.

Run `command_tests.cpp` as a native test executable linked with `shell32`,
`shlwapi`, `ole32`, `advapi32`, `uuid`, and `user32`. It checks Windows argument round trips
and COM object ownership. Validate the real first-level menus on both Windows 10
and Windows 11, including a non-elevated token, cold/warm launches, settings
toggles, Unicode paths, and installed/portable locations.
