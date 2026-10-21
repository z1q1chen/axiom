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
item      := use | struct | impl | fn | let     // top level
use       := "use" STRING ";"
struct    := "struct" IDENT "{" field* invariant* "}"
field     := IDENT ":" type ";"
invariant := "invariant" expr ";"
impl      := "impl" IDENT "{" fn* "}"           // methods (v0.3)
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

## Methods and interpolation (v0.3)

Methods are declared in `impl StructName { ... }` blocks. The
first parameter must be `self`; written bare (`fn m(self, x: int)`)
it takes the impl's struct type. Calls `obj.m(args)` resolve
through the impl table; contracts and invariant checks behave
exactly as for free functions (a field assignment to `self`
re-checks the struct invariant).

String literals interpolate: `"total: {inv.total}"` desugars to
concatenation with `str(...)` around each braced expression.
`{{` and `}}` are literal braces; a lone `}` is a parse error.
Braces nest and may contain string literals.

## Generics (v0.4)

Generic structs (`struct Pair<A, B> { ... }`) and generic
functions (`fn first<T>(xs: [T]) -> T`) are monomorphized: each
concrete instantiation is checked and generated as its own
struct or function (`Pair__int__string`, `first__int`), with the
template's invariants and contracts re-checked per instantiation.

Type arguments come from, in order: explicit arguments
(`Pair<int, string> { ... }`, `first<string>(words)`), the
expected type at the use site (annotations, argument positions,
return position), or inference from field values and argument
types. Operations on a bare type parameter are limited to what
every type supports (passing, storing, `==`, `len` on containers
of it); there are no trait bounds yet, so `fn sum<T>(xs: [T])`
cannot add elements. Generic methods and methods on generic
structs are not supported yet.

## Roadmap (not in v0.1)

- Capability declarations with binding to pre-verified
  implementations (the proposal's capability layer).
- Contract tiers above runtime-checked: bounded model checking of
  small pure functions; proof for a tiny critical core.
- Freeing memory (a real allocator policy), separate compilation.
- Trait bounds for generic parameters; generic methods.
