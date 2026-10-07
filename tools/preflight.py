#!/usr/bin/env python3
"""Preflight: lay every design sheet over the others and list what will fail or is unimplemented.

    python3 tools/preflight.py --milestone 1            # build gate for milestone 1
    python3 tools/preflight.py --milestone 1 --release  # also require every in-scope cell verified

Rows with milestone <= N are in scope: every required cell must be filled and every reference must
resolve to a row that is not from a later milestone. Rows above N are deferred: their unfilled cells are
listed (so you can see what is still unimplemented) but do not block. Exit code 0 = clean, 1 = blocked.
"""
import argparse
import json
import re
import sys
from pathlib import Path

SENTINELS = {"tbd", "?", "todo", "n/a?", "unknown"}
ID_RE = re.compile(r"^[a-z][a-z0-9_]*$")
PATH_TOKENS = ("{appdata}", "{game}", "{me2}", "{data}", "{sm2}")


def load_sheets(sheet_dir: Path):
    sheets = {}
    problems = []
    for path in sorted(sheet_dir.glob("*.json")):
        try:
            data = json.loads(path.read_text(encoding="utf-8"))
        except json.JSONDecodeError as e:
            problems.append(("syntax", f"{path.name}: invalid JSON: {e}"))
            continue
        for key in ("sheet", "columns", "rows"):
            if key not in data:
                problems.append(("syntax", f"{path.name}: missing '{key}'"))
                break
        else:
            sheets[data["sheet"]] = data
    return sheets, problems


def is_unfilled(value, col):
    if value is None:
        return True
    t = col["type"]
    if t == "text":
        return False
    if isinstance(value, str):
        return value.strip() == "" or value.strip().lower() in SENTINELS
    if t in ("list", "intlist", "reflist"):
        return isinstance(value, list) and len(value) < col.get("min", 1)
    return False


def type_problem(value, col, sheet_name, row_id, col_name, sheets):
    """Return a problem string if a *filled* value has the wrong type; None if fine."""
    t = col["type"]
    where = f"{sheet_name}.{row_id}.{col_name}"
    if t == "id":
        if not (isinstance(value, str) and ID_RE.match(value)):
            return f"{where}: id must match {ID_RE.pattern}, got {value!r}"
    elif t in ("string", "text"):
        if not isinstance(value, str):
            return f"{where}: expected string, got {type(value).__name__}"
    elif t == "int":
        if isinstance(value, bool) or not isinstance(value, int):
            return f"{where}: expected int, got {value!r}"
    elif t == "float":
        if isinstance(value, bool) or not isinstance(value, (int, float)):
            return f"{where}: expected number, got {value!r}"
    elif t == "bool":
        if not isinstance(value, bool):
            return f"{where}: expected true/false, got {value!r}"
    elif t == "enum":
        if value not in col.get("values", []):
            return f"{where}: {value!r} is not one of {col.get('values')}"
    elif t == "ref":
        if not isinstance(value, str):
            return f"{where}: ref must be a row id string"
    elif t in ("list", "reflist"):
        if not (isinstance(value, list) and all(isinstance(x, str) and x.strip() for x in value)):
            return f"{where}: expected a list of non-empty strings"
    elif t == "intlist":
        if not (isinstance(value, list) and all(isinstance(x, int) and not isinstance(x, bool) for x in value)):
            return f"{where}: expected a list of ints"
    else:
        return f"{where}: unknown column type {t!r}"
    return None


def row_milestone(sheet, row):
    return row.get("milestone", 0) if "milestone" in sheet["columns"] else 0


