#!/usr/bin/env python3
"""Lucide 图标 → 折线常量,生成 crates/ui-kit/src/lucide.rs。

    python3 assets/icons/gen_icons.py

读 icons/lucide/*.svg(lucide-static 的原文件,ISC 许可,见 icons/lucide/LICENSE),
把 24×24 网格上的路径压平成折线;颜色、线宽、尺寸由 ui_kit::icons::paint_icon 决定。
构建不需要本脚本(产物已入库)。

新增图标:从 https://unpkg.com/lucide-static@<版本>/icons/<名字>.svg 下载到
icons/lucide/,在下面 ICONS 里登记,重新运行本脚本。
"""

import math
import pathlib
import re
import xml.etree.ElementTree as ET

HERE = pathlib.Path(__file__).resolve().parent
SRC = HERE / "lucide"
OUT = HERE.parent.parent / "crates" / "ui-kit" / "src" / "lucide.rs"

# (SVG 文件名, Rust 常量名)
ICONS = [
    ("settings", "SETTINGS"),
    ("sun", "SUN"),
    ("moon", "MOON"),
    ("chevron-down", "CHEVRON_DOWN"),
    ("chevron-up", "CHEVRON_UP"),
    ("chevron-left", "CHEVRON_LEFT"),
    ("chevron-right", "CHEVRON_RIGHT"),
    ("chevrons-up-down", "CHEVRONS_UP_DOWN"),
    ("ellipsis", "ELLIPSIS"),
    ("refresh-cw", "REFRESH"),
    ("plus", "PLUS"),
    ("minus", "MINUS"),
    ("x", "X"),
    ("check", "CHECK"),
    ("star", "STAR"),
    ("arrow-up", "ARROW_UP"),
    ("arrow-down", "ARROW_DOWN"),
    ("arrow-up-down", "ARROW_UP_DOWN"),
    ("arrow-up-narrow-wide", "SORT_ASC"),
    ("arrow-down-wide-narrow", "SORT_DESC"),
    ("filter", "FILTER"),
    ("search", "SEARCH"),
    ("download", "DOWNLOAD"),
    ("upload", "UPLOAD"),
    ("play", "PLAY"),
    ("sparkles", "SPARKLES"),
    ("history", "HISTORY"),
    ("align-left", "ALIGN_LEFT"),
    ("file-text", "FILE_TEXT"),
    ("arrow-right-left", "TRANSFER"),
    ("git-compare", "COMPARE"),
    ("file-diff", "FILE_DIFF"),
    ("blocks", "BLOCKS"),
    ("square-plus", "SQUARE_PLUS"),
    ("square-minus", "SQUARE_MINUS"),
    ("square-pen", "SQUARE_PEN"),
    ("pencil-ruler", "PENCIL_RULER"),
    ("table-properties", "TABLE_PROPS"),
    ("table-2", "TABLE"),
    ("database", "DATABASE"),
    ("key", "KEY"),
    ("trash-2", "TRASH"),
    ("pencil", "PENCIL"),
    ("copy", "COPY"),
    ("eye", "EYE"),
    ("eye-off", "EYE_OFF"),
    ("infinity", "INFINITY"),
    ("plug", "PLUG"),
    ("link-2", "LINK"),
    ("columns-3", "COLUMNS"),
    ("hash", "HASH"),
    ("rows-3", "ROWS"),
    ("file-plus-2", "FILE_PLUS"),
    ("wand-sparkles", "WAND"),
    ("book-open", "BOOK_OPEN"),
    ("bookmark", "BOOKMARK"),
    ("list-filter", "LIST_FILTER"),
    ("circle-check", "CIRCLE_CHECK"),
    ("circle-x", "CIRCLE_X"),
    ("info", "INFO"),
    ("triangle-alert", "TRIANGLE_ALERT"),
    ("circle-plus", "CIRCLE_PLUS"),
    ("corner-down-left", "CORNER_DOWN_LEFT"),
]

NUM = re.compile(r"[+-]?(\d+\.?\d*|\.\d+)([eE][+-]?\d+)?")


class Cursor:
    def __init__(self, s):
        self.s = s
        self.i = 0

    def skip(self):
        while self.i < len(self.s) and self.s[self.i] in " \t\r\n,":
            self.i += 1

    def peek_cmd(self):
        self.skip()
        if self.i < len(self.s) and self.s[self.i].isalpha():
            c = self.s[self.i]
            self.i += 1
            return c
        return None

    def number(self):
        self.skip()
        m = NUM.match(self.s, self.i)
        if not m:
            raise ValueError(f"not a number at {self.i}: {self.s[self.i:self.i+12]!r}")
        self.i = m.end()
        return float(m.group(0))

    def flag(self):
        self.skip()
        c = self.s[self.i]
        if c not in "01":
            raise ValueError(f"not a flag at {self.i}: {c!r}")
        self.i += 1
        return c == "1"


