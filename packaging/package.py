"""Packaging for the Rust edition.

  python packaging/package.py stamp v2026.10.0            set the workspace version (release builds)
  python packaging/package.py pack --arch amd64       turn target/release into ./release/* artifacts

`pack` expects `cargo build --release -p pinshot` to have run, and `imageio-ffmpeg` (pip) for the bundled ffmpeg.
Windows: zip + Inno Setup installer; macOS: ad-hoc signed .app in a dmg; Linux: .deb + .tar.gz.
"""
import argparse
import os
import re
import shutil
import subprocess
import sys
import tarfile
import textwrap
import time

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))
OUT = os.path.join(ROOT, "release")
STAGE = os.path.join(ROOT, "build", "rust-stage")
EXE = "pinshot.exe" if sys.platform == "win32" else "pinshot"


def run(*cmd, **kw):
    print("+", " ".join(map(str, cmd)), flush=True)
    subprocess.run(cmd, check=True, **kw)


def version() -> str:
    text = open(os.path.join(ROOT, "Cargo.toml"), encoding="utf-8").read()
    m = re.search(r'^version = "([^"]+)"', text, re.M)
    if not m:
        sys.exit("no version in Cargo.toml")
    return m.group(1)


def stamp(tag: str) -> None:
    ver = tag.lstrip("v")
    m = re.fullmatch(r"(\d{4})\.(\d{1,2})\.(\d+)", ver)
    if not m or not 1 <= int(m.group(2)) <= 12:
        sys.exit(f"invalid version {tag!r}: expected YYYY.M.N (calendar version, e.g. v2026.10.0)")
    path = os.path.join(ROOT, "Cargo.toml")
    text = open(path, encoding="utf-8").read()
    new, n = re.subn(r'^version = "[^"]+"', f'version = "{ver}"', text, count=1, flags=re.M)
    if n != 1:
        sys.exit("could not stamp Cargo.toml")
    open(path, "w", encoding="utf-8").write(new)
    print("version set to", ver)


def ffmpeg_exe() -> str:
    import imageio_ffmpeg  # pip install imageio-ffmpeg

    return imageio_ffmpeg.get_ffmpeg_exe()


def program_dir() -> str:
    """Folder with the program and the bundled ffmpeg, side by side."""
    d = os.path.join(STAGE, "PinShot")
    shutil.rmtree(STAGE, ignore_errors=True)
    os.makedirs(d)
    shutil.copy(os.path.join(ROOT, "target", "release", EXE), os.path.join(d, EXE))
    ff = os.path.join(d, "ffmpeg.exe" if sys.platform == "win32" else "ffmpeg")
    shutil.copy(ffmpeg_exe(), ff)
    os.chmod(ff, 0o755)
    os.chmod(os.path.join(d, EXE), 0o755)
    return d


def windows(arch: str) -> None:
    ver = version()
    d = program_dir()
    os.replace(os.path.join(d, EXE), os.path.join(d, "PinShot.exe"))
    base = f"PinShot-{ver}-windows-{arch}"
    shutil.make_archive(os.path.join(OUT, base), "zip", STAGE, "PinShot")
    iscc = shutil.which("iscc") or shutil.which("ISCC")
    for cand in (r"C:\Program Files (x86)\Inno Setup 6\ISCC.exe", r"C:\Program Files\Inno Setup 6\ISCC.exe"):
        iscc = iscc or (cand if os.path.exists(cand) else None)
    if not iscc:
        print("Inno Setup not found: only the portable zip is produced")
        return
    try:
        run(iscc, f"/DAppVersion={ver}", f"/DArch={arch}", f"/DSourceDir={d}", f"/DOutputDir={OUT}",
            f"/DOutputBase={base}-setup", f"/DIconFile={os.path.join(ROOT, 'assets', 'icon.ico')}",
            os.path.join(ROOT, "packaging", "pinshot.iss"))
    except subprocess.CalledProcessError as ex:
        print(f"WARNING: installer build failed ({ex}); shipping the portable zip only")


PLIST = """<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleName</key><string>PinShot</string>
<key>CFBundleDisplayName</key><string>PinShot</string>
<key>CFBundleIdentifier</key><string>com.nuricanozturk.pinshot</string>
<key>CFBundleExecutable</key><string>pinshot</string>
<key>CFBundleIconFile</key><string>icon.icns</string>
<key>CFBundleVersion</key><string>{ver}</string>
<key>CFBundleShortVersionString</key><string>{ver}</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>LSUIElement</key><true/>
<key>NSHighResolutionCapable</key><true/>
<key>NSMicrophoneUsageDescription</key><string>PinShot records the microphone in screen recordings.</string>
<key>NSScreenCaptureUsageDescription</key><string>PinShot captures and records the screen.</string>
</dict></plist>
"""


