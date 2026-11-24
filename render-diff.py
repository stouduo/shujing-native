#!/usr/bin/env python3
"""逐像素对比两组渲染图(渲染测试落在 /tmp/shujing_ui 的 PNG)。

给"声称不改外观"的改动用:纯重构、token 化、组件搬迁。测试通过说明不了
外观没变——测试不锁像素。提交前跑一次,把"逐像素不变"从声明变成证据。

    # 1) 基准:在主工作树里检出改动前的提交,渲染,存起来
    git checkout <base>
    rm -f /tmp/shujing_ui/*.png
    cargo test -p shujing-app -- render_test --skip render_english_mode
    mkdir -p /tmp/rd/before && cp /tmp/shujing_ui/*.png /tmp/rd/before/
    # 2) 切回改动,同样渲染、存起来
    git checkout -
    rm -f /tmp/shujing_ui/*.png
    cargo test -p shujing-app -- render_test --skip render_english_mode
    mkdir -p /tmp/rd/after && cp /tmp/shujing_ui/*.png /tmp/rd/after/
    # 3) 对比
    python3 render-diff.py /tmp/rd/before /tmp/rd/after

踩过的坑,每一条都让"零差异"变成假结论:
- 差异必须取三个颜色通道的最大值。对 RGBA 图直接取包围盒不行:图像库对
  RGBA 默认只看 alpha,两张不透明图永远"零差异"
- 结论可信的前提是检测有信号:拿一个已知有差异的版本跑一遍,确认它会报
- 两次渲染都跳过 render_english_mode:旧提交里这个测试在锁外切全局语言,
  同一次运行里别的图会随机变成英文
- 渲染旧版本要在主工作树里切换提交,不要用临时 worktree 加共用的构建目录:
  旧版本的产物会留在缓存里被当成当前的
- 渲染前清空输出目录,历史残留的图会混进来

依赖 Pillow。
"""

import pathlib
import sys

from PIL import Image, ImageChops


def diff_one(a, b):
    """返回 (差异像素数, 差异包围盒);两张完全一致时为 (0, None)。"""
    ia, ib = Image.open(a).convert("RGB"), Image.open(b).convert("RGB")
    if ia.size != ib.size:
        return None, f"尺寸 {ia.size} vs {ib.size}"
    r, g, bl = ImageChops.difference(ia, ib).split()
    m = ImageChops.lighter(ImageChops.lighter(r, g), bl)
    bbox = m.getbbox()
    if not bbox:
        return 0, None
    return sum(m.histogram()[1:]), bbox


def main():
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    if len(args) != 2:
        sys.exit(__doc__)
    before, after = pathlib.Path(args[0]), pathlib.Path(args[1])
    names_b = {p.name for p in before.glob("*.png")}
    names_a = {p.name for p in after.glob("*.png")}
    common = sorted(names_b & names_a)
    same, differ = [], []
    for n in common:
        count, info = diff_one(before / n, after / n)
        (same if count == 0 else differ).append((n, count, info))
    print(f"公共图 {len(common)} 张:逐像素一致 {len(same)},有差异 {len(differ)}")
    for n, count, info in differ:
        print(f"  差异  {n}  {count if count is not None else info}"
              + (f"  范围 {info}" if count else ""))
    if names_b - names_a:
        print(f"仅基准有的图: {sorted(names_b - names_a)}")
    if names_a - names_b:
        print(f"仅改动后有的图: {sorted(names_a - names_b)}")
    # 有差异不一定是错:有意新增/调整的界面也会出现在这里,逐张确认后才能下结论
    sys.exit(1 if differ else 0)


if __name__ == "__main__":
    main()
