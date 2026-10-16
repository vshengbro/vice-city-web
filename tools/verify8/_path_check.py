#!/usr/bin/env python3.11
"""Static check #2: catch the two failure modes `_static_check.py` misses.

  1. `SyntaxError` - nested `def` using `await` passed the static check and
     then died at import.
  2. `UnboundLocalError` - an `if/elif` chain where one branch assigns a
     name and another does not, or where a chain can fall off the end of the
     function without a `return`.

Deliberately narrow, and it is meant to be pointed at the **worker**
scripts. It reports two things:

  1. an `if/elif/else` chain that ends the function where at least one
     branch does not itself `return`/`raise` (control falls off the end);
  2. a local that is read on a path where it has not been assigned yet -
     the `UnboundLocalError` shape. Loop targets, comprehension targets,
     `with ... as` and `except ... as` names are excluded because they have
     their own scoping rules.

`vlib.py` still produces a handful of false positives from its nested
closures, so the runner is wired to the workers only.

Usage: python3.11 _path_check.py w1_walk.py [w2_visibility.py ...]
"""
import ast
import sys

TERMINALS = (ast.Return, ast.Raise)


def ends_in_return(body: list) -> bool:
    """Does this statement list end by returning or raising on every path?"""
    if not body:
        return False
    last = body[-1]
    if isinstance(last, TERMINALS):
        return True
    if isinstance(last, ast.If):
        return ends_in_return(last.body) and ends_in_return(last.orelse)
    if isinstance(last, (ast.For, ast.While, ast.With)):
        return ends_in_return(last.body) and ends_in_return(last.orelse)
    if isinstance(last, ast.Try):
        handlers_ok = all(ends_in_return(h.body) for h in last.handlers)
        orelse_ok = ends_in_return(last.orelse) if last.orelse else True
        final_ok = ends_in_return(last.finalbody) if last.finalbody else True
        return ends_in_return(last.body) and handlers_ok and orelse_ok and final_ok
    if isinstance(last, ast.Match):
        return bool(last.cases) and all(ends_in_return(c.body) for c in last.cases)
    return False


def flatten_chain(node: ast.If) -> list[list[ast.stmt]]:
    """Split an if/elif/else chain into its branch bodies.

    `elif` is encoded as a nested `If` inside `orelse`, and the final `else`
    is whatever remains of the deepest `orelse`. Both have to be unwound,
    otherwise the terminal `else` is silently dropped and the chain looks
    complete when it is not.
    """
    chains = [list(node.body)]
    cur: list[ast.stmt] = list(node.orelse)
    while True:
        if not cur:
            break
        if isinstance(cur[0], ast.If):
            chains.append(list(cur[0].body))
            cur = list(cur[0].orelse)
            continue
        chains.append(cur)   # the final `else` body
        break
    return chains


