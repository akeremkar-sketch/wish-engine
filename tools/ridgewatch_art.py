"""Draws the placeholder pixel art of the example game `examples/ridgewatch`.

The engine's `asset_create` makes one-shape placeholder pictures; a walk cycle
and a tileset need frames that differ, so these few pictures are drawn here,
in code, and checked into the example (no art is downloaded). Run it again
after changing a picture:

    python tools/ridgewatch_art.py

Every picture is drawn from plain integer arithmetic, so the files come out
the same on every machine. The characters are drawn in white and greys: the
game tints each player's keeper with their own colour (`Sprite.color`).
"""

from pathlib import Path

from PIL import Image

ROOT = Path(__file__).resolve().parent.parent / "examples" / "ridgewatch" / "assets"


def rgba(hex_text, alpha=255):
    value = int(hex_text, 16)
    return ((value >> 16) & 255, (value >> 8) & 255, value & 255, alpha)


CLEAR = (0, 0, 0, 0)


def draw(picture, x0, y0, rows, palette):
    """Paints `rows` (strings, one character a pixel) at (x0, y0)."""
    for dy, row in enumerate(rows):
        for dx, key in enumerate(row):
            if key in palette:
                picture.putpixel((x0 + dx, y0 + dy), palette[key])


# --- The keeper: a hooded storm parka with goggles, 16 x 24 per frame -------

KEEPER = {
    "o": rgba("2a2a33"),  # outline
    "W": rgba("ffffff"),  # parka, lit
    "w": rgba("cfcfd6"),  # parka, shade
    "s": rgba("9a9aa6"),  # parka, deep shade
    "f": rgba("3c3a44"),  # face opening
    "g": rgba("fff2b8"),  # goggles
    "b": rgba("5b5560"),  # boots
    "l": rgba("ffd27a"),  # lantern glass
}

HEAD = [
    "......oooo......",
    "....ooWWWWoo....",
    "...oWWWWWWWwo...",
    "..oWWWffffWWwo..",
    "..oWWfgffgfWwo..",
    "..oWWffffffWwo..",
    "..owWWffffWWso..",
    "...owwWWWWwso...",
]

BODY_IDLE = [
    "..ooWWWWWWWWoo..",
    ".oWWWWWWWWWWwwo.",
    ".oWWwWWWWWWwwso.",
    ".owWwWWWWWWwsso.",
    "oWwoWWWWWWWWosso",
    "oWwoWWWWWWWwoslo",
    "owsoWWWWWWwwooo.",
    ".oo.wWWWWWwso...",
    "....wwWWWwwso...",
    "....swwwwwsso...",
    "....oss..sso....",
    "....obb..bbo....",
    "....oooo.ooo....",
]

LEGS_WALK = [
    [
        "....swwwwwsso...",
        "...oss....sso...",
        "...obb.....bbo..",
        "...ooo.....ooo..",
    ],
    [
        "....swwwwwsso...",
        "....oss..sso....",
        "....obb..bbo....",
        "....oooo.ooo....",
    ],
    [
        "....swwwwwsso...",
        "..oss.....sso...",
        "..obb....obbo...",
        "..ooo....ooo....",
    ],
    [
        "....swwwwwsso...",
        "....oss..sso....",
        "....obb..bbo....",
        "....oooo.ooo....",
    ],
]


def keeper_frame(picture, x0, y0, bob, legs=None):
    head_y = y0 + 3 + bob
    draw(picture, x0, head_y, HEAD, KEEPER)
    body = BODY_IDLE[:9] if legs is not None else BODY_IDLE
    draw(picture, x0, head_y + len(HEAD), body, KEEPER)
    if legs is not None:
        draw(picture, x0, head_y + len(HEAD) + 9, legs, KEEPER)


