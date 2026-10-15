#!/usr/bin/env python3.11
"""verify8 worker 的静态自检(不连服务器、不起 Chrome)。

三类坑都是**实跑才暴露**的,而它们全都不会被 `ast.parse` 或 `import`
抓到 —— 所以要在开跑前扫一遍:

1. **`await <协程>[i]` / `await <协程>.attr`** —— 在对协程结果下标或取属性,
   运行到那一行才炸 `TypeError: 'coroutine' object is not subscriptable`。
   实测在 w5 犯过两次、w8 一次,都是「跑到一半才 harness failure」,
   把前面已经跑出来的真判定一起吞掉。
2. **函数自调用** —— 批量替换代码时把 `await s.st()` 一起换成了
   `await _st(s)`,结果 `_st` 内部调自己,`RecursionError`。
   树形递归(带 depth 参数)是合法的,所以要区分开。
3. **f-string 里对可能为 None 的读数直接 `{x:.2f}`** —— `st()` 的字段
   缺失时返回 None,`{None:.2f}` 抛 `TypeError: unsupported format string
   passed to NoneType.__format__`。同理,拿读数做除法时分母可能是 0。

只报不修。它只输出问题清单,由人决定改不改。

用法:`python3.11 _static_check.py tools/verify8/w1_walk.py`
退出码 0 = 干净,1 = 有发现(verify8.sh 只当 WARN,不阻断)。
"""

from __future__ import annotations

import ast
import re
import sys


def check(path: str) -> list[str]:
    tree = ast.parse(open(path, encoding="utf-8").read())
    out: list[str] = []

    # 1) await 结果被下标 / 取属性
    for node in ast.walk(tree):
        if isinstance(node, ast.Subscript) and isinstance(node.value, ast.Await):
            out.append(f"L{node.lineno}: await 结果被下标(应为 `(await f(x))[i]`)")
        # `await x.y` 里 .get() 之类要放行:常见的 `(await _rich(s)).get(...)`
        # 是合法写法,只在链式继续取属性时报。
        if isinstance(node, ast.Attribute) and isinstance(node.value, ast.Await):
            if node.attr in {"get", "keys", "items", "values"}:
                continue
            out.append(f"L{node.lineno}: await 结果取属性 .{node.attr}")

    # 2) 函数无 depth 参数却自调用
    for node in ast.walk(tree):
        if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)):
            args = [a.arg for a in node.args.args]
            for sub in ast.walk(node):
                if (
                    isinstance(sub, ast.Call)
                    and isinstance(sub.func, ast.Name)
                    and sub.func.id == node.name
                    and "depth" not in args
                ):
                    out.append(f"L{sub.lineno}: `{node.name}` 无 depth 参数却自调用(无限递归)")

    # 3) f-string 里对 .get(...) 的结果直接用格式说明符
    for node in ast.walk(tree):
        if not isinstance(node, ast.JoinedStr):
            continue
        for part in node.values:
            if not isinstance(part, ast.FormattedValue):
                continue
            if part.format_spec is None:
                continue
            src = ast.unparse(part.value)
            if ".get(" not in src or "_f(" in src:
                continue
            # `.get(k, 0)` / `.get(k, default)` 带默认值,不会产出 None。
            if re.search(r"\.get\([^)]*,[^)]*\)", src):
                continue
            out.append(
                f"L{node.lineno}: f-string 对 {src[:44]} 用格式说明符,"
                f"读数为 None 时会 TypeError(用 _f())"
            )

    # 4) 除法分母可能是 0 的读数
    for node in ast.walk(tree):
        if isinstance(node, ast.BinOp) and isinstance(node.op, ast.Div):
            src = ast.unparse(node)
            if ".get(" in src and "max(" not in src and "len(" not in src:
                out.append(f"L{node.lineno}: 除法分母可能为 0({src[:50]})")

    return out


def main() -> int:
    if len(sys.argv) < 2:
        print("用法: _static_check.py <worker.py> [...]", file=sys.stderr)
        return 2
    clean = True
    for path in sys.argv[1:]:
        try:
            issues = check(path)
        except SyntaxError as exc:
            print(f"  {path}: 语法错误 {exc}")
            clean = False
            continue
        for issue in issues:
            print(f"  {path.split('/')[-1]}:{issue}")
        if issues:
            clean = False
    return 0 if clean else 1


if __name__ == "__main__":
    sys.exit(main())