def macos(arch: str) -> None:
    ver = version()
    shutil.rmtree(STAGE, ignore_errors=True)
    app = os.path.join(STAGE, "dmg", "PinShot.app")
    macos_dir, res = os.path.join(app, "Contents", "MacOS"), os.path.join(app, "Contents", "Resources")
    os.makedirs(macos_dir)
    os.makedirs(res)
    shutil.copy(os.path.join(ROOT, "target", "release", "pinshot"), os.path.join(macos_dir, "pinshot"))
    shutil.copy(ffmpeg_exe(), os.path.join(res, "ffmpeg"))
    helper = os.path.join(ROOT, "packaging", "bin", "pinshot-audiotap")
    if os.path.isfile(helper):
        shutil.copy(helper, os.path.join(res, "pinshot-audiotap"))
    else:
        print("WARNING: pinshot-audiotap not built: no system sound on macOS")
    shutil.copy(os.path.join(ROOT, "assets", "icon.icns"), os.path.join(res, "icon.icns"))
    open(os.path.join(app, "Contents", "Info.plist"), "w", encoding="utf-8").write(PLIST.format(ver=ver))
    for p in (os.path.join(macos_dir, "pinshot"), os.path.join(res, "ffmpeg"), os.path.join(res, "pinshot-audiotap")):
        if os.path.isfile(p):
            os.chmod(p, 0o755)
            run("codesign", "--force", "--sign", "-", p)  # unsigned executables are killed on Apple Silicon
    run("codesign", "--force", "--deep", "--sign", "-", app)
    stage = os.path.join(STAGE, "dmg")
    os.symlink("/Applications", os.path.join(stage, "Applications"))
    dmg = os.path.join(OUT, f"PinShot-{ver}-macos-{arch}.dmg")
    for attempt in range(1, 4):  # hdiutil is occasionally "resource busy" on CI
        try:
            run("hdiutil", "create", "-volname", "PinShot", "-srcfolder", stage, "-ov", "-format", "UDZO", dmg)
            break
        except subprocess.CalledProcessError:
            if attempt == 3:
                raise
            time.sleep(5)


DEB_DEPENDS = "libc6, libxkbcommon0, libx11-6, libxkbcommon-x11-0, libxcb1, libdbus-1-3"


def linux(arch: str) -> None:
    ver = version()
    d = program_dir()
    base = f"PinShot-{ver}-linux-{arch}"
    shutil.copy(os.path.join(ROOT, "assets", "icon.png"), os.path.join(d, "pinshot.png"))
    with tarfile.open(os.path.join(OUT, base + ".tar.gz"), "w:gz") as t:
        t.add(d, arcname="PinShot")
    root = os.path.join(STAGE, "deb")
    for sub in ("DEBIAN", "opt", "usr/bin", "usr/share/applications", "usr/share/icons/hicolor/512x512/apps"):
        os.makedirs(os.path.join(root, sub))
    shutil.copytree(d, os.path.join(root, "opt", "pinshot"))
    os.symlink("/opt/pinshot/pinshot", os.path.join(root, "usr", "bin", "pinshot"))
    shutil.copy(os.path.join(ROOT, "assets", "icon.png"),
                os.path.join(root, "usr/share/icons/hicolor/512x512/apps/pinshot.png"))
    with open(os.path.join(root, "usr/share/applications/pinshot.desktop"), "w") as f:
        f.write(textwrap.dedent("""\
            [Desktop Entry]
            Type=Application
            Name=PinShot
            Comment=Capture, annotate, pin, OCR and record your screen
            Exec=pinshot
            Icon=pinshot
            Categories=Graphics;Utility;
            Terminal=false
            """))
    size_kb = sum(os.lstat(os.path.join(dp, fn)).st_size for dp, _, fns in os.walk(root) for fn in fns) // 1024
    with open(os.path.join(root, "DEBIAN", "control"), "w") as f:
        f.write(textwrap.dedent(f"""\
            Package: pinshot
            Version: {ver}
            Section: graphics
            Priority: optional
            Architecture: {arch}
            Installed-Size: {size_kb}
            Depends: {DEB_DEPENDS}
            Recommends: xdg-desktop-portal, pulseaudio-utils | pipewire-bin
            Maintainer: Nuri Can OZTURK <noreply@example.com>
            Description: Screenshot, annotate, OCR, pin and screen-record tool
             Region capture with annotation tools, offline OCR, always-on-top pinned images
             and MP4/GIF screen recording.
            """))
    run("chmod", "-R", "u=rwX,go=rX", root)  # Debian policy: 755 dirs / 644 files
    for exe in ("pinshot", "ffmpeg"):
        os.chmod(os.path.join(root, "opt", "pinshot", exe), 0o755)
    run("dpkg-deb", "--build", "--root-owner-group", root, os.path.join(OUT, base + ".deb"))


def main() -> None:
    ap = argparse.ArgumentParser()
    sub = ap.add_subparsers(dest="cmd", required=True)
    sub.add_parser("stamp").add_argument("tag")
    p = sub.add_parser("pack")
    p.add_argument("--arch", required=True, choices=["amd64", "arm64"])
    args = ap.parse_args()
    if args.cmd == "stamp":
        return stamp(args.tag)
    os.makedirs(OUT, exist_ok=True)
    {"win32": windows, "darwin": macos}.get(sys.platform, linux)(args.arch)
    print("\n".join(sorted(os.listdir(OUT))))


if __name__ == "__main__":
    main()
