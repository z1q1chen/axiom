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

expect_verify_ok() {
    name="$1"; file="$2"
    rm -f ./*.verify.json
    if $AX verify "$file" > /tmp/axv.out 2>&1 && grep -q "VERIFY OK (bounded)" /tmp/axv.out; then
        echo "PASS $name"
    else
        echo "FAIL $name: verify did not report BOUNDED OK:"
        cat /tmp/axv.out
        fail=1
    fi
    rm -f ./*.verify.json
}

expect_verify_counterexample() {
    name="$1"; file="$2"; want="$3"
    rm -f ./*.verify.json
    if $AX verify "$file" > /tmp/axv.out 2>&1; then
        echo "FAIL $name: verify unexpectedly passed"
        fail=1
    elif grep -q "$want" /tmp/axv.out; then
        echo "PASS $name"
    else
        echo "FAIL $name: no counterexample found in output:"
        cat /tmp/axv.out
        fail=1
    fi
    rm -f ./*.verify.json
}

expect_out hello examples/hello.ax "total: 10"
expect_out fib examples/fib.ax "10 55"
expect_out transfer examples/transfer.ax "cash 70"
expect_out ledger examples/ledger/main.ax "TRIAL BALANCE OK"
expect_out billing examples/billing/main.ax "ALL CLOSING BALANCES MATCH"
expect_out methods examples/methods.ax "count is 3"
expect_out interp examples/methods.ax "literal braces: {ok}"
expect_out generics examples/generics.ax "first is 10 and x"
expect_out files examples/files.ax "lines: alpha,beta,gamma,"
expect_out joinrepl examples/files.ax "home set: true, exists: true"
expect_out wordcount examples/wordcount.ax "the appears 4 times"
expect_out maptests tests/map_basics.ax "map ok"
expect_violation requires tests/violate_requires.ax "amt > 0"
expect_violation invariant tests/violate_invariant.ax "balance >= 0"
expect_violation extern tests/violate_extern.ax "len(path) > 0"
expect_verify_ok verifydemo examples/verify_demo.ax
expect_verify_counterexample verifyce tests/verify_counterexample.ax "counterexample: bump(-3)"

# RockStore: two processes over one database file prove
# append, replay recovery, delete, and compaction.
rm -f /tmp/rockstore_test.db
printf 'put a 1\nput b 2\nput a 3\nquit\n' | ROCKSTORE_PATH=/tmp/rockstore_test.db $AX run examples/rockstore/main.ax > /tmp/rs.out 2>/dev/null
printf 'get a\ndel b\ncompact\nvalidate\nquit\n' | ROCKSTORE_PATH=/tmp/rockstore_test.db $AX run examples/rockstore/main.ax >> /tmp/rs.out 2>/dev/null
if grep -q "^3$" /tmp/rs.out && grep -q "compacted: dead_bytes" /tmp/rs.out && grep -q "^VALID$" /tmp/rs.out; then
    echo "PASS rockstore"
else
    echo "FAIL rockstore:"
    cat /tmp/rs.out
    fail=1
fi
rm -f /tmp/rockstore_test.db

# Separate compilation: the billing system split per module,
# compiled to objects, linked, and run.
rm -rf /tmp/axsep && mkdir -p /tmp/axsep
ROOT_ABS="$(pwd)"
AX_ABS="$ROOT_ABS/target/release/axiomc"
sep_out="$(cd /tmp/axsep && $AX_ABS build $ROOT_ABS/examples/billing/main.ax --separate -o /tmp/axsep/billing 2>&1 && /tmp/axsep/billing)"
if printf '%s' "$sep_out" | grep -q "ALL CLOSING BALANCES MATCH"; then
    echo "PASS separate"
else
    echo "FAIL separate:"
    printf '%s\n' "$sep_out"
    fail=1
fi
rm -rf /tmp/axsep

if [ $fail -eq 0 ]; then
    echo "ALL TESTS PASS"
else
    exit 1
fi
