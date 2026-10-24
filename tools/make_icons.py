"""Generate assets/icon.png, icon.ico and icon.icns from the vector app icon (needs PySide6 + Pillow)."""
import os
import sys

os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")
ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, ROOT)

from PIL import Image  # noqa: E402
from PySide6.QtWidgets import QApplication  # noqa: E402

from pinshot.icons import app_pixmap  # noqa: E402

app = QApplication([])
out = os.path.join(ROOT, "assets")
os.makedirs(out, exist_ok=True)
png = os.path.join(out, "icon.png")
app_pixmap(1024).save(png, "PNG")
img = Image.open(png).convert("RGBA")
img.save(os.path.join(out, "icon.ico"), sizes=[(s, s) for s in (16, 24, 32, 48, 64, 128, 256)])
img.save(os.path.join(out, "icon.icns"))
print("icons written to", out)