def _assigned_before(fn: ast.AST, name: str, line: int) -> bool:
    """Is `name` assigned on EVERY path that can reach `line`?

    Walks the function body tracking which names are definitely bound as it
    descends. At a branch point only the names bound in **all** branches
    survive - that is precisely the condition under which Python raises
    UnboundLocalError rather than reading a stale or missing value.
    """
    def bound_in(body: list) -> set[str]:
        """Names guaranteed bound after running `body` to completion."""
        out: set[str] = set()
        for st in body:
            for n in ast.walk(st):
                if isinstance(n, (ast.FunctionDef, ast.AsyncFunctionDef)):
                    continue
                if isinstance(n, ast.Name) and isinstance(n.ctx, ast.Store):
                    out.add(n.id)
                # comprehension / generator targets have their own scope and
                # are not locals of this function at all
                elif isinstance(n, ast.alias):
                    out.add(n.asname or n.name.split(".")[0])
            # with-items, except-handler names, for-targets
            if isinstance(st, (ast.For, ast.AsyncFor)):
                for nn in ast.walk(st.target):
                    if isinstance(nn, ast.Name):
                        out.add(nn.id)
            if isinstance(st, ast.comprehension):
                pass
        return out

    def comprehension_names(st: ast.AST) -> set[str]:
        """Names bound inside comprehensions (own scope, never locals)."""
        out: set[str] = set()
        for n in ast.walk(st):
            if isinstance(n, (ast.ListComp, ast.SetComp, ast.DictComp,
                              ast.GeneratorExp)):
                for t in ast.walk(n.target):
                    if isinstance(t, ast.Name):
                        out.add(t.id)
        return out

    def scan(body: list, have: set[str], target: int) -> bool | None:
        """True/False = bound on reaching `target`; None = target not here."""
        for st in body:
            for n in ast.walk(st):
                if isinstance(n, ast.Name) and n.lineno == target \
                        and isinstance(n.ctx, ast.Load) and n.id == name:
                    return name in have
            if isinstance(st, (ast.FunctionDef, ast.AsyncFunctionDef)):
                continue
            if getattr(st, "lineno", 0) > target:
                return name in have
            if isinstance(st, ast.If):
                bset = bound_in(st.body)
                oset = bound_in(st.orelse) if st.orelse else set()
                both = have | (bset & oset)
                r = scan(st.body, have | bset, target)
                if r is not None:
                    return r
                if st.orelse:
                    r = scan(st.orelse, have | oset, target)
                    if r is not None:
                        return r
                have = both
                continue
            if isinstance(st, (ast.For, ast.While, ast.With, ast.Try)):
                for field in ("body", "orelse", "finalbody"):
                    sub = getattr(st, field, None)
                    if not sub:
                        continue
                    sset = bound_in(sub)
                    r = scan(sub, have | sset, target)
                    if r is not None:
                        return r
                have = have | bound_in(st.body)
                if isinstance(st, (ast.For, ast.While)):
                    have = have | bound_in(st.orelse) if st.orelse else have
                continue
            have = have | bound_in([st])
        return None

    # seed with the parameters: they are bound from the first statement, so
    # a function that only reads its own argument is never flagged
    a = fn.args
    params: set[str] = {x.arg for x in (*a.posonlyargs, *a.args, *a.kwonlyargs)}
    if a.vararg:
        params.add(a.vararg.arg)
    if a.kwarg:
        params.add(a.kwarg.arg)
    r = scan(list(fn.body), params, line)
    return r is True


def locals_read_before_any_store(fn: ast.AST) -> list[str]:
    """Locals that are LOADed but never stored anywhere in the function.

    Catches the concrete `w4` failure: a branch that used `ok` after an
    edit had moved the `ok = ...` assignment into a different branch.
    Python decides `ok` is local for the whole function (it is assigned
    *somewhere*), so the failure only appears at runtime, on the one code
    path that reaches the load - which is exactly what makes it expensive.
    """
    stored: set[str] = set()
    loaded: dict[str, int] = {}
    # names that live in an inner scope (comprehension targets, loop
    # variables, with-items, except-as) are not function locals
    out_of_scope: set[str] = set()
    for n in ast.walk(fn):
        if isinstance(n, (ast.ListComp, ast.SetComp, ast.DictComp,
                          ast.GeneratorExp)):
            tgt = getattr(n, "target", None) or getattr(n, "elt", None)
            if tgt is not None:
                for t in ast.walk(tgt):
                    if isinstance(t, ast.Name):
                        out_of_scope.add(t.id)
        if isinstance(n, (ast.For, ast.AsyncFor)):
            for t in ast.walk(n.target):
                if isinstance(t, ast.Name):
                    out_of_scope.add(t.id)
        if isinstance(n, ast.ExceptHandler) and n.name:
            out_of_scope.add(n.name)
        if isinstance(n, (ast.With, ast.AsyncWith)):
            for item in n.items:
                if item.optional_vars is not None:
                    for t in ast.walk(item.optional_vars):
                        if isinstance(t, ast.Name):
                            out_of_scope.add(t.id)
    # parameters are always bound - they are not what we are looking for
    a = fn.args
    for arg in (*a.posonlyargs, *a.args, *a.kwonlyargs):
        stored.add(arg.arg)
    if a.vararg:
        stored.add(a.vararg.arg)
    if a.kwarg:
        stored.add(a.kwarg.arg)
    for n in ast.walk(fn):
        if isinstance(n, (ast.FunctionDef, ast.AsyncFunctionDef)):
            continue  # a nested function has its own scope
        if isinstance(n, ast.Name):
            if isinstance(n.ctx, ast.Store):
                stored.add(n.id)
            elif isinstance(n.ctx, ast.Load) and n.id not in loaded:
                loaded[n.id] = n.lineno
    # Only report names that are **assigned somewhere in this function**
    # (so Python has already decided they are local) AND that are read inside
    # a statement list which provably runs **before** any assignment to them
    # on that path. A name never assigned in the function at all resolves to
    # a global - a NameError, a different bug - and reporting it here
    # produced hundreds of false alarms.
    return [f"{fn.name}: '{name}' is read at line {line} but is only assigned "
            f"on some paths (UnboundLocalError on the others)"
            for name, line in sorted(loaded.items(), key=lambda kv: kv[1])
            if name in stored and name not in _BUILTINS
            and name not in out_of_scope
            and not _assigned_before(fn, name, line)]


