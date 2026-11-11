"""Draws Brink's app icon, menu bar glyph and DMG background.

    python3 scripts/make-icons.py && npx tauri icon src-tauri/icons/source.png
"""
from PIL import Image, ImageChops, ImageDraw, ImageFilter, ImageFont
import os

ROOT = os.path.join(os.path.dirname(__file__), "..")
ICONS = os.path.join(ROOT, "src-tauri", "icons")
SITE = os.path.join(ROOT, "site", "assets")
os.makedirs(ICONS, exist_ok=True)
os.makedirs(SITE, exist_ok=True)


def lerp(a, b, t):
    return tuple(int(a[i] + (b[i] - a[i]) * t) for i in range(len(a)))


def vgrad(w, h, top, bottom):
    g = Image.new("RGBA", (w, h))
    d = ImageDraw.Draw(g)
    for y in range(h):
        d.line([(0, y), (w, y)], fill=lerp(top, bottom, y / max(1, h - 1)))
    return g


def rounded_mask(w, h, r, scale=4):
    m = Image.new("L", (w * scale, h * scale), 0)
    ImageDraw.Draw(m).rounded_rectangle([0, 0, w * scale - 1, h * scale - 1], r * scale, fill=255)
    return m.resize((w, h), Image.LANCZOS)


def app_icon(size=1024):
    """A trackpad outline whose right edge is lit, with a fingertip on it: the brink."""
    k = 2  # supersampled
    W = size * k
    img = Image.new("RGBA", (W, W), (0, 0, 0, 0))
    # macOS icon grid: 824px body, centered, ~185px corner radius.
    body, r = 824 * k, 186 * k
    off = (W - body) // 2
    box = [off, off, off + body, off + body]

    shadow = Image.new("RGBA", (W, W), (0, 0, 0, 0))
    ImageDraw.Draw(shadow).rounded_rectangle([off, off + 20 * k, off + body, off + body + 20 * k], r, fill=(0, 0, 0, 120))
    img.alpha_composite(shadow.filter(ImageFilter.GaussianBlur(24 * k)))
    full = Image.new("RGBA", (W, W), (0, 0, 0, 0))
    full.paste(vgrad(body, body, (44, 45, 49, 255), (16, 16, 18, 255)), (off, off))
    img.paste(full, (0, 0), ring_mask(W, box, r))
    # A soft top bevel that fades out by the middle of the body.
    bevel = ImageChops.multiply(ring_mask(W, box, r, 3 * k), vgrad(W, W, (255,) * 4, (0, 0, 0, 255)).convert("L").point(lambda v: max(0, v * 2 - 255)))
    img.alpha_composite(tinted((255, 255, 255), bevel, 0.28))

    pw, ph, pr, sw = 600 * k, 448 * k, 70 * k, 34 * k
    px, py = (W - pw) // 2, (W - ph) // 2
    ring = ring_mask(W, [px, py, px + pw, py + ph], pr, sw)
    img.alpha_composite(tinted((255, 255, 255), ring, 0.2))

    # The lit edge: the right side of the ring, fading in around both right corners.
    fade = Image.new("L", (W, W), 0)
    fd = ImageDraw.Draw(fade)
    x0 = px + pw - 270 * k
    for x in range(x0, W):
        t = min(1.0, (x - x0) / (150 * k))
        fd.line([(x, 0), (x, W)], fill=int(255 * t * t * (3 - 2 * t)))
    lit = ImageChops.multiply(ring, fade)
    warm = vgrad(W, W, (255, 196, 92, 255), (255, 84, 22, 255))
    glow = warm.copy()
    glow.putalpha(lit.filter(ImageFilter.GaussianBlur(26 * k)).point(lambda v: int(v * 0.9)))
    img.alpha_composite(glow)
    edge = warm.copy()
    edge.putalpha(lit)
    img.alpha_composite(edge)

    # A fingertip mid-slide on the lit edge.
    cx, cy = px + pw - sw // 2, py + int(ph * 0.4)
    halo = Image.new("RGBA", (W, W), (0, 0, 0, 0))
    ImageDraw.Draw(halo).ellipse([cx - 64 * k, cy - 64 * k, cx + 64 * k, cy + 64 * k], fill=(255, 240, 220, 170))
    img.alpha_composite(halo.filter(ImageFilter.GaussianBlur(20 * k)))
    ImageDraw.Draw(img).ellipse([cx - 34 * k, cy - 34 * k, cx + 34 * k, cy + 34 * k], fill=(255, 255, 255, 255))
    return img.resize((size, size), Image.LANCZOS)


