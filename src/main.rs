// axiomc: the Axiom compiler driver.
//   axiomc build <file.ax> [-o <out>] [--emit-c]
//   axiomc run   <file.ax>
//   axiomc check <file.ax>
mod ast;
mod checker;
mod codegen;
mod lexer;
mod parser;

use ast::Program;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::exit;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        usage();
        exit(2);
    }
    let cmd = args[1].as_str();
    let file = args[2].clone();
    let mut out: Option<String> = None;
    let mut emit_c = false;
    let mut i = 3;
    while i < args.len() {
        match args[i].as_str() {
            "-o" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("axiomc: -o needs a value");
                    exit(2);
                }
                out = Some(args[i].clone());
            }
            "--emit-c" => emit_c = true,
            other => {
                eprintln!("axiomc: unknown option {}", other);
                exit(2);
            }
        }
        i += 1;
    }

    let result = match cmd {
        "check" => cmd_check(&file),
        "build" => cmd_build(&file, out.as_deref(), emit_c, false),
        "run" => cmd_build(&file, out.as_deref(), emit_c, true),
        "verify" => cmd_verify(&file),
        _ => {
            usage();
            exit(2);
        }
    };
    if let Err(msg) = result {
        eprintln!("axiomc: {}", msg);
        exit(1);
    }
}

fn usage() {
    eprintln!("usage:");
    eprintln!("  axiomc build <file.ax> [-o <out>] [--emit-c]");
    eprintln!("  axiomc run   <file.ax>");
    eprintln!("  axiomc check <file.ax>");
    eprintln!("  axiomc verify <file.ax>");
}

/// Load the entry file and everything it `use`s, merged into one Program.
fn load_program(entry: &str) -> Result<Program, String> {
    let mut items = Vec::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();
    load_file(Path::new(entry), Path::new(entry), &mut items, &mut seen)?;
    Ok(Program { items })
}

fn load_file(
    path: &Path,
    display: &Path,
    items: &mut Vec<ast::Item>,
    seen: &mut HashSet<PathBuf>,
) -> Result<(), String> {
    let canon = std::fs::canonicalize(path)
        .map_err(|e| format!("cannot open {}: {}", path.display(), e))?;
    if !seen.insert(canon) {
        return Ok(());
    }
    let src = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {}", path.display(), e))?;
    let label = display.display().to_string();
    let toks = lexer::lex(&src, &label)?;
    let mut p = parser::Parser::new(toks, &src, &label);
    let out = p.parse_program()?;
    // Dependencies first, so global initialization order follows `use`.
    for u in &out.uses {
        let dep = path.parent().map(|b| b.join(u)).unwrap_or_else(|| PathBuf::from(u));
        let dep_display = display
            .parent()
            .map(|b| b.join(u))
            .unwrap_or_else(|| PathBuf::from(u));
        load_file(&dep, &dep_display, items, seen)?;
    }
    items.extend(out.items);
    Ok(())
}

fn compile(file: &str) -> Result<(checker::TProgram, String), String> {
    let prog = load_program(file)?;
    let tp = checker::Checker::new().check(&prog)?;
    let missing = codegen::natives_missing(&tp);
    if !missing.is_empty() {
        return Err(format!(
            "extern fn '{}' has no native implementation in the Axiom runtime",
            missing.join("', '")
        ));
    }
    let c = codegen::CodeGen::new(&tp).gen_program(&tp);
    Ok((tp, c))
}