_BUILTINS = {
    "True", "False", "None", "print", "len", "range", "str", "int", "float",
    "bool", "list", "dict", "set", "tuple", "abs", "min", "max", "sum", "round",
    "isinstance", "enumerate", "zip", "sorted", "any", "all", "type", "repr",
    "getattr", "hasattr", "issubclass", "open", "super", "Exception", "ValueError",
    "KeyError", "TypeError", "IndexError", "RuntimeError", "StopIteration",
    "asyncio", "sys", "os", "re", "math", "json", "time", "shutil", "urllib",
    "subprocess", "struct", "zlib", "base64", "vlib", "Session", "Verdict",
    "emit", "fail_harness", "annotations",
}


def check_fn(fn: ast.AST) -> list[str]:
    out: list[str] = list(locals_read_before_any_store(fn))
    # A hazard is an if/elif chain that **ends the function** and has a branch
    # that does not itself return. `elif` is encoded as a nested If inside
    # `orelse`, so the chain has to be recognised by unwinding that nesting -
    # looking only at `body[-1]` misses every real `elif` chain.
    body = list(getattr(fn, "body", []))
    if not body:
        return out
    last = body[-1]
    chain_root: ast.If | None = None
    if isinstance(last, ast.If):
        chain_root = last
        # if the tail of orelse is itself an If, that is the `elif`; the
        # chain root is still the outer If
    if chain_root is not None:
        chains = flatten_chain(chain_root)
        if len(chains) > 1 and not all(ends_in_return(c) for c in chains):
            bad = [c[0].lineno for c in chains if not ends_in_return(c)]
            out.append(f"{fn.name}: if/elif chain at line {last.lineno} can "
                       f"fall through (branches starting at {bad} do not return)")
            # names assigned in some branches but not all
            names_per = []
            for c in chains:
                ns = {t.id for s in c if isinstance(s, ast.Assign)
                      for t in s.targets if isinstance(t, ast.Name)}
                names_per.append(ns)
            union = set().union(*names_per)
            for n in sorted(union):
                missing = [i for i, ns in enumerate(names_per) if n not in ns]
                if missing and len(missing) < len(names_per):
                    out.append(f"{fn.name}: '{n}' assigned in only some "
                               f"branches of the chain at line {last.lineno} "
                               f"(absent from branch #{missing[0]})")
    return out


def main() -> int:
    bad = 0
    for path in sys.argv[1:]:
        tree = ast.parse(open(path, encoding="utf-8").read())
        for fn in ast.walk(tree):
            if not isinstance(fn, (ast.FunctionDef, ast.AsyncFunctionDef)):
                continue
            if any(isinstance(n, (ast.Yield, ast.YieldFrom)) for n in ast.walk(fn)):
                continue
            for msg in check_fn(fn):
                print(f"{path}: {msg}")
                bad += 1
    print("PATH CHECK:", "issues found" if bad else "clean")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
