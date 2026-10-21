# Axiom v0.1 Design

## Pipeline

```
.ax source ──▶ lexer ──▶ parser ──▶ AST ──▶ type checker
          ──▶ C codegen (GNU statement expressions) ──▶ cc ──▶ binary
          └─▶ <program>.ledger.json  (every contract, with tier)
```

Semantics notes: structs and arrays have reference semantics in
v0.1 (assignment and parameter passing share the object; field and
element mutation is visible to all holders). Scalars are values.

Runtime representation is a tagged value (`AxVal`): the static type
checker proves the program well-typed, the C runtime carries values
in one uniform representation. Memory is arena-style (allocated,
never freed) in v0.1; programs are batch jobs.

## Types

`int` (i64), `float` (f64), `bool`, `string`, `void`, arrays `[T]`,
maps `{K: V}` (K must be `int` or `string`), and declared structs.
No implicit numeric conversion; use the `int()` / `float()` builtins.

Map semantics: reading a missing key is a runtime error (use `has`
to test first). Iteration order is insertion order; `for k in m`
iterates keys. Maps, like arrays and structs, are shared by
reference.

## Grammar sketch

```
program   := item*
item      := use | struct | fn | let            // top level
use       := "use" STRING ";"
struct    := "struct" IDENT "{" field* invariant* "}"
field     := IDENT ":" type ";"
invariant := "invariant" expr ";"
fn        := "fn" IDENT "(" params? ")" ("->" type)? block
block     := "{" contract* stmt* "}"
contract  := ("requires" | "ensures") expr ";"
stmt      := let | assign | field_assign | if | while | for
           | return | break | continue | check | expr ";"
for       := "for" IDENT "in" expr block
expr      := or
or        := and ("||" and)*
and       := eq ("&&" eq)*
eq        := cmp (("==" | "!=") cmp)*
cmp       := add (("<"|"<="|">"|">=") add)*
add       := mul (("+"|"-") mul)*
mul       := unary (("*"|"/"|"%") unary)*
unary     := ("!"|"-") unary | postfix
postfix   := primary ( "(" args ")" | "." IDENT | "[" expr "]" )*
primary   := INT | FLOAT | STRING | "true" | "false" | IDENT
           | "(" expr ")" | "[" elems "]" | IDENT "{" fields "}"
```

`//` line comments and `/* */` block comments. Entry point is
`fn main()`.

## Contract semantics (v0.1, exact)

- `requires e;` — checked on function entry. On failure the program
  stops with the function name, the clause text, and the source
  location. May reference parameters only.
- `ensures e;` — checked on every return path, after the return
  value is computed. May reference parameters and `result`.
  Functions with `ensures` must declare a non-void return type.
- `invariant e;` (inside a struct) — checked (a) after a struct
  literal is constructed, and (b) after every field assignment to a
  value of that struct type. May reference `this.field` via bare
  field names... in v0.1, invariants reference fields by bare name
  (`balance >= 0`), resolved against the struct being defined.
- `check e;` — a plain runtime assertion statement, usable anywhere.
  Not recorded in the ledger.

A violated contract exits with code 70 and a message on stderr.

## Verification ledger

Every `axiomc build` / `run` writes `<output>.ledger.json`:

```json
{
  "program": "ledger",
  "contracts": [
    {"kind": "requires", "owner": "fn post_entry", "text": "debits == credits",
     "file": "examples/ledger/journal.ax", "line": 12,
     "tier": "runtime-checked"}
  ]
}
```

Tier vocabulary, kept honest: v0.1 only ever emits
`runtime-checked`. `proven` and `bounded` exist in the vocabulary
for the roadmap and are never claimed by this compiler.

## Builtins

`print(...)` (space-separated, newline at end), `len(x)`,
`push(arr, v)` (mutates, returns the array), `range(n)` /
`range(a, b)` (array of ints), `str(x)`, `int(x)`, `float(x)`,
`raise(msg)` (stop with a message), `has(m, k)`, `keys(m)`,
`del(m, k)` (returns the map), `split(s, sep)` (array of strings).

## Equality

`==` compares ints, floats, bools by value and strings by content.
Arrays and structs compare by identity in v0.1.

## Roadmap (not in v0.1)

- String interpolation, methods on structs, generics.
- Capability declarations with binding to pre-verified
  implementations (the proposal's capability layer).
- Contract tiers above runtime-checked: bounded model checking of
  small pure functions; proof for a tiny critical core.
- Freeing memory (a real allocator policy), separate compilation.