def arc_points(x1, y1, rx, ry, rot, large, sweep, x2, y2):
    """SVG 椭圆弧端点参数 → 圆心参数,按角度采样。"""
    phi = math.radians(rot)
    cp, sp = math.cos(phi), math.sin(phi)
    dx, dy = (x1 - x2) / 2.0, (y1 - y2) / 2.0
    x1p, y1p = cp * dx + sp * dy, -sp * dx + cp * dy
    if rx == 0 or ry == 0:
        return [(x2, y2)]
    lam = x1p * x1p / rx**2 + y1p * y1p / ry**2
    if lam > 1:
        k = math.sqrt(lam)
        rx, ry = rx * k, ry * k
    num = rx**2 * ry**2 - rx**2 * y1p**2 - ry**2 * x1p**2
    den = rx**2 * y1p**2 + ry**2 * x1p**2
    coef = 0.0 if den == 0 else math.sqrt(max(0.0, num / den))
    if large == sweep:
        coef = -coef
    cxp, cyp = coef * rx * y1p / ry, coef * -ry * x1p / rx
    cx, cy = cp * cxp - sp * cyp + (x1 + x2) / 2.0, sp * cxp + cp * cyp + (y1 + y2) / 2.0

    def ang(ux, uy, vx, vy):
        a = math.atan2(ux * vy - uy * vx, ux * vx + uy * vy)
        return a

    ux, uy = (x1p - cxp) / rx, (y1p - cyp) / ry
    vx, vy = (-x1p - cxp) / rx, (-y1p - cyp) / ry
    theta = ang(1.0, 0.0, ux, uy)
    delta = ang(ux, uy, vx, vy)
    if not sweep and delta > 0:
        delta -= 2 * math.pi
    elif sweep and delta < 0:
        delta += 2 * math.pi
    n = max(2, math.ceil(abs(delta) / math.radians(12.5)))
    pts = []
    for i in range(1, n + 1):
        a = theta + delta * i / n
        px, py = rx * math.cos(a), ry * math.sin(a)
        pts.append((cp * px - sp * py + cx, sp * px + cp * py + cy))
    return pts


def parse_path(d):
    """SVG path d → 若干条折线(每条是一串点;闭合的首尾重合)。"""
    c = Cursor(d)
    lines, cur, start = [], [], None
    x = y = 0.0
    cmd = None
    closed = False
    while True:
        nxt = c.peek_cmd()
        if nxt:
            cmd = nxt
            if cmd in "Zz":
                if cur:
                    if cur[0] != cur[-1]:
                        cur.append(cur[0])
                    lines.append(cur)
                cur, start, closed = [], start, True
                continue
        else:
            c.skip()
            if c.i >= len(c.s) or cmd is None or cmd in "Zz":
                break
        closed = False
        rel = cmd.islower()
        C = cmd.upper()
        ox, oy = (x, y) if rel else (0.0, 0.0)
        if C == "M":
            x, y = ox + c.number(), oy + c.number()
            if cur:
                lines.append(cur)
            cur, start = [(x, y)], (x, y)
            cmd = "l" if rel else "L"  # 后续隐式参数按直线处理
        elif C == "L":
            x, y = ox + c.number(), oy + c.number()
            cur.append((x, y))
        elif C == "H":
            x = ox + c.number()
            cur.append((x, y))
        elif C == "V":
            y = oy + c.number()
            cur.append((x, y))
        elif C == "C":
            x1, y1 = ox + c.number(), oy + c.number()
            x2, y2 = ox + c.number(), oy + c.number()
            x3, y3 = ox + c.number(), oy + c.number()
            n = 14
            for i in range(1, n + 1):
                t = i / n
                u = 1.0 - t
                px = u**3 * x + 3 * u * u * t * x1 + 3 * u * t * t * x2 + t**3 * x3
                py = u**3 * y + 3 * u * u * t * y1 + 3 * u * t * t * y2 + t**3 * y3
                cur.append((px, py))
            x, y = x3, y3
        elif C in "TSQ":
            raise ValueError(f"unsupported path command {cmd!r} in {d!r}")
        elif C == "A":
            rx, ry = c.number(), c.number()
            rot = c.number()
            large, sweep = c.flag(), c.flag()
            x2, y2 = ox + c.number(), oy + c.number()
            cur.extend(arc_points(x, y, rx, ry, rot, large, sweep, x2, y2))
            x, y = x2, y2
        else:
            raise ValueError(f"unsupported path command {cmd!r}")
    if cur:
        lines.append(cur)
    return lines


