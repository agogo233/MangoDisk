# Linux bundle compatibility

Build with `pnpm tauri:build:linux` on Ubuntu 22.04 for each supported Linux
architecture. Keep the glibc 2.35 baseline; moving the build to a newer Ubuntu
release can prevent the bundles from running on older supported systems.

The build excludes `libwayland-*.so*` through `LINUXDEPLOY_EXCLUDED_LIBRARIES` so
Mesa uses the host's matching Wayland libraries. Tauri CLI 2.12.1 also supplies
the updated linuxdeploy and GTK plugin. The project AppRun delegates to Tauri's
launcher and uses `usr/lib/gio/modules`, the GTK plugin's current layout, to
isolate bundled GLib from newer host GIO modules.

`scripts/test-linux-bundle.py` exercises library rejection and launcher behavior
before packaging. `scripts/check-linux-bundle.py` inspects the final DEB and
AppImage for architecture, glibc requirements, launcher permissions and content,
the GIO TLS module and cache, and any remaining Wayland libraries or symlinks.
Both checks run through the build script.

Validate the resulting AppImage on Ubuntu 22.04 and a newer Mesa/Wayland system.
Confirm the interface renders, WebKit processes remain alive, and HTTPS works.
Building and checking ELF dependencies alone does not verify those runtime paths.
