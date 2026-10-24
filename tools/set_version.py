"""Stamp pinshot/__init__.py with the release version: python tools/set_version.py 1.2.3"""
import pathlib
import re
import sys

ver = sys.argv[1].lstrip("v")
if not re.fullmatch(r"\d+\.\d+\.\d+", ver):
    sys.exit(f"invalid version {ver!r}: expected MAJOR.MINOR.PATCH")
p = pathlib.Path(__file__).resolve().parent.parent / "pinshot" / "__init__.py"
new, n = re.subn(r'__version__ = "[^"]*"', f'__version__ = "{ver}"', p.read_text(encoding="utf-8"))
if n != 1:
    sys.exit("could not find __version__ in pinshot/__init__.py")
p.write_text(new, encoding="utf-8")
print("version set to", ver)
