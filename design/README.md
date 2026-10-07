# Design sheets

The sheets in `sheets/` are the **source of truth**. Code is generated from them (`tools/gen.py`) and
checked against them (`tools/preflight.py`). When anything is unclear, go back to the sheets; change the
sheet **before** the code.

## Shape

Every sheet is one JSON file:

```json
{
  "sheet": "hooks",
  "doc": "what this sheet covers",
  "columns": { "id": {"type": "id"}, "module": {"type": "string"}, ... },
  "rows": [ { "id": "fs_createfilew", "module": "kernelbase.dll", ... } ]
}
```

Each **row** is one thing (a hook, a file, a weapon). Each **column** is one property of it. Every game
system the mashup touches has a row somewhere. Each row generates one piece of code built from its columns.

Column types: `id`, `string`, `text` (may be empty), `int`, `float`, `bool`, `enum` (`values`), `ref`
(`sheet`: the id of a row in another sheet), `list` (list of strings), `reflist`. Add `"required": false`
for optional columns.

## Cells are checkboxes

Every (row, column) crossing is a checkbox with two ticks:

1. **filled**: the cell has a real value (not missing, `null`, `""`, `"TBD"` or `"?"`);
2. **verified**: someone confirmed it against the source of truth or an oracle (a test, a log, a game
   file). A row lists what is verified in `"_verified": ["col", ...]` or `"_verified": "all"`.

A row with an unfilled or unverified cell is unfinished.

## Milestones

Rows carry a `milestone` (0 = design, 1 = safe sandbox, 2 = read SM2 content, 3 = weapons in DS3,
4 = release). `preflight.py --milestone N` treats rows at or below N as **in scope**: they must be fully
filled and every reference must resolve (a row may not depend on a later milestone). Rows above N are
**deferred**: their unfilled cells are listed so you can see what is still unimplemented, but they do not
block the build.

## Preflight before every build

```
python3 tools/preflight.py --milestone 1          # must be clean before building milestone 1
python3 tools/preflight.py --milestone 1 --release  # additionally requires every in-scope cell verified
python3 tools/gen.py                                # regenerate code from the sheets
python3 tools/gen.py --check                        # fail if generated code is stale
```

## Path tokens

`{appdata}` Roaming AppData · `{game}` the Dark Souls III folder Melty passes · `{me2}` ModEngine2's folder ·
`{data}` the mashup's own folder (`{managed}/ashenmarine`) · `{sm2}` the Space Marine 2 folder the mashup
finds itself.