def check(sheets, milestone, release):
    blocking, deferred, unverified, stats = [], [], [], {}
    ids = {name: {} for name in sheets}  # sheet -> row id -> row

    # pass 1: ids
    for name, sheet in sheets.items():
        seen = ids[name]
        for row in sheet["rows"]:
            rid = row.get("id")
            if rid in seen:
                blocking.append(("dup-id", f"{name}.{rid}: duplicate id"))
            seen[rid] = row

    # pass 2: cells
    for name, sheet in sheets.items():
        cols = sheet["columns"]
        st = dict(rows=0, in_scope=0, deferred=0, unfilled=0, bad_refs=0, unverified=0)
        for row in sheet["rows"]:
            rid = row.get("id", "<no id>")
            ms = row_milestone(sheet, row)
            in_scope = ms <= milestone
            st["rows"] += 1
            st["in_scope" if in_scope else "deferred"] += 1

            for k in row:
                if k not in cols and k != "_verified":
                    blocking.append(("unknown-column", f"{name}.{rid}.{k}: column is not defined in the sheet"))

            for cname, col in cols.items():
                value = row.get(cname)
                where = f"{name}.{rid}.{cname}"
                required = col.get("required", True)
                if is_unfilled(value, col):
                    if required:
                        st["unfilled"] += 1
                        target = blocking if in_scope else deferred
                        target.append(("unfilled" if in_scope else "deferred", f"{where}" + ("" if in_scope else f" (milestone {ms})")))
                    continue
                tp = type_problem(value, col, name, rid, cname, sheets)
                if tp:
                    (blocking if in_scope else deferred).append(("type", tp))
                    continue
                # references
                if col["type"] in ("ref", "reflist"):
                    target_sheet = col.get("sheet")
                    refs = [value] if col["type"] == "ref" else value
                    for ref in refs:
                        if target_sheet not in sheets or ref not in ids[target_sheet]:
                            st["bad_refs"] += 1
                            (blocking if in_scope else deferred).append(("bad-ref", f"{where} -> {target_sheet}.{ref} does not resolve"))
                        else:
                            trow = ids[target_sheet][ref]
                            tms = row_milestone(sheets[target_sheet], trow)
                            if tms > ms:
                                st["bad_refs"] += 1
                                (blocking if in_scope else deferred).append(
                                    ("ref-to-later", f"{where} -> {target_sheet}.{ref} belongs to milestone {tms}, later than this row's {ms}"))

            # verification
            ver = row.get("_verified", [])
            if ver == "all":
                ver = list(cols)
            for cname in ver:
                if cname not in cols:
                    blocking.append(("unknown-column", f"{name}.{rid}._verified lists unknown column {cname}"))
            if in_scope:
                for cname in cols:
                    if cname not in ver:
                        st["unverified"] += 1
                        unverified.append(f"{name}.{rid}.{cname}")
        stats[name] = st

    # row-consistency rules (things the generator relies on)
    hooks = sheets.get("hooks", {"rows": []})
    for row in hooks["rows"]:
        if row_milestone(hooks, row) > milestone:
            continue
        rid, act = row.get("id"), row.get("action")
        n_args = len(row.get("args") or [])
        tmax = max(row.get("target_args") or [-1])
        if tmax >= n_args:
            blocking.append(("rule", f"hooks.{rid}: target_args index {tmax} but only {n_args} args"))
        if act == "custom" and row.get("custom_fn") in (None, "none"):
            blocking.append(("rule", f"hooks.{rid}: action custom needs custom_fn"))
        if act != "custom" and row.get("custom_fn") not in (None, "none"):
            blocking.append(("rule", f"hooks.{rid}: custom_fn only applies to action custom"))
        if act in ("deny_addr", "deny_name"):
            if row.get("deny_ret") in (None, "none") or row.get("error_api") == "none":
                blocking.append(("rule", f"hooks.{rid}: {act} needs deny_ret and an error_api"))
        if act == "deny_name" and row.get("name_kind") == "none":
            blocking.append(("rule", f"hooks.{rid}: deny_name needs name_kind utf16 or ansi"))
        if act == "redirect_paths" and (row.get("deny_ret") not in (None, "none") or row.get("name_kind") != "none"):
            blocking.append(("rule", f"hooks.{rid}: redirect_paths must not set deny_ret/name_kind"))
        for a in row.get("args") or []:
            if ":" not in a:
                blocking.append(("rule", f"hooks.{rid}: arg {a!r} is not 'name:Type'"))

    files = sheets.get("files", {"rows": []})
    for row in files["rows"]:
        if row_milestone(files, row) > milestone:
            continue
        if not str(row.get("path", "")).startswith(PATH_TOKENS):
            blocking.append(("rule", f"files.{row.get('id')}: path must start with one of {PATH_TOKENS}"))

    # every in-scope system must have at least one file or hook or artifact tying it to code
    systems = sheets.get("systems", {"rows": []})
    used = set()
    for sname in ("files", "hooks"):
        for row in sheets.get(sname, {"rows": []})["rows"]:
            used.add(row.get("system"))
    for row in systems["rows"]:
        if row_milestone(systems, row) <= milestone and row.get("id") not in used and row.get("id") not in ("fail_closed", "window_marker", "save_backup_verify_x"):
            blocking.append(("rule", f"systems.{row.get('id')}: no file or hook row uses this system (is it implemented anywhere?)"))

    return blocking, deferred, unverified, stats


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--milestone", type=int, required=True)
    ap.add_argument("--release", action="store_true", help="also block on unverified in-scope cells")
    ap.add_argument("--sheets", default=str(Path(__file__).resolve().parent.parent / "design" / "sheets"))
    ap.add_argument("--show-deferred", action="store_true")
    args = ap.parse_args()

    sheets, problems = load_sheets(Path(args.sheets))
    blocking, deferred, unverified, stats = check(sheets, args.milestone, args.release)
    blocking = problems + blocking

    print(f"PREFLIGHT  milestone <= {args.milestone}" + ("  (release: verification required)" if args.release else ""))
    print(f"{'sheet':<14}{'rows':>5}{'in':>5}{'later':>6}{'unfilled':>10}{'bad-refs':>10}{'unverified':>12}")
    for name, st in stats.items():
        print(f"{name:<14}{st['rows']:>5}{st['in_scope']:>5}{st['deferred']:>6}{st['unfilled']:>10}{st['bad_refs']:>10}{st['unverified']:>12}")
    print()

    if blocking:
        print(f"BLOCKING ({len(blocking)}):")
        for kind, msg in blocking:
            print(f"  [{kind}] {msg}")
        print()
    if args.release and unverified:
        print(f"UNVERIFIED in-scope cells ({len(unverified)}) - block a release:")
        for u in unverified:
            print(f"  [unverified] {u}")
        print()
    if deferred:
        print(f"DEFERRED, not blocking this build ({len(deferred)} cells in later milestones" + ("" if args.show_deferred else "; --show-deferred to list") + ")")
        if args.show_deferred:
            for kind, msg in deferred:
                print(f"  [{kind}] {msg}")
        print()

    bad = len(blocking) + (len(unverified) if args.release else 0)
    if bad:
        print(f"RESULT: BLOCKED ({bad} problems). Fix the sheets first, then rebuild.")
        return 1
    print("RESULT: CLEAN" + ("" if args.release else f"  (unverified in-scope cells: {len(unverified)}; run with --release before shipping)"))
    return 0


if __name__ == "__main__":
    sys.exit(main())