def ring_mask(W, box, r, width=None):
    m = Image.new("L", (W, W), 0)
    d = ImageDraw.Draw(m)
    if width:
        d.rounded_rectangle(box, r, outline=255, width=width)
    else:
        d.rounded_rectangle(box, r, fill=255)
    return m


def tinted(rgb, mask, alpha):
    layer = Image.new("RGBA", mask.size, rgb + (0,))
    layer.putalpha(mask.point(lambda v: int(v * alpha)))
    return layer


def tray_icon():
    # 22pt template glyph @2x: the trackpad outline with a fingertip on its right edge.
    W, H = 44, 44
    s = 8
    im = Image.new("RGBA", (W * s, H * s), (0, 0, 0, 0))
    d = ImageDraw.Draw(im)
    x0, y0, x1, y1 = 4 * s, 11 * s, 36 * s, 33 * s
    d.rounded_rectangle([x0, y0, x1, y1], 5 * s, outline=(0, 0, 0, 255), width=int(2.6 * s))
    cx, cy = x1 - 1.3 * s, y0 + 9.5 * s
    d.ellipse([cx - 7 * s, cy - 7 * s, cx + 7 * s, cy + 7 * s], fill=(0, 0, 0, 0))  # a gap around the fingertip
    d.ellipse([cx - 4.8 * s, cy - 4.8 * s, cx + 4.8 * s, cy + 4.8 * s], fill=(0, 0, 0, 255))
    return im.resize((W, H), Image.LANCZOS)


def dmg_background():
    W, H = 1320, 800
    img = vgrad(W, H, (246, 245, 242, 255), (226, 224, 219, 255)).convert("RGB")
    d = ImageDraw.Draw(img)
    for i in range(100):
        t = i / 99
        x = 360 + 600 * t
        y = 340 - 80 * (4 * t * (1 - t))
        r = 4 - 2 * t
        d.ellipse([x - r, y - r, x + r, y + r], fill=(255, 100, 30))
    d.polygon([(950, 320), (972, 340), (950, 360)], fill=(255, 100, 30))
    try:
        font = ImageFont.truetype("/System/Library/Fonts/SFNS.ttf", 30)
    except OSError:
        font = ImageFont.truetype("/System/Library/Fonts/Helvetica.ttc", 30)
    text = "Drag Brink to Applications"
    w = d.textbbox((0, 0), text, font=font)[2]
    d.text(((W - w) // 2, H - 140), text, fill=(70, 72, 76), font=font)
    img.save(os.path.join(ICONS, "dmg-background.png"), dpi=(144, 144))


if __name__ == "__main__":
    icon = app_icon()
    icon.save(os.path.join(ICONS, "source.png"))
    icon.resize((512, 512), Image.LANCZOS).save(os.path.join(SITE, "icon-512.png"))
    icon.resize((64, 64), Image.LANCZOS).save(os.path.join(SITE, "favicon.png"))
    # The panel header's logo (the website embeds its own copy of the panel).
    for d in ("ui", os.path.join("site", "panel")):
        icon.resize((128, 128), Image.LANCZOS).save(os.path.join(ROOT, d, "icon.png"))
    tray_icon().save(os.path.join(ICONS, "tray.png"))
    dmg_background()
    print("icons written")
