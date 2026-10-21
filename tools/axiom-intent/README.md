# axiom-intent: Axiom in its intent-first form

The language in this repo proved the mechanisms (contracts,
invariants, evidence tiers). The conclusion that followed:
**the forcing lives in the intent loop, not in the target
language.** This tool is that form.

- The **intent file is the only source of truth**. It declares
  structs with invariants, functions with requires/ensures at
  intent level, decisions with defaults, and properties with
  named evidence.
- The Rust code is a **generated artifact**. Contracts are
  lowered into it (requires on entry, ensures on the single
  exit path, invariants at function boundaries). A lockfile
  records its hash; `axiomi.py check` reports IN-LOOP,
  STALE (intent changed, rebuild), or BYPASSED (someone
  edited the generated Rust directly).
- **The forcing**: a property with no evidence fails the
  build unless the intent file carries a signed acceptance
  (`"accepted": true` + a note). Unanswered decisions run on
  their defaults and are listed as accepted risks. Skipping a
  guarantee is a recorded decision, never an oversight.
- Evidence tiers stay honest: scenario snippets and lowered
  contracts are runtime-checked; pure int functions can be
  exhausted over a bounded domain (bounded); anything else
  is UNCHECKED and visible in EVIDENCE.md.

```
python3 tools/axiom-intent/axiomi.py build tools/axiom-intent/examples/coupon2.intent.json
python3 tools/axiom-intent/axiomi.py check tools/axiom-intent/examples/coupon2.intent.json
```

Division of labor: an AI drafts the function bodies and
scenario snippets inside the intent file; this tool pins the
spec, generates, compiles, runs, verifies, and reports. It
authors nothing. The earlier prototype (`tools/intent/`)
targets Axiom itself; this one targets Rust, because the
target language was never the point.