def ice_frame(picture, x0, y0):
    """A keeper frozen in a block of ice (white: the tint shows whose)."""
    keeper_frame(picture, x0, y0, 0)
    ice = rgba("bfe8ff", 150)
    edge = rgba("eaf8ff", 230)
    for y in range(y0 + 2, y0 + 24):
        for x in range(x0 + 1, x0 + 15):
            under = picture.getpixel((x, y))
            on_edge = x in (x0 + 1, x0 + 14) or y in (y0 + 2, y0 + 23)
            colour = edge if on_edge else ice
            if under[3] == 0 or on_edge:
                picture.putpixel((x, y), colour)
            else:
                picture.putpixel(
                    (x, y),
                    tuple((a + b * 2) // 3 for a, b in zip(under[:3], colour[:3])) + (255,),
                )
    for k in range(5):
        picture.putpixel((x0 + 3 + k, y0 + 4 + k), rgba("ffffff"))


def keeper_sheet():
    picture = Image.new("RGBA", (64, 72), CLEAR)
    # Row 0: idle (two breaths), row 1: the walk cycle, row 2: frozen.
    for column, bob in enumerate([0, 1, 0, 1]):
        keeper_frame(picture, column * 16, 0, bob)
    for column, legs in enumerate(LEGS_WALK):
        keeper_frame(picture, column * 16, 24, column % 2, legs)
    ice_frame(picture, 0, 48)
    return picture


# --- Props: 16 x 16 per frame -------------------------------------------------

PROPS = {
    "o": rgba("1f1d24"),
    "B": rgba("e8b84a"),  # brass
    "b": rgba("a87822"),
    "d": rgba("6b4a14"),
    "M": rgba("8d96a3"),  # metal
    "m": rgba("5e6672"),
    "k": rgba("3a404a"),
    "R": rgba("ff5a4a"),  # warning light
    "G": rgba("6cff9a"),  # screen
    "g": rgba("2d8a52"),
    "Y": rgba("ffe066"),
    "C": rgba("9fe3ff"),  # glass
    "W": rgba("ffffff"),
    "w": rgba("cfe6ff"),
}

BELL = [
    ".......oo.......",
    "......oBBo......",
    ".....okddko.....",
    "......oBBo......",
    ".....oBBBbo.....",
    "....oBBBBBbo....",
    "....oBWBBBbo....",
    "...oBBWBBBBbo...",
    "...oBBBBBBBbo...",
    "..oBBBBBBBBBbo..",
    "..oBBBBBBBBBbdo.",
    ".oBBBBBBBBBBbbdo",
    ".oddddddddddddo.",
    "......obbo......",
    "......oddo......",
    ".......oo.......",
]

GENERATOR = [
    "................",
    ".oooooooooooooo.",
    ".oMMMMMMMMMMMMo.",
    ".oMkkkkMMRMMGMo.",
    ".oMkMMkMMMMMMMo.",
    ".oMkkkkMMmmmmMo.",
    ".oMMMMMMMmGGmMo.",
    ".oMmmmmmMmGGmMo.",
    ".oMmkkkmMmmmmMo.",
    ".oMmkkkmMMMMMMo.",
    ".oMmmmmmMkMkMko.",
    ".oMMMMMMMkMkMko.",
    ".ommmmmmmmmmmmo.",
    ".okk.......kkko.",
    ".oo.........ooo.",
    "................",
]

RADIO = [
    "................",
    "..........o.....",
    "..........o.....",
    "..oooooooooooo..",
    "..oMMMMMMMMMMo..",
    "..oMGGGGGGGGMo..",
    "..oMGgGgGgGGMo..",
    "..oMGGGGGGGGMo..",
    "..oMMMMMMMMMMo..",
    "..oMkMkMkMRMMo..",
    "..oMMMMMMMMMMo..",
    "..oMmkkmMmkkmo..",
    "..oMmkkmMmkkmo..",
    "..ommmmmmmmmmo..",
    "..oo........oo..",
    "................",
]

ANTENNA = [
    "....ooooooo.....",
    "..ooMMMMMMMoo...",
    ".oMMMmmmmmMMMo..",
    "oMMmmMMMMMmmMMo.",
    "oMmMMMMMMMMMmMo.",
    ".oMMMMMYMMMMMo..",
    "..ooMMMoMMMoo...",
    "....oooMooo.....",
    ".......M........",
    ".......M........",
    "......oMo.......",
    "......oMo.......",
    ".....oMMMo......",
    "....okkkkko.....",
    "...okkkkkkko....",
    "...ooooooooo....",
]

TELESCOPE = [
    "...........ooo..",
    "..........oCCo..",
    ".........oMMMo..",
    "........oMMMo...",
    ".......oMMMo....",
    "......oMMMo.....",
    ".....oMMMo......",
    "....oMMMo.......",
    "...okMMo........",
    "...okko.........",
    "....oMo.........",
    "....oMo.........",
    "...oMMMo........",
    "..oMkkkMo.......",
    ".oMk...kMo......",
    ".oo.....oo......",
]

BOILER = [
    "................",
    "....oooooooo....",
    "...oMMMMMMMMo...",
    "..oMMmmmmmmMMo..",
    "..oMmRRRRRRmMo..",
    "..oMmRYYYYRmMo..",
    "..oMmRYYYYRmMo..",
    "..oMmRRRRRRmMo..",
    "..oMMmmmmmmMMo..",
    "..oMMMMMMMMMMo..",
    "..oMkMMMMMMkMo..",
    "..oMMMMMMMMMMo..",
    "..oMMMMMMMMMMo..",
    "...oMMMMMMMMo...",
    "....oooooooo....",
    "................",
]

FUSE_BOX = [
    "................",
    "...oooooooooo...",
    "...oMMMMMMMMo...",
    "...oMYYMMYYMo...",
    "...oMYYMMYYMo...",
    "...oMMMMMMMMo...",
    "...oMkkkkkkMo...",
    "...oMkRRRRkMo...",
    "...oMkkkkkkMo...",
    "...oMMMMMMMMo...",
    "...oMYYMMYYMo...",
    "...oMYYMMYYMo...",
    "...oMMMMMMMMo...",
    "...oooooooooo...",
    "................",
    "................",
]

DOOR = [
    "oooooooooooooooo",
    "oMMMMMMMMMMMMMMo",
    "oMmmmmmmmmmmmmMo",
    "oMmMMMMMMMMMMmMo",
    "oMmMkkkkkkkkMmMo",
    "oMmMkYkkkkYkMmMo",
    "oMmMkkkkkkkkMmMo",
    "oMmMMMMMMMMMMmMo",
    "oMmMMMMMMMMMMmMo",
    "oMmMkkkkkkkkMmMo",
    "oMmMkYkkkkYkMmMo",
    "oMmMkkkkkkkkMmMo",
    "oMmMMMMMMMMMMmMo",
    "oMmmmmmmmmmmmmMo",
    "oMMMMMMMMMMMMMMo",
    "oooooooooooooooo",
]

WISP = [
    "................",
    "......wwww......",
    ".....wWWWWw.....",
    "....wWWWWWWw....",
    "....wWoWWoWw....",
    "....wWWWWWWw....",
    "....wWWWWWWw....",
    ".....wWWWWw.....",
    "......wWWw......",
    ".......ww.......",
    "......w..w......",
    ".......w........",
    "................",
    "................",
    "................",
    "................",
]

LAMP = [
    "................",
    "......oooo......",
    ".....oYYYYo.....",
    "....oYYWWYYo....",
    "....oYWWWWYo....",
    "....oYYWWYYo....",
    ".....oYYYYo.....",
    "......okko......",
    "................",
    "................",
    "................",
    "................",
    "................",
    "................",
    "................",
    "................",
]


def props_sheet():
    frames = [BELL, GENERATOR, RADIO, ANTENNA, TELESCOPE, BOILER, FUSE_BOX, DOOR, WISP, LAMP]
    picture = Image.new("RGBA", (128, 32), CLEAR)
    for index, rows in enumerate(frames):
        draw(picture, (index % 8) * 16, (index // 8) * 16, rows, PROPS)
    return picture


# --- The station's tiles: 16 x 16, 8 in a row --------------------------------


def tile(picture, index, colour_at):
    for y in range(16):
        for x in range(16):
            picture.putpixel((index * 16 + x, y), colour_at(x, y))


def station_tiles():
    picture = Image.new("RGBA", (128, 16), CLEAR)

    def snow(x, y):
        speck = (x * 7 + y * 13) % 23 == 0 or (x * 5 + y * 3) % 31 == 0
        return rgba("f2f6ff") if speck else rgba("dfe7f5")

    def metal(x, y):
        if x in (0, 15) or y in (0, 15):
            return rgba("4a525e")
        if (x, y) in ((2, 2), (13, 2), (2, 13), (13, 13)):
            return rgba("9aa4b2")
        return rgba("6f7886") if (x + y) % 8 else rgba("737c8a")

    def planks(x, y):
        seam = y % 4 == 0 or (x + (y // 4) * 5) % 16 == 0
        return rgba("6a4a32") if seam else rgba("9a6c48")

    def grating(x, y):
        if x % 4 == 0 or y % 4 == 0:
            return rgba("5a6270")
        return rgba("2c313a")

    def wall(x, y):
        if y < 3:
            return rgba("c9d2de") if y == 0 else rgba("aab4c2")
        if y == 15:
            return rgba("1d2128")
        brick = (y // 4) % 2
        if y % 4 == 3 or (x + brick * 8) % 16 == 0:
            return rgba("2e333c")
        return rgba("49505c")

    def wall_stripe(x, y):
        if 6 <= y <= 9:
            return rgba("e8b84a") if (x + y) % 6 < 3 else rgba("2e333c")
        return wall(x, y)

    def hazard(x, y):
        if y < 3 or y > 12:
            return rgba("e8b84a") if (x + y) % 6 < 3 else rgba("2e333c")
        return metal(x, y)

    def rug(x, y):
        if x in (0, 15) or y in (0, 15):
            return rgba("5a2430")
        return rgba("8a3644") if (x // 2 + y // 2) % 2 else rgba("7a2e3c")

    for index, painter in enumerate([snow, metal, planks, grating, wall, wall_stripe, hazard, rug]):
        tile(picture, index, painter)
    return picture


def main():
    (ROOT / "sprites").mkdir(parents=True, exist_ok=True)
    (ROOT / "tiles").mkdir(parents=True, exist_ok=True)
    keeper_sheet().save(ROOT / "sprites" / "keeper.png", optimize=True)
    props_sheet().save(ROOT / "sprites" / "props.png", optimize=True)
    station_tiles().save(ROOT / "tiles" / "station.png", optimize=True)


if __name__ == "__main__":
    main()
