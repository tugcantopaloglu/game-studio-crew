import sys

from PIL import Image


def main():
    if len(sys.argv) < 2:
        print("usage: cutout_check.py <image.png>", file=sys.stderr)
        return 2

    path = sys.argv[1]
    try:
        image = Image.open(path)
    except Exception as why:
        print(f"unreadable {path}: {why}", file=sys.stderr)
        return 1

    if image.mode != "RGBA":
        image = image.convert("RGBA")

    width, height = image.size
    alpha = image.getchannel("A")
    pixels = alpha.load()
    corners = [
        pixels[0, 0],
        pixels[width - 1, 0],
        pixels[0, height - 1],
        pixels[width - 1, height - 1],
    ]

    histogram = alpha.histogram()
    opaque = sum(histogram[201:])
    clear = sum(histogram[:55])

    print(
        f"cutout {width}x{height} (corners {max(corners)}, "
        f"{opaque} opaque, {clear} clear, {width * height} total)"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
