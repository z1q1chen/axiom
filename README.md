# Axiom

A contract-first programming language. You state what must be true;
the compiler turns those statements into checks that travel with the
program, and records every one of them in a **verification ledger**.

- Compiler (`axiomc`) written in Rust, emits C (GNU dialect),
  builds with the system C compiler.
- Static types, runtime-checked contracts: `requires`, `ensures`,
  struct `invariant`.
- Every contract lands in `<program>.ledger.json` with its kind,
  location, source text, and evidence tier.

## Status

v0.9, built 2026-10-21. A working language, not the full design
proposal: the proof tier is on the roadmap in `DESIGN.md`, not in
this build. Contracts are runtime-checked (bound for externs,
bounded where `axiomc verify` has exhausted a domain), and the
ledger and verify report say exactly that.

v0.9 adds an opt-in garbage collector (`--gc`): precise
mark-sweep over kind-tagged allocations, rooted by generated
shadow stacks, collecting only at statement boundaries. v0.8
added separate compilation (`axiomc build --separate`): one
C file and object per Axiom source file, linked through a
generated header. v0.6 added `axiomc verify`: bounded exhaustive checking of pure
int/bool functions, with counterexamples and a verify report at
tier `bounded`. See `examples/verify_demo.ax`. v0.5 added extern
functions and capability binding: `extern fn`
binds to a native runtime implementation, grouped by
`capability` blocks, contracts checked at the boundary, ledger
tier `bound`. Shipped natives cover files and stdin/env; builtins
`join` / `replace` also landed. See `examples/files.ax`. v0.4
added generics: generic structs and functions, monomorphized
per instantiation, type arguments explicit or inferred (including
return position). See `examples/generics.ax`. v0.3 added methods
(`impl` blocks, `obj.m(args)`) and string interpolation
(`"total: {inv.total}"`, with `{{` / `}}` as literal braces). See
`examples/methods.ax`. v0.2 added maps (`{string: int}` style, int
or string keys) with `has` / `keys` / `del`, plus a `split`
builtin. See `examples/wordcount.ax`.

## Quick start

```sh
cargo build --release          # builds axiomc
./target/release/axiomc run examples/hello.ax
./target/release/axiomc build examples/ledger/main.ax -o build/ledger
./target/release/axiomc verify examples/verify_demo.ax   # bounded checking
./build/ledger                 # a double-entry bookkeeping system, in Axiom
cat build/ledger.ledger.json   # the contracts it carries, with tiers
```

## Language in one screen

```axiom
struct Account {
    name: string;
    balance: int;
    invariant balance >= 0;
}

fn transfer(from: Account, to: Account, amt: int) -> bool {
    requires amt > 0;
    requires from.balance >= amt;
    ensures from.balance >= 0;
    from.balance = from.balance - amt;
    to.balance = to.balance + amt;
    return true;
}

fn main() {
    let a = Account { name: "cash", balance: 100 };
    let b = Account { name: "sales", balance: 0 };
    transfer(a, b, 30);
    print(a.name, a.balance);   // cash 70
}
```

Features: `int / float / bool / string`, arrays, maps, structs,
functions (recursion included), `if / while / for-in`, modules via
`use`, string concatenation, builtins `print, len, push, range, str,
int, float, has, keys, del, split`, and a `check expr;` statement
for inline assertions.

See `DESIGN.md` for the spec, the contract semantics, and the
roadmap. Examples live in `examples/`; the substantial ones are
`examples/ledger/`, a double-entry bookkeeping system with balanced
journal entries enforced by contract, and `examples/billing/`, a
full billing system built on that ledger: subscription lifecycle
with trials, period invoicing with metered usage, 13% tax,
payments with overpay-to-credit, refunds, mid-period plan-change
proration, and a dunning sweep (past due, grace, void, cancel).
Its 90-day simulation asserts every closing balance in cents
before it prints the trial balance.

```sh
./target/release/axiomc run examples/billing/main.ax
```

The third substantial example is `examples/rockstore/`: a
log-structured key-value database (Bitcask-style) with an
in-memory index, crash recovery by log replay, tombstone
deletes, byte-exact space accounting, and compaction, driven
by a small command line. It exists to prove a database can be
built in Axiom on the `file` capability:

```sh
printf 'put name ziqi\nget name\nscan\nstats\nquit\n' \
  | ./target/release/axiomc run examples/rockstore/main.ax
```
