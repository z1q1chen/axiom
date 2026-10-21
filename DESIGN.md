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
`del(m, k)` (returns the map), `split(s, sep)` (array of strings),
`join(parts, sep)`, `replace(s, from, to)`, `substr(s, start, n)`
(clamped to bounds), `index_of(s, sub)` (-1 when absent),
`starts_with(s, prefix)`.

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

## Extern functions and capabilities (v0.5)

`extern fn` declares a function whose body is a native
implementation in the runtime (`axn_<name>` in
`runtime/axiom_rt.c`). The compiler refuses to build when an
extern has no native implementation. Contracts on an extern are
checked by the generated wrapper on every call, before and after
the native code runs: the native is trusted to exist and perform,
never to respect the contract silently.

`capability Name { extern fn ...; ... }` groups externs under a
name. The ledger records each binding, and the contracts on it,
at tier **bound** (the implementation is bound native code; its
contracts are runtime-checked at the boundary).

Shipped natives: capability `file` (read_file, write_file,
append_file, file_exists) and `io` (read_line, env_get). Builtins
`join` and `replace` also arrived in v0.5.

## Bounded verification (v0.6)

`axiomc verify <file>` is the first tier above runtime-checked.
For every eligible function it generates a harness that calls the
function on every combination of a bounded input domain, in a
forked child per case. Inputs that fail `requires` are excluded;
a case that passes `requires` and then fails any contract (or
crashes) is a counterexample, printed with its concrete inputs.

Eligible: pure functions (no extern calls, no print/raise, no
mutation of globals, fields, arrays, or maps, directly or through
callees; purity is computed to a fixpoint over the call graph)
with only int/bool parameters and int/bool/void returns. The int
domain is -3..3 plus, per parameter, every literal (and its
neighbours) that the function's `requires` compares it against.
Combinations are capped (domains shrink to -1..1 past 20,000
combinations; beyond that the function is reported skipped).

`verify` writes `<program>.verify.json`: functions with zero
counterexamples are recorded at tier **bounded** — proven for
the stated domain, not for all inputs — and the rest stay
runtime-checked. Functions that are not eligible are absent
from the report and stay runtime-checked too.

## Separate compilation (v0.8)

`axiomc build --separate` emits one C file per Axiom source
file under `<binary>.separate/`, compiles each to its own
object, and links them. Generated functions, invariant
checkers, and globals get external linkage through a generated
header (`axiom_gen.h`); each module also carries a private copy
of the runtime helpers, which are stateless (values are plain
malloc'd memory), so the copies interoperate. The checker still
runs on the whole program: module interfaces are the checked
program itself, not per-module signatures, so changing a shared
type still rechecks everything even though only changed
modules need their C recompiled (the driver currently rebuilds
all objects; it does not do make-style staleness).

## Roadmap (not in v0.1)

- Proof for a tiny critical core (the tier above bounded).
- Freeing memory (a real allocator policy).
- Trait bounds for generic parameters; generic methods.
