"""Turn PyInstaller output (dist/) into release artifacts: zip/exe (Windows), dmg (macOS), deb+tar.gz (Linux).

usage: python packaging/package.py --arch amd64|arm64      -> files in ./release/
"""
import argparse
import os
import platform
import shutil
import subprocess
import sys
import tarfile
import textwrap
import time

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))
sys.path.insert(0, ROOT)
from pinshot import __version__  # noqa: E402

DIST = os.path.join(ROOT, "dist")
OUT = os.path.join(ROOT, "release")


def run(*cmd, **kw):
    print("+", " ".join(map(str, cmd)), flush=True)
    subprocess.run(cmd, check=True, **kw)


def windows(arch: str) -> None:
    src = os.path.join(DIST, "PinShot")
    base = f"PinShot-{__version__}-windows-{arch}"
    shutil.make_archive(os.path.join(OUT, base), "zip", DIST, "PinShot")       # portable build
    iscc = shutil.which("iscc") or shutil.which("ISCC")
    for cand in (r"C:\Program Files (x86)\Inno Setup 6\ISCC.exe", r"C:\Program Files\Inno Setup 6\ISCC.exe"):
        iscc = iscc or (cand if os.path.exists(cand) else None)
    if iscc:
        try:
            run(iscc, f"/DAppVersion={__version__}", f"/DArch={arch}", f"/DSourceDir={src}",
                f"/DOutputDir={OUT}", f"/DOutputBase={base}-setup",
                f"/DIconFile={os.path.join(ROOT, 'assets', 'icon.ico')}", os.path.join(ROOT, "packaging", "pinshot.iss"))
        except subprocess.CalledProcessError as ex:     # the portable zip is still a complete deliverable
            print(f"WARNING: installer build failed ({ex}); shipping the portable zip only")
    else:
        print("Inno Setup not found: only the portable zip is produced")


def macos(arch: str) -> None:
    app = os.path.join(DIST, "PinShot.app")
    # Ad-hoc sign (required for arm64 to launch). Not notarised: first launch needs right-click > Open.
    run("codesign", "--force", "--deep", "--sign", "-", app)
    stage = os.path.join(ROOT, "build", "dmg")
    shutil.rmtree(stage, ignore_errors=True)
    os.makedirs(stage)
    shutil.copytree(app, os.path.join(stage, "PinShot.app"), symlinks=True)
    os.symlink("/Applications", os.path.join(stage, "Applications"))
    dmg = os.path.join(OUT, f"PinShot-{__version__}-macos-{arch}.dmg")
    for attempt in range(1, 4):                         # hdiutil is occasionally "resource busy" on CI
        try:
            run("hdiutil", "create", "-volname", "PinShot", "-srcfolder", stage, "-ov", "-format", "UDZO", dmg)
            break
        except subprocess.CalledProcessError:
            if attempt == 3:
                raise
            time.sleep(5)


DEB_DEPENDS = ("libc6, libgl1, libegl1, libfontconfig1, libdbus-1-3, libxkbcommon0, libxkbcommon-x11-0, "
               "libxcb-cursor0, libxcb-icccm4, libxcb-image0, libxcb-keysyms1, libxcb-randr0, "
               "libxcb-render-util0, libxcb-shape0, libxcb-xkb1")


def linux(arch: str) -> None:
    src = os.path.join(DIST, "PinShot")
    base = f"PinShot-{__version__}-linux-{arch}"
    with tarfile.open(os.path.join(OUT, base + ".tar.gz"), "w:gz") as t:
        t.add(src, arcname="PinShot")
    root = os.path.join(ROOT, "build", "deb")
    shutil.rmtree(root, ignore_errors=True)
    for d in ("DEBIAN", "opt", "usr/bin", "usr/share/applications", "usr/share/icons/hicolor/512x512/apps"):
        os.makedirs(os.path.join(root, d))
    shutil.copytree(src, os.path.join(root, "opt", "pinshot"), symlinks=True)
    os.symlink("/opt/pinshot/PinShot", os.path.join(root, "usr", "bin", "pinshot"))
    shutil.copy(os.path.join(ROOT, "assets", "icon.png"),
                os.path.join(root, "usr/share/icons/hicolor/512x512/apps/pinshot.png"))
    with open(os.path.join(root, "usr/share/applications/pinshot.desktop"), "w") as f:
        f.write(textwrap.dedent("""\
            [Desktop Entry]
            Type=Application
            Name=PinShot
            Comment=Screenshot, annotate, OCR and pin to screen
            Exec=pinshot
            Icon=pinshot
            Categories=Graphics;Utility;
            Terminal=false
            StartupWMClass=PinShot
            """))
    size_kb = sum(os.lstat(os.path.join(dp, fn)).st_size for dp, _, fns in os.walk(root) for fn in fns) // 1024
    with open(os.path.join(root, "DEBIAN", "control"), "w") as f:
        f.write(textwrap.dedent(f"""\
            Package: pinshot
            Version: {__version__}
            Section: graphics
            Priority: optional
            Architecture: {arch}
            Installed-Size: {size_kb}
            Depends: {DEB_DEPENDS}
            Maintainer: PinShot <noreply@example.com>
            Description: Screenshot, annotate, OCR and pin-to-screen tool
             Region capture with annotation tools, offline OCR and always-on-top pinned images.
            """))
    run("chmod", "-R", "u=rwX,go=rX", root)               # Debian policy: 755 dirs / 644 files
    os.chmod(os.path.join(root, "opt", "pinshot", "PinShot"), 0o755)
    run("dpkg-deb", "--build", "--root-owner-group", root, os.path.join(OUT, base + ".deb"))


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--arch", required=True, choices=["amd64", "arm64"])
    args = ap.parse_args()
    os.makedirs(OUT, exist_ok=True)
    {"win32": windows, "darwin": macos}.get(sys.platform, linux)(args.arch)
    print("\n".join(sorted(os.listdir(OUT))))


if __name__ == "__main__":
    main()
