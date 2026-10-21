#!/usr/bin/env python3
"""intentc: the Axiom intent layer, as loop machinery.

The loop (from the design): a human states intent in near-natural
language; an AI drafts Axiom code from it; every place the intent
is ambiguous becomes a DECISION with options and a default; the
human answers only the decisions they care about; the answers pin
the spec; the machine then produces evidence, tiered honestly.

This tool is the non-AI half of that loop. It takes:
  <name>.intent.json   intent text, decisions, properties
  <name>.ax            the AI-drafted Axiom program (companion)

and produces, next to them:
  SPEC.md              the pinned spec (decisions + chosen answers;
                       decisions still on their default are listed
                       as accepted risks, not hidden)
  EVIDENCE.md          per-property evidence: scenario properties
                       are runtime-checked (the program prints
                       "PROP <id> OK" after its checks pass),
                       verify properties are bounded (axiomc
                       verify's report), anything else is listed
                       under UNCHECKED.

Usage: python3 intentc.py <name>.intent.json
Exit code 0 iff every evidenced property passes.
"""

import json
import os
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))


def find_axiomc():
    env = os.environ.get("AXIOMC")
    if env:
        return env
    cand = os.path.normpath(os.path.join(HERE, "..", "..", "target", "release", "axiomc"))
    if os.path.exists(cand):
        return cand
    return "axiomc"


def run(cmd, cwd=None):
    p = subprocess.run(cmd, cwd=cwd, capture_output=True, text=True)
    return p.returncode, p.stdout, p.stderr


def main():
    if len(sys.argv) != 2:
        print("usage: intentc.py <name>.intent.json", file=sys.stderr)
        return 2
    intent_path = os.path.abspath(sys.argv[1])
    base_dir = os.path.dirname(intent_path)
    with open(intent_path, encoding="utf-8") as f:
        spec = json.load(f)
    module = spec["module"]
    source = os.path.join(base_dir, spec["source"])
    axiomc = find_axiomc()

    # --- pin the spec ---
    decisions = spec.get("decisions", [])
    unanswered = [d for d in decisions if d.get("answer") is None]
    lines = [f"# Spec: {module}", "", spec["intent"].strip(), "", "## Decisions", ""]
    lines.append("| id | question | options | chosen | source |")
    lines.append("|---|---|---|---|---|")
    for d in decisions:
        chosen = d.get("answer") if d.get("answer") is not None else d["default"]
        origin = "answered" if d.get("answer") is not None else "DEFAULT (accepted risk)"
        lines.append(
            f"| {d['id']} | {d['question']} | {', '.join(d['options'])} | {chosen} | {origin} |"
        )
    lines += ["", "## Properties", ""]
    for p in spec.get("properties", []):
        lines.append(f"- **{p['id']}**: {p['text']} (evidence: {p['evidence']})")
    spec_md = os.path.join(base_dir, "SPEC.md")
    with open(spec_md, "w", encoding="utf-8") as f:
        f.write("\n".join(lines) + "\n")

    # --- run the machine checks ---
    results = {}  # property id -> (tier, "PASS"/"FAIL"/"UNCHECKED", note)
    with tempfile.TemporaryDirectory() as td:
        bin_path = os.path.join(td, module)
        rc, out, err = run([axiomc, "build", source, "-o", bin_path])
        if rc != 0:
            print(f"intentc: axiomc build failed:\n{err}", file=sys.stderr)
            return 1
        ledger_path = bin_path + ".ledger.json"
        n_contracts = 0
        if os.path.exists(ledger_path):
            with open(ledger_path, encoding="utf-8") as f:
                n_contracts = len(json.load(f).get("contracts", []))
        rc, stdout, stderr = run([bin_path])
        for p in spec.get("properties", []):
            if p["evidence"] == "scenario":
                ok = rc == 0 and f"PROP {p['id']} OK" in stdout
                results[p["id"]] = (
                    "runtime-checked",
                    "PASS" if ok else "FAIL",
                    "scenario checks in the program's main",
                )
        # bounded tier: axiomc verify writes <stem>.verify.json in cwd
        stem = os.path.splitext(os.path.basename(source))[0]
        rc_v, out_v, err_v = run([axiomc, "verify", source], cwd=td)
        report = {}
        report_path = os.path.join(td, f"{stem}.verify.json")
        if os.path.exists(report_path):
            with open(report_path, encoding="utf-8") as f:
                for r in json.load(f).get("results", []):
                    report[r["fn"]] = r
        for p in spec.get("properties", []):
            if p["evidence"] == "verify":
                r = report.get(p.get("fn", ""))
                if r and r.get("tier") == "bounded" and r.get("failed", 1) == 0:
                    results[p["id"]] = (
                        "bounded",
                        "PASS",
                        f"exhausted on the declared domain ({r.get('cases', '?')} cases)",
                    )
                else:
                    results[p["id"]] = ("bounded", "FAIL", "counterexample or not eligible")
    for p in spec.get("properties", []):
        if p["id"] not in results:
            results[p["id"]] = ("none", "UNCHECKED", "no machine evidence wired")

    # --- evidence report ---
    ev = [f"# Evidence: {module}", ""]
    ev.append(f"Contracts in ledger: {n_contracts}. "
              f"Decisions: {len(decisions) - len(unanswered)} answered, "
              f"{len(unanswered)} on default.")
    ev += ["", "| property | tier | result | note |", "|---|---|---|---|"]
    failed = 0
    for p in spec.get("properties", []):
        tier, result, note = results[p["id"]]
        if result == "FAIL":
            failed += 1
        ev.append(f"| {p['id']} | {tier} | {result} | {note} |")
    ev += ["", "## Unchecked / accepted risks", ""]
    unchecked = [p["id"] for p in spec.get("properties", [])
                 if results[p["id"]][1] == "UNCHECKED"]
    if unchecked:
        for pid in unchecked:
            ev.append(f"- property `{pid}` has no machine evidence")
    if unanswered:
        for d in unanswered:
            ev.append(f"- decision `{d['id']}` still on default `{d['default']}`")
    if not unchecked and not unanswered:
        ev.append("- none")
    ev_path = os.path.join(base_dir, "EVIDENCE.md")
    with open(ev_path, "w", encoding="utf-8") as f:
        f.write("\n".join(ev) + "\n")

    print(f"intentc: SPEC.md and EVIDENCE.md written for {module}; "
          f"{sum(1 for r in results.values() if r[1] == 'PASS')} passed, "
          f"{failed} failed, {len(unchecked)} unchecked, "
          f"{len(unanswered)} decisions on default")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
