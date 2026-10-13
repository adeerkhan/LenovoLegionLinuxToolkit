#!/usr/bin/env python3
"""Generate the application icon at every size the icon theme expects.

Drawn rather than sourced, so the project carries no binary blob in git history
and the icon can be regenerated with `make icon`. The design is deliberately
simple enough to stay legible at 16px: an accent-coloured rounded badge with a
white 'L', echoing the accent the application itself uses.
"""

from pathlib import Path

SIZES = [16, 22, 24, 32, 48, 64, 128, 256, 512]

ACCENT = (255, 33, 33)
BADGE = (18, 18, 20)
NEAR_WHITE = (246, 246, 248)


def inset(box: list[float], fraction: float) -> list[float]:
    """Shrink a box by `fraction` of its own size on every side.

    Insetting by a fraction of the *box* keeps the drawing proportional
    whatever it is placed inside — insetting by a fraction of the image instead
    silently distorts it whenever the outer box is not the whole image.
    """
    dx = (box[2] - box[0]) * fraction
    dy = (box[3] - box[1]) * fraction
    return [box[0] + dx, box[1] + dy, box[2] - dx, box[3] - dy]


def draw_l(draw, box: list[float]) -> None:
    """A bold 'L' from two rectangles — a stem and a foot.

    Sized as fractions of the box so a single definition scales to every output
    size without per-size tuning.
    """
    width = box[2] - box[0]
    stem = width * 0.34
    foot = width * 0.32

    draw.rectangle([box[0], box[1], box[0] + stem, box[3]], fill=NEAR_WHITE)
    draw.rectangle(
        [box[0], box[3] - foot, box[0] + width * 0.64, box[3]], fill=NEAR_WHITE
    )


def build(size: int) -> None:
    from PIL import Image, ImageDraw

    scale = 4  # supersample, then downscale, for clean edges at small sizes
    edge = size * scale
    img = Image.new("RGBA", (edge, edge), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)

    pad = edge * 0.06
    outer = [pad, pad, edge - pad, edge - pad]
    d.rounded_rectangle(outer, radius=edge * 0.22, fill=ACCENT)

    # Inset the inner badge by a fraction of the outer box's own size, so the
    # ring it leaves is proportional at every output size.
    inner = inset(outer, 0.15)
    d.rounded_rectangle(inner, radius=edge * 0.15, fill=BADGE)

    draw_l(d, inset(inner, 0.18))

    img = img.resize((size, size), Image.LANCZOS)
    out = Path("packaging/icons/hicolor") / f"{size}x{size}" / "apps"
    out.mkdir(parents=True, exist_ok=True)
    img.save(out / "legiontoolkit.png")
    print(f"wrote {out / 'legiontoolkit.png'}")


if __name__ == "__main__":
    for size in SIZES:
        build(size)
