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

v0.2, built 2026-10-21. A working language, not the full design
proposal: formal proof tiers and capability binding are on the
roadmap in `DESIGN.md`, not in this build. Contracts are
runtime-checked, and the ledger says exactly that.

v0.2 adds maps (`{string: int}` style, int or string keys) with
`has` / `keys` / `del`, plus a `split` builtin. See
`examples/wordcount.ax`.

## Quick start

```sh
cargo build --release          # builds axiomc
./target/release/axiomc run examples/hello.ax
./target/release/axiomc build examples/ledger/main.ax -o build/ledger
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
