#!/bin/sh
# Axiom smoke tests. Run from the repo root: sh tests/run_tests.sh
set -u
command -v cargo >/dev/null 2>&1 || PATH="$HOME/.cargo/bin:$PATH"
cd "$(dirname "$0")/.."
AX="./target/release/axiomc"
cargo build --release -q || { echo "FAIL: cargo build"; exit 1; }
fail=0

expect_out() {
    name="$1"; file="$2"; want="$3"
    got="$($AX run "$file" 2>/dev/null)"
    if printf '%s' "$got" | grep -q "$want"; then
        echo "PASS $name"
    else
        echo "FAIL $name: wanted output containing '$want', got:"
        printf '%s\n' "$got"
        fail=1
    fi
}

expect_violation() {
    name="$1"; file="$2"; want="$3"
    out="$($AX run "$file" 2>&1)"
    code=$?
    if [ $code -eq 1 ] && printf '%s' "$out" | grep -q "contract violated" && printf '%s' "$out" | grep -q "$want"; then
        echo "PASS $name"
    else
        echo "FAIL $name: wanted a contract violation mentioning '$want'"
        printf '%s\n' "$out"
        fail=1
    fi
}

expect_out hello examples/hello.ax "total: 10"
expect_out fib examples/fib.ax "10 55"
expect_out transfer examples/transfer.ax "cash 70"
expect_out ledger examples/ledger/main.ax "TRIAL BALANCE OK"
expect_out billing examples/billing/main.ax "ALL CLOSING BALANCES MATCH"
expect_out methods examples/methods.ax "count is 3"
expect_out interp examples/methods.ax "literal braces: {ok}"
expect_out wordcount examples/wordcount.ax "the appears 4 times"
expect_out maptests tests/map_basics.ax "map ok"
expect_violation requires tests/violate_requires.ax "amt > 0"
expect_violation invariant tests/violate_invariant.ax "balance >= 0"

if [ $fail -eq 0 ]; then
    echo "ALL TESTS PASS"
else
    exit 1
fi