fn ledger_json(tp: &checker::TProgram, program: &str) -> String {
    let mut entries: Vec<String> = Vec::new();
    for f in &tp.fns {
        // Extern bindings are recorded at the bound tier: the
        // implementation is native (trusted as bound), while its
        // contracts are still checked on every call.
        let tier = if f.is_extern { "bound" } else { "runtime-checked" };
        if f.is_extern {
            let cap = match &f.capability {
                Some(c) => format!(", \"capability\": \"{}\"", codegen::c_escape(c)),
                None => String::new(),
            };
            entries.push(format!(
                "    {{\"kind\": \"binding\", \"owner\": \"fn {}\", \"text\": \"native implementation axn_{}\"{}, \"file\": \"{}\", \"line\": {}, \"tier\": \"bound\"}}",
                codegen::c_escape(&f.name),
                codegen::c_escape(&f.name),
                cap,
                codegen::c_escape(&f.file),
                f.line
            ));
        }
        for c in &f.contracts {
            let kind = match c.kind {
                ast::ContractKind::Requires => "requires",
                ast::ContractKind::Ensures => "ensures",
            };
            entries.push(format!(
                "    {{\"kind\": \"{}\", \"owner\": \"fn {}\", \"text\": \"{}\", \"file\": \"{}\", \"line\": {}, \"tier\": \"{}\"}}",
                kind,
                codegen::c_escape(&f.name),
                codegen::c_escape(&c.text),
                codegen::c_escape(&c.file),
                c.line,
                tier
            ));
        }
    }
    for s in &tp.structs {
        for inv in &s.invariants {
            entries.push(format!(
                "    {{\"kind\": \"invariant\", \"owner\": \"struct {}\", \"text\": \"{}\", \"file\": \"{}\", \"line\": {}, \"tier\": \"runtime-checked\"}}",
                codegen::c_escape(&s.name),
                codegen::c_escape(&inv.text),
                codegen::c_escape(&inv.file),
                inv.line
            ));
        }
    }
    format!(
        "{{\n  \"program\": \"{}\",\n  \"contracts\": [\n{}\n  ]\n}}\n",
        codegen::c_escape(program),
        entries.join(",\n")
    )
}

fn count_contracts(tp: &checker::TProgram) -> usize {
    tp.fns.iter().map(|f| f.contracts.len()).sum::<usize>()
        + tp.structs.iter().map(|s| s.invariants.len()).sum::<usize>()
}

fn cmd_check(file: &str) -> Result<(), String> {
    let (tp, _) = compile(file)?;
    println!(
        "ok: {} functions, {} structs, {} contracts",
        tp.fns.len(),
        tp.structs.len(),
        count_contracts(&tp)
    );
    Ok(())
}

