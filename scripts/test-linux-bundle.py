#!/usr/bin/env python3
"""Exercise AppImage library checks without requiring a Linux build."""

from pathlib import Path
import runpy
import shutil
import subprocess
import tempfile
import unittest


SCRIPTS_DIR = Path(__file__).resolve().parent
check_appimage_libraries = runpy.run_path(str(SCRIPTS_DIR / "check-linux-bundle.py"))[
    "check_appimage_libraries"
]


class AppImageLibraryTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory(prefix="mangodisk bundle ")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.modules = self.root / "usr/lib/gio/modules"
        self.modules.mkdir(parents=True)
        for name in ("libgiognutls.so", "giomodule.cache"):
            (self.modules / name).touch()

    def test_accepts_gio_modules_without_bundled_wayland(self) -> None:
        (self.root / "usr/lib/libwebkit2gtk-4.1.so.0").touch()
        check_appimage_libraries(self.root)

    def test_rejects_each_wayland_library_and_versioned_copy(self) -> None:
        for name in (
            "libwayland-client.so.0",
            "libwayland-cursor.so.0",
            "libwayland-egl.so.1",
            "libwayland-server.so.0",
            "libwayland-client.so.0.24.0",
        ):
            with self.subTest(name=name):
                library = self.root / "usr/lib" / name
                library.touch()
                with self.assertRaisesRegex(RuntimeError, name):
                    check_appimage_libraries(self.root)
                library.unlink()

    def test_rejects_nested_broken_wayland_symlink(self) -> None:
        library = self.root / "usr/lib/aarch64-linux-gnu/libwayland-client.so.0"
        library.parent.mkdir()
        library.symlink_to("missing-library")
        with self.assertRaisesRegex(RuntimeError, "aarch64-linux-gnu/libwayland-client"):
            check_appimage_libraries(self.root)

    def test_rejects_old_gio_module_layout(self) -> None:
        old_modules = self.root / "usr/lib/aarch64-linux-gnu/gio/modules"
        old_modules.parent.mkdir(parents=True)
        self.modules.rename(old_modules)
        with self.assertRaisesRegex(RuntimeError, "usr/lib/gio/modules/libgiognutls"):
            check_appimage_libraries(self.root)

    def test_rejects_missing_tls_module_or_cache(self) -> None:
        for name in ("libgiognutls.so", "giomodule.cache"):
            with self.subTest(name=name):
                module = self.modules / name
                module.unlink()
                with self.assertRaisesRegex(RuntimeError, name):
                    check_appimage_libraries(self.root)
                module.touch()

    def test_launcher_sets_gio_path_and_preserves_arguments_and_exit_code(self) -> None:
        launcher = self.root / "AppRun"
        shutil.copyfile(SCRIPTS_DIR / "appimage-apprun.sh", launcher)
        delegate = self.root / "AppRun.tauri"
        delegate.write_text(
            '#!/usr/bin/env bash\nprintf "%s\\n" "$GIO_MODULE_DIR" "$@"\nexit 23\n'
        )
        delegate.chmod(0o755)
        result = subprocess.run(
            ["bash", str(launcher), "argument with spaces", "--flag"],
            capture_output=True,
            text=True,
        )
        self.assertEqual(result.returncode, 23, result.stderr)
        self.assertEqual(
            result.stdout.splitlines(),
            [str(self.modules), "argument with spaces", "--flag"],
        )


if __name__ == "__main__":
    unittest.main()
