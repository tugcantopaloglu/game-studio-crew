import base64
import io
import struct
from pathlib import Path

from PIL import Image, ImageDraw

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
SOURCE = HERE / "logo.png"

INK = (232, 237, 245)
TILE_TOP = (24, 29, 38)
TILE_BOTTOM = (10, 13, 18)
EDGE = (148, 163, 184, 46)
PAGE_TOP = (13, 16, 21)
PAGE_BOTTOM = (8, 9, 13)

ICO_SIZES = [16, 20, 24, 32, 40, 48, 64, 128, 256]


def mark():
    im = Image.open(SOURCE).convert("RGBA")
    solid = im.getchannel("A").point(lambda a: 255 if a > 8 else 0)
    return im.crop(solid.getbbox())


def inked(glyph, rgb):
    out = Image.new("RGBA", glyph.size, rgb + (0,))
    out.putalpha(glyph.getchannel("A"))
    return out


def centred(canvas, glyph, fraction):
    w, h = glyph.size
    scale = (min(canvas) if isinstance(canvas, tuple) else canvas) * fraction / max(w, h)
    box = (max(1, round(w * scale)), max(1, round(h * scale)))
    return glyph.resize(box, Image.LANCZOS), box


def vertical_gradient(size, top, bottom):
    w, h = size
    grad = Image.new("RGB", (1, h))
    draw = ImageDraw.Draw(grad)
    for y in range(h):
        t = y / max(1, h - 1)
        draw.point((0, y), tuple(round(a + (b - a) * t) for a, b in zip(top, bottom)))
    return grad.resize((w, h), Image.NEAREST).convert("RGBA")


def tile(edge):
    scale = 8 if edge < 128 else 2
    big = edge * scale
    radius = round(big * (0.19 if edge < 48 else 0.22))
    mask = Image.new("L", (big, big), 0)
    ImageDraw.Draw(mask).rounded_rectangle([0, 0, big - 1, big - 1], radius=radius, fill=255)
    face = vertical_gradient((big, big), TILE_TOP, TILE_BOTTOM)
    face.putalpha(mask)
    ImageDraw.Draw(face).rounded_rectangle(
        [0, 0, big - 1, big - 1], radius=radius, outline=EDGE, width=max(scale, round(big * 0.012))
    )
    glyph, box = centred(big, inked(mark(), INK), 0.80 if edge < 48 else 0.72)
    face.alpha_composite(glyph, ((big - box[0]) // 2, (big - box[1]) // 2))
    return face.resize((edge, edge), Image.LANCZOS)


def bmp_frame(image):
    w, h = image.size
    header = struct.pack("<IiiHHIIiiII", 40, w, h * 2, 1, 32, 0, 0, 0, 0, 0, 0)
    pixels = image.transpose(Image.FLIP_TOP_BOTTOM).tobytes("raw", "BGRA")
    mask = bytes(((w + 31) // 32) * 4 * h)
    return header + pixels + mask


def png_frame(image):
    buffer = io.BytesIO()
    image.save(buffer, "PNG", optimize=True)
    return buffer.getvalue()


def write_ico(path, images):
    frames = [bmp_frame(im) if im.size[0] < 256 else png_frame(im) for im in images]
    offset = 6 + 16 * len(frames)
    directory = b""
    for image, frame in zip(images, frames):
        edge = image.size[0]
        directory += struct.pack(
            "<BBBBHHII", edge % 256, edge % 256, 0, 0, 1, 32, len(frame), offset
        )
        offset += len(frame)
    path.write_bytes(struct.pack("<HHH", 0, 1, len(frames)) + directory + b"".join(frames))


def write_png(path, image):
    path.parent.mkdir(parents=True, exist_ok=True)
    image.save(path, "PNG", optimize=True)


def wordmark(width):
    glyph = mark()
    height = max(1, round(width * glyph.size[1] / glyph.size[0]))
    alpha = glyph.resize((width, height), Image.LANCZOS).getchannel("A")
    luma = round(sum(INK) / 3)
    return Image.merge("LA", (Image.new("L", alpha.size, luma), alpha))


def wizard_page(size):
    page = vertical_gradient(size, PAGE_TOP, PAGE_BOTTOM)
    glyph, box = centred(size, inked(mark(), INK), 0.62)
    page.alpha_composite(glyph, ((size[0] - box[0]) // 2, round(size[1] * 0.30) - box[1] // 2))
    return page.convert("RGB")


def flattened(image, background):
    plate = Image.new("RGBA", image.size, background + (255,))
    plate.alpha_composite(image)
    return plate.convert("RGB")


def write_bmp(path, image):
    path.parent.mkdir(parents=True, exist_ok=True)
    image.convert("RGB").save(path, "BMP")


def main():
    icon = ROOT / "desktop" / "assets" / "icon.ico"
    icon.parent.mkdir(parents=True, exist_ok=True)
    write_ico(icon, [tile(edge) for edge in ICO_SIZES])

    splash = wordmark(160)
    (ROOT / "desktop" / "assets" / "mark.b64").write_text(
        base64.b64encode(png_frame(splash)).decode("ascii"), encoding="ascii"
    )

    web = ROOT / "crates" / "studio-server" / "web"
    write_png(web / "mark.png", wordmark(160))
    write_png(web / "favicon.png", tile(64))

    installer = ROOT / "installer" / "assets"
    write_bmp(installer / "wizard-page.bmp", wizard_page((164, 314)))
    write_bmp(installer / "wizard-page-2x.bmp", wizard_page((328, 628)))
    write_bmp(installer / "wizard-badge.bmp", flattened(tile(55), (255, 255, 255)))
    write_bmp(installer / "wizard-badge-2x.bmp", flattened(tile(110), (255, 255, 255)))

    write_png(HERE / "logo-tile.png", tile(256))

    for path in sorted(
        [icon, web / "mark.png", web / "favicon.png", HERE / "logo-tile.png"]
        + list(installer.glob("*.bmp"))
        + [ROOT / "desktop" / "assets" / "mark.b64"]
    ):
        print(f"{path.relative_to(ROOT).as_posix()}  {path.stat().st_size / 1024:.1f} KB")


if __name__ == "__main__":
    main()
