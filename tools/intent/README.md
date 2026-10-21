# The intent layer (tools/intent)

Axiom's language core checks contracts and keeps the evidence
ledger. This directory is the layer that sits on top of it:
the intent loop from the design.

1. A human states intent in near-natural language (the `intent`
   field of a `*.intent.json`).
2. An AI drafts the Axiom program (`*.ax` next to it).
3. Every ambiguity in the intent becomes a **decision**: id,
   question, options, a default, and an `answer` slot. The
   human answers only what they care about; unanswered
   decisions run on their default and are reported as accepted
   risks, never silently.
4. Each claimed property names its evidence: `scenario` (the
   program's own checks, printing `PROP <id> OK` — tier
   runtime-checked), `verify` (a pure function for
   `axiomc verify` — tier bounded), or `none` (listed as
   UNCHECKED).

Run it:

```
python3 tools/intent/intentc.py tools/intent/examples/coupon.intent.json
```

It writes `SPEC.md` (the pinned spec) and `EVIDENCE.md` (the
per-property evidence table) next to the intent file, and exits
non-zero if any evidenced property fails. Division of labor,
stated plainly: the drafting step is an AI's job; everything
after the draft — pinning, running, tiering, reporting — is
this tool's, and it invents nothing.