def ellipse(cx, cy, rx, ry):
    n = 36 if max(rx, ry) >= 3 else 16
    pts = [(cx + rx * math.cos(2 * math.pi * i / n), cy + ry * math.sin(2 * math.pi * i / n)) for i in range(n)]
    pts.append(pts[0])
    return pts


def rounded_rect(x, y, w, h, rx, ry):
    rx = min(rx or 0.0, w / 2.0)
    ry = min(ry or rx or 0.0, h / 2.0)
    if rx <= 0 or ry <= 0:
        pts = [(x, y), (x + w, y), (x + w, y + h), (x, y + h)]
        pts.append(pts[0])
        return pts
    pts = []

    def corner(cx, cy, a0):
        n = 6
        for i in range(1, n + 1):
            a = math.radians(a0 + 90.0 * i / n)
            pts.append((cx + rx * math.cos(a), cy + ry * math.sin(a)))

    pts.append((x + rx, y))
    corner(x + w - rx, y + ry, -90)
    corner(x + w - rx, y + h - ry, 0)
    corner(x + rx, y + h - ry, 90)
    corner(x + rx, y + ry, 180)
    pts.append(pts[0])
    return pts


def parse_svg(path):
    root = ET.parse(path).getroot()
    lines = []
    for el in root:
        tag = el.tag.split("}")[-1]
        a = el.attrib

        def f(k, default=0.0):
            return float(a.get(k, default))

        if tag == "path":
            lines.extend(parse_path(a["d"]))
        elif tag == "circle":
            lines.append(ellipse(f("cx"), f("cy"), f("r"), f("r")))
        elif tag == "ellipse":
            lines.append(ellipse(f("cx"), f("cy"), f("rx"), f("ry")))
        elif tag == "rect":
            lines.append(rounded_rect(f("x"), f("y"), f("width"), f("height"), f("rx"), f("ry")))
        elif tag == "line":
            lines.append([(f("x1"), f("y1")), (f("x2"), f("y2"))])
        elif tag in ("polygon", "polyline"):
            nums = [float(v) for v in re.split(r"[ ,\s]+", a["points"].strip()) if v]
            pts = list(zip(nums[0::2], nums[1::2]))
            if tag == "polygon" and pts[0] != pts[-1]:
                pts.append(pts[0])
            lines.append(pts)
        else:
            raise ValueError(f"unsupported element <{tag}> in {path.name}")
    return lines


def main():
    out = [
        "//! 由 assets/icons/gen_icons.py 生成,不要手改:Lucide 图标的折线数据。",
        "//!",
        "//! 坐标在 24×24 网格上(原版描边宽 2);绘制见 `icons::paint_icon`。",
        "//! 图标来自 Lucide(https://lucide.dev),ISC License,许可见 assets/icons/lucide/LICENSE。",
        "",
        "/// 一个图标:若干条折线(闭合的首尾重合)",
        "pub struct Icon {",
        "    pub lines: &'static [&'static [[f32; 2]]],",
        "}",
        "",
    ]
    total_pts = 0
    for fname, name in ICONS:
        lines = parse_svg(SRC / f"{fname}.svg")
        parts = []
        for ln in lines:
            total_pts += len(ln)
            pts = ", ".join(f"[{x:.2f}, {y:.2f}]" for x, y in ln)
            parts.append(f"        &[{pts}],")
        out.append(f"pub const {name}: Icon = Icon {{\n    lines: &[\n" + "\n".join(parts) + "\n    ],\n};\n")
    header = "// ---------------------------------------------------------------- " + f"{len(ICONS)} 个图标\n\n"
    out.append("/// 全部图标:(名字, 图标)。图标总览图与测试用\n")
    out.append("pub const ALL: &[(&str, &Icon)] = &[\n")
    for _, name in ICONS:
        out.append(f'    ("{name.lower()}", &{name}),\n')
    out.append("];\n")
    body = out[:9] + [header] + out[9:]
    OUT.write_text("\n".join(body))
    print(f"{len(ICONS)} 个图标,{total_pts} 个点 → {OUT} ({OUT.stat().st_size // 1024} KB)")


if __name__ == "__main__":
    main()
