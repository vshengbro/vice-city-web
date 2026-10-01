"""Static check for the unbound-loop-variable shape that broke a real run.

`walk_to` reads `samples` / `stopped` after an inner `for` that can `break`
before assigning them. CPython turns that into UnboundLocalError at run
time -- in the middle of a four-minute browser walk, which is the worst
possible moment to learn it. This walks the AST instead.

Rule: a name that is only ever assigned inside a `for`/`async for` body and
is also read in the same function *outside* that loop must have been
initialised before the loop.
"""
import ast

BASE = "/Users/sqs/.hermes/cache/scratch/vcw-cdp/"


def loop_vars(fn, loop):
    """Names assigned inside the loop body (NOT its target).

    The target of `for name, ok, note in results` is bound on every
    iteration by the `for` itself, so it can never be the unbound-variable
    bug -- counting it flagged every ordinary `for _, x in ...: use(x)`.
    """
    out = {}
    for node in ast.walk(loop):
        if isinstance(node, ast.Assign):
            for t in node.targets:
                if isinstance(t, ast.Name):
                    out.setdefault(t.id, node.lineno)
        elif isinstance(node, (ast.AnnAssign, ast.AugAssign)):
            t = node.target
            if isinstance(t, ast.Name):
                out.setdefault(t.id, node.lineno)
    targets = set()
    tgt = loop.target
    for n in ast.walk(tgt):
        if isinstance(n, ast.Name):
            targets.add(n.id)
    for name in targets:
        out.pop(name, None)
    return out


def guarded(fn, loop, name, alineno):
    """Is the name assigned unconditionally BEFORE the loop, in the same block?

    Two things produce a false positive if ignored:
      * `gap = ...` on the line before its use -- the loop is not what
        binds it, so it is always safe;
      * a second, unrelated loop that happens to use the same name, as in
        `walk_to`, which has three `for` loops and reuses `gap`.
    So compare line numbers: a read that sits at or after the loop's last
    assignment line, in a block that is not inside the loop, is the only
    real risk.
    """
    for node in ast.walk(fn):
        targets = []
        if isinstance(node, ast.Assign):
            targets = [t for t in node.targets if isinstance(t, ast.Name)]
        elif isinstance(node, (ast.AnnAssign, ast.AugAssign)):
            t = node.target
            targets = [t] if isinstance(t, ast.Name) else []
        for t in targets:
            if t.id != name or node.lineno >= loop.lineno:
                continue
            if loop.lineno <= node.lineno <= (loop.end_lineno or 0):
                continue
            return True
    return False


def enclosing_blocks(fn, loop):
    """Statements that lexically CONTAIN the loop, outermost first.

    This is the right frame for scoping. The real bug reads `samples` in
    the body of the outer `for step` loop, after the inner
    `for _ in range(2)` that assigns it -- so the read sits in an
    *enclosing* block, which is exactly the case a `break` can leave
    unbound. Conversely `main()`'s two unrelated `ok` variables are in
    *sibling* top-level blocks and must never be paired.
    """
    parents = {}
    for node in ast.walk(fn):
        for child in ast.iter_child_nodes(node):
            parents[id(child)] = node

    chain, cur = [], parents.get(id(loop))
    while cur is not None and isinstance(
            cur, (ast.For, ast.AsyncFor, ast.While, ast.If,
                  ast.With, ast.AsyncWith, ast.Try)):
        chain.append(cur)
        cur = parents.get(id(cur))
    return chain


def in_scope(fn, loop, lineno):
    """Does `lineno` sit in the SAME statement that directly holds the loop?

    Ancestry alone is not enough. In `main()` the teleport loop and the
    summary loop are both statements of one `async with`, so a read in the
    summary block "encloses" the teleport loop too -- pairing them by
    ancestry invents a bug between two unrelated `ok` variables. What
    actually makes a read at risk is that it belongs to the same block the
    loop hangs off: that is the block a `break` falls back into.

    So: find the innermost enclosing block `B` of the loop, and require the
    read to be inside `B` and after the loop. Sibling blocks are excluded
    even though they share a parent.
    """
    blocks = enclosing_blocks(fn, loop)
    if not blocks:
        return False
    inner = blocks[0]
    return (inner.lineno or 0) <= lineno <= (inner.end_lineno or 0)


def outside_reads(fn, loop, name):
    """Reads of `name` AFTER the loop, in a block that encloses it."""
    last = loop_end(loop)
    return [n.lineno for n in ast.walk(fn)
            if isinstance(n, ast.Name) and n.id == name
            and isinstance(n.ctx, ast.Load) and n.lineno > last
            and in_scope(fn, loop, n.lineno)]


def loop_end(loop) -> int:
    """Last line covered by the loop body, for the `inside` test."""
    return max((s.end_lineno or 0) for s in loop.body)


def assigned_outside(fn, loop, name):
    """Does the function assign `name` anywhere the loop cannot skip?

    This is the discriminator between the real bug and the noise:
      * `walk_to` reuses `gap` in three loops, but the slide loop's `gap`
        is assigned on the line above its use, outside every loop -- safe.
      * the `samples` / `stopped` bug had assignments ONLY inside a loop
        and a read after that loop -- the first `break` skipped them.
    """
    holder_blocks = enclosing_blocks(fn, loop)
    if not holder_blocks:
        return True
    for node in ast.walk(fn):
        targets = []
        if isinstance(node, ast.Assign):
            targets = [t for t in node.targets if isinstance(t, ast.Name)]
        elif isinstance(node, (ast.AnnAssign, ast.AugAssign)):
            t = node.target
            targets = [t] if isinstance(t, ast.Name) else []
        for t in targets:
            if t.id != name:
                continue
            if loop.lineno <= node.lineno <= loop_end(loop):
                continue  # inside the loop: a `break` can skip it
            return True
    return False


bad = []
for fname in ("full_sweep.py", "full_sweep_lib.py"):
    tree = ast.parse(open(BASE + fname).read())
    for fn in ast.walk(tree):
        if not isinstance(fn, (ast.AsyncFunctionDef, ast.FunctionDef)):
            continue
        for loop in ast.walk(fn):
            if not isinstance(loop, (ast.For, ast.AsyncFor)):
                continue
            for name, alineno in loop_vars(fn, loop).items():
                reads = outside_reads(fn, loop, name)
                if reads and not assigned_outside(fn, loop, name):
                    bad.append(f"{fname}:{reads[0]} {fn.name}(): "
                               f"{name!r} assigned only inside the loop "
                               f"(line {alineno}) and read after it")

if bad:
    print("UNBOUND LOOP VARIABLES (advisory):")
    for b in bad:
        print("  " + b)
    print("note: sibling loops under one block can pair unrelated names;")
    print("      confirm each against the source before acting.")
else:
    print("ok   no name is read after a loop that can skip its assignment")
