#!/usr/bin/env python3
"""
Generate all Tauri app icons from source image.
- Remove watermark (bottom-right "豆包AI生成")
- Resize to all required PNG sizes
- Generate icon.ico (Windows multi-size)
- Generate icon.icns (macOS)
"""

import os
import struct
from PIL import Image

SOURCE = r"E:\MyWork\gnome-remote\logo\设计系统桌面UI logo (1).png"
OUT_DIR = r"E:\MyWork\gnome-remote\src-tauri\icons"

# Backup existing icons
BACKUP_DIR = OUT_DIR + "_backup"
os.makedirs(BACKUP_DIR, exist_ok=True)

# --- Step 1: Backup existing icons ---
for f in os.listdir(OUT_DIR):
    src = os.path.join(OUT_DIR, f)
    dst = os.path.join(BACKUP_DIR, f)
    if os.path.isfile(src):
        with open(src, 'rb') as sf:
            with open(dst, 'wb') as df:
                df.write(sf.read())
        print(f"Backed up: {f}")

# --- Step 2: Load source, remove watermark ---
img = Image.open(SOURCE).convert("RGBA")
w, h = img.size

# Watermark bounding box (pre-computed): (1543, 1743) to (2021, 2020)
# Fill with white
for y in range(1743, 2021):
    for x in range(1543, 2021):
        img.putpixel((x, y), (255, 255, 255, 255))

print(f"\nWatermark removed from ({1543},{1743}) to ({2021},{2020})")

# Save clean source
clean_path = r"E:\MyWork\gnome-remote\logo\clean_logo.png"
img.save(clean_path, "PNG")
print(f"Clean logo saved to: {clean_path}")

# --- Step 3: Generate PNG icons ---
PNG_SIZES = {
    "32x32.png": 32,
    "128x128.png": 128,
    "128x128@2x.png": 256,
    "icon.png": 512,
    "Square30x30Logo.png": 30,
    "Square44x44Logo.png": 44,
    "Square71x71Logo.png": 71,
    "Square89x89Logo.png": 89,
    "Square107x107Logo.png": 107,
    "Square142x142Logo.png": 142,
    "Square150x150Logo.png": 150,
    "Square284x284Logo.png": 284,
    "Square310x310Logo.png": 310,
    "StoreLogo.png": 50,
}

print("\n--- Generating PNG icons ---")
for filename, size in sorted(PNG_SIZES.items()):
    out_path = os.path.join(OUT_DIR, filename)
    resized = img.resize((size, size), Image.LANCZOS)
    resized.save(out_path, "PNG")
    print(f"  {filename}: {size}x{size}")

# --- Step 4: Generate icon.ico (multi-size: 16, 24, 32, 48, 64, 128, 256) ---
print("\n--- Generating icon.ico ---")
ico_sizes = [16, 24, 32, 48, 64, 128, 256]
ico_images = []
for s in ico_sizes:
    ico_img = img.resize((s, s), Image.LANCZOS)
    ico_images.append(ico_img)

ico_path = os.path.join(OUT_DIR, "icon.ico")
ico_images[0].save(
    ico_path,
    format="ICO",
    sizes=[(s, s) for s in ico_sizes],
    append_images=ico_images[1:]
)
print(f"  icon.ico: {ico_sizes}")

# --- Step 5: Generate icon.icns ---
# macOS ICNS format
print("\n--- Generating icon.icns ---")

def rgba_to_premultiplied(img):
    """Convert RGBA to premultiplied ARGB for ICNS."""
    data = []
    pixels = img.load()
    for y in range(img.height):
        for x in range(img.width):
            r, g, b, a = pixels[x, y]
            # Premultiply
            r = r * a // 255
            g = g * a // 255
            b = b * a // 255
            data.append(struct.pack('4B', a, r, g, b))
    return b''.join(data)

def create_icns_entry(icon_type, img):
    """Create an ICNS entry for a given image."""
    if img.mode != 'RGBA':
        img = img.convert('RGBA')
    raw = rgba_to_premultiplied(img)
    # Header: type (4 bytes) + size (4 bytes big-endian)
    entry_size = 8 + len(raw)
    return struct.pack('>4sI', icon_type, entry_size) + raw

# ICNS icon types and sizes
ICNS_SIZES = [
    (b'ic07', 128, 128),    # 128x128
    (b'ic08', 256, 256),    # 256x256
    (b'ic09', 512, 512),    # 512x512
    (b'ic10', 1024, 1024),  # 1024x1024 (Retina)
    (b'ic11', 32, 32),      # 32x32
    (b'ic12', 64, 64),      # 64x64
    (b'ic13', 256, 256),    # 256x256@2x
    (b'ic14', 512, 512),    # 512x512@2x
]

entries = []
for icon_type, w, h in ICNS_SIZES:
    icns_img = img.resize((w, h), Image.LANCZOS)
    entry = create_icns_entry(icon_type, icns_img)
    entries.append(entry)

# ICNS file structure: 'icns' header + entries
total_size = 8 + sum(len(e) for e in entries)  # 8 = icns header
icns_data = struct.pack('>4sI', b'icns', total_size) + b''.join(entries)

icns_path = os.path.join(OUT_DIR, "icon.icns")
with open(icns_path, 'wb') as f:
    f.write(icns_data)
print(f"  icon.icns: {[f'{w}x{h}' for _, w, h in ICNS_SIZES]}")

print(f"\n✅ All icons generated in: {OUT_DIR}")
print(f"📁 Backup saved to: {BACKUP_DIR}")