/// Bounded exhaustive verification: build a harness that calls
/// every eligible pure function on every combination of a bounded
/// input domain and reports counterexamples. Writes
/// <stem>.verify.json with per-function results at tier "bounded"
/// for functions with no counterexample.
fn cmd_verify(file: &str) -> Result<(), String> {
    let (tp, _) = compile(file)?;
    let plan = codegen::verify_plan(&tp);
    if plan.is_empty() {
        println!(
            "verify: no eligible functions (need pure functions with int/bool parameters)"
        );
        return Ok(());
    }
    let stem = Path::new(file)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "program".to_string());
    let c_src = codegen::CodeGen::new(&tp).gen_verify_program(&tp, &plan);
    let dir = std::env::temp_dir().join(format!("axiomc-verify-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let c_path = dir.join(format!("{}.verify.c", stem));
    let bin_path = dir.join(format!("{}-verify", stem));
    std::fs::write(&c_path, &c_src).map_err(|e| e.to_string())?;
    let cc_out = std::process::Command::new("cc")
        .args(["-O2", "-std=gnu99", "-w", "-o"])
        .arg(&bin_path)
        .arg(&c_path)
        .output()
        .map_err(|e| format!("cannot run cc: {}", e))?;
    if !cc_out.status.success() {
        return Err(format!(
            "C compilation of the verify harness failed:\n{}",
            String::from_utf8_lossy(&cc_out.stderr)
        ));
    }
    let run = std::process::Command::new(&bin_path)
        .output()
        .map_err(|e| format!("cannot run verify harness: {}", e))?;
    let stdout = String::from_utf8_lossy(&run.stdout).to_string();
    print!("{}", stdout);
    if !run.stderr.is_empty() {
        eprint!("{}", String::from_utf8_lossy(&run.stderr));
    }
    // Assemble the verify report from the VERIFY lines.
    let mut results: Vec<String> = Vec::new();
    let mut any_fail = false;
    for line in stdout.lines() {
        let rest = match line.strip_prefix("VERIFY fn=") {
            Some(r) => r,
            None => continue,
        };
        let mut fields: Vec<(&str, &str)> = Vec::new();
        let mut name = "";
        for tok in rest.split(' ') {
            if let Some((k, v)) = tok.split_once('=') {
                fields.push((k, v));
            } else {
                name = tok;
            }
        }
        let get = |k: &str| {
            fields
                .iter()
                .find(|(fk, _)| *fk == k)
                .map(|(_, v)| *v)
                .unwrap_or("0")
        };
        if fields.iter().any(|(k, _)| *k == "skipped") {
            results.push(format!(
                "    {{\"fn\": \"{}\", \"skipped\": \"{}\", \"tier\": \"runtime-checked\"}}",
                codegen::c_escape(name),
                codegen::c_escape(get("skipped"))
            ));
            continue;
        }
        let failed: i64 = get("failed").parse().unwrap_or(0);
        if failed > 0 {
            any_fail = true;
        }
        results.push(format!(
            "    {{\"fn\": \"{}\", \"cases\": {}, \"excluded\": {}, \"passed\": {}, \"failed\": {}, \"tier\": \"{}\"}}",
            codegen::c_escape(name),
            get("cases"),
            get("excluded"),
            get("passed"),
            get("failed"),
            if failed == 0 { "bounded" } else { "runtime-checked" }
        ));
    }
    let report = format!(
        "{{\n  \"program\": \"{}\",\n  \"results\": [\n{}\n  ]\n}}\n",
        codegen::c_escape(&stem),
        results.join(",\n")
    );
    let report_path = format!("{}.verify.json", stem);
    std::fs::write(&report_path, &report).map_err(|e| e.to_string())?;
    println!("verify report: {}", report_path);
    if any_fail || !run.status.success() {
        return Err("verify found counterexamples (see above)".to_string());
    }
    Ok(())
}

fn cmd_build(
    file: &str,
    out: Option<&str>,
    emit_c: bool,
    run: bool,
) -> Result<(), String> {
    let (tp, c_src) = compile(file)?;
    let stem = Path::new(file)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "program".to_string());
    let (bin_path, c_path) = if run {
        let dir = std::env::temp_dir().join(format!("axiomc-run-{}", std::process::id()));
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        (dir.join(&stem), dir.join(format!("{}.c", stem)))
    } else {
        let bin = out.map(|s| s.to_string()).unwrap_or_else(|| stem.clone());
        (PathBuf::from(bin), PathBuf::from(format!("{}.c", stem)))
    };
    if let Some(parent) = bin_path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
    }
    std::fs::write(&c_path, &c_src).map_err(|e| e.to_string())?;
    let cc_out = std::process::Command::new("cc")
        .args(["-O2", "-std=gnu99", "-w", "-o"])
        .arg(&bin_path)
        .arg(&c_path)
        .output()
        .map_err(|e| format!("cannot run cc: {}", e))?;
    if !cc_out.status.success() {
        return Err(format!(
            "C compilation failed:\n{}",
            String::from_utf8_lossy(&cc_out.stderr)
        ));
    }
    if !emit_c && !run {
        let _ = std::fs::remove_file(&c_path);
    }
    // Verification ledger next to the binary.
    let ledger_path = {
        let mut p = bin_path.as_os_str().to_owned();
        p.push(".ledger.json");
        PathBuf::from(p)
    };
    std::fs::write(&ledger_path, ledger_json(&tp, &stem)).map_err(|e| e.to_string())?;

    if run {
        eprintln!(
            "axiomc: built {} ({} contracts, ledger at {})",
            bin_path.display(),
            count_contracts(&tp),
            ledger_path.display()
        );
        let status = std::process::Command::new(&bin_path)
            .status()
            .map_err(|e| format!("cannot run {}: {}", bin_path.display(), e))?;
        if !status.success() {
            return Err(format!(
                "program exited with {}",
                status.code().unwrap_or(-1)
            ));
        }
        Ok(())
    } else {
        println!(
            "built {} ({} contracts, ledger at {})",
            bin_path.display(),
            count_contracts(&tp),
            ledger_path.display()
        );
        Ok(())
    }
}
