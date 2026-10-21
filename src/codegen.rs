// Axiom code generator: typed AST -> one self-contained C file
// (runtime source first, then the program).
use crate::ast::{BinOp, ContractKind, UnOp};
use crate::checker::*;
use std::collections::{HashMap, HashSet};

const RUNTIME: &str = include_str!("../runtime/axiom_rt.c");

// Names of extern functions whose native implementation
// (axn_<name>) is missing from the runtime source.
pub fn natives_missing(tp: &TProgram) -> Vec<String> {
    let mut missing = Vec::new();
    for f in &tp.fns {
        if f.is_extern && !RUNTIME.contains(&format!("axn_{}(", f.name)) {
            missing.push(f.name.clone());
        }
    }
    missing
}

pub fn c_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            '\0' => out.push_str("\\0"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\x{:02x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

#[derive(Clone, Copy, PartialEq)]
enum Ctx {
    Normal,
    Invariant,
}

pub struct CodeGen<'a> {
    structs: &'a [TStruct],
    struct_by_name: HashMap<String, &'a TStruct>,
    globals: HashSet<String>,
    scopes: Vec<HashSet<String>>,
    tmp: usize,
    // Separate compilation: generated functions and invariant
    // checkers get external linkage so modules can link.
    pub separate: bool,
    // Garbage collection (--gc): generated code maintains shadow
    // root stacks and collects at statement boundaries.
    pub gc: bool,
    // Per-scope count of rooted locals (parallel to scopes);
    // index 0 of a function counts body-level lets only,
    // parameters are rooted separately.
    scope_roots: Vec<usize>,
    // Scope-stack index of the innermost loop's own scope, for
    // computing how many roots break/continue must pop.
    loop_marks: Vec<usize>,
}

impl<'a> CodeGen<'a> {
    pub fn new(tp: &'a TProgram) -> Self {
        let mut by_name = HashMap::new();
        for s in &tp.structs {
            by_name.insert(s.name.clone(), s);
        }
        CodeGen {
            structs: &tp.structs,
            struct_by_name: by_name,
            globals: tp.globals.iter().map(|g| g.name.clone()).collect(),
            scopes: Vec::new(),
            tmp: 0,
            separate: false,
            gc: false,
            scope_roots: Vec::new(),
            loop_marks: Vec::new(),
        }
    }

    fn link_prefix(&self) -> &'static str {
        if self.separate { "" } else { "static " }
    }

    fn fresh(&mut self) -> usize {
        self.tmp += 1;
        self.tmp
    }

    fn is_local(&self, name: &str) -> bool {
        self.scopes.iter().rev().any(|s| s.contains(name))
    }

    fn ident_c(&self, name: &str, ctx: Ctx) -> String {
        match ctx {
            Ctx::Invariant => format!("f_{}", name),
            Ctx::Normal => {
                if self.is_local(name) {
                    format!("v_{}", name)
                } else if self.globals.contains(name) {
                    format!("g_{}", name)
                } else if name == "result" {
                    "__result".to_string()
                } else {
                    format!("v_{}", name)
                }
            }
        }
    }

    pub fn gen_program(&mut self, tp: &TProgram) -> String {
        let mut out = self.gen_core(tp);
        // Entry point.
        out.push_str("int main(void) {\n");
        for g in &tp.globals {
            let init = self.gen_expr(&g.init, Ctx::Normal);
            out.push_str(&format!("    g_{} = {};\n", g.name, init));
        }
        if self.gc {
            for g in &tp.globals {
                out.push_str(&format!("    ax_root_global(&g_{});\n", g.name));
            }
            out.push_str("    ax_gc_enable();\n");
        }
        out.push_str("    axf_main();\n    return 0;\n}\n");
        out
    }

    fn gen_inv_checker(&mut self, s: &TStruct) -> String {
        let mut out = format!(
            "{}void ax_check_inv_{}(AxVal self) {{\n",
            self.link_prefix(),
            s.name
        );
        for (i, (fname, _)) in s.fields.iter().enumerate() {
            out.push_str(&format!(
                "    AxVal f_{} = ax_obj_get(self, {});\n",
                fname, i
            ));
        }
        for inv in &s.invariants {
            let cond = self.gen_expr(&inv.expr, Ctx::Invariant);
            out.push_str(&format!(
                "    if (!ax_truth({})) ax_contract_fail(\"invariant\", \"{}\", \"{}\", \"{}\", {});\n",
                cond,
                c_escape(&s.name),
                c_escape(&inv.text),
                c_escape(&inv.file),
                inv.line
            ));
        }
        out.push_str("}\n\n");
        out
    }

    fn fn_prototype(f: &TFn, prefix: &str) -> String {
        let params = (0..f.params.len())
            .map(|_| "AxVal")
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "{}AxVal axf_{}({});\n",
            prefix,
            f.name,
            if params.is_empty() { "void".to_string() } else { params }
        )
    }

    // Everything except the entry point: runtime, invariant
    // checkers, prototypes, globals, function bodies.
    fn gen_core(&mut self, tp: &TProgram) -> String {
        let mut out = String::new();
        out.push_str(RUNTIME);
        out.push_str("\n/* ===== generated by axiomc ===== */\n");

        // Invariant checkers.
        for s in self.structs.iter() {
            if s.invariants.is_empty() {
                continue;
            }
            let checker = self.gen_inv_checker(s);
            out.push_str(&checker);
        }

        // Function prototypes.
        for f in &tp.fns {
            out.push_str(&Self::fn_prototype(f, "static "));
        }
        // Native prototypes for extern bindings (implemented by
        // the runtime section above).
        for f in &tp.fns {
            if f.is_extern {
                let params = (0..f.params.len())
                    .map(|_| "AxVal")
                    .collect::<Vec<_>>()
                    .join(", ");
                out.push_str(&format!(
                    "static AxVal axn_{}({});\n",
                    f.name,
                    if params.is_empty() { "void".to_string() } else { params }
                ));
            }
        }
        out.push('\n');

        // Globals.
        for g in &tp.globals {
            out.push_str(&format!("static AxVal g_{};\n", g.name));
        }
        out.push('\n');

        // Functions.
        for f in &tp.fns {
            out.push_str(&self.gen_fn(f));
            out.push('\n');
        }
        out
    }

    fn gen_fn(&mut self, f: &TFn) -> String {
        self.scopes.clear();
        self.scope_roots.clear();
        let mut param_scope = HashSet::new();
        let mut params = Vec::new();
        for (pname, _) in &f.params {
            param_scope.insert(pname.clone());
            params.push(format!("AxVal v_{}", pname));
        }
        self.scopes.push(param_scope);
        self.scope_roots.push(0);
        let mut out = String::new();
        out.push_str(&format!(
            "{}AxVal axf_{}({}) {{\n",
            self.link_prefix(),
            f.name,
            if params.is_empty() { "void".to_string() } else { params.join(", ") }
        ));
        out.push_str("    AxVal __result = ax_void();\n");
        if self.gc {
            // Parameters are roots for the whole call: contract
            // clauses and callees may trigger collection.
            for (pname, _) in &f.params {
                out.push_str(&format!("    ax_root_push(&v_{});\n", pname));
            }
        }
        // requires
        for c in &f.contracts {
            if c.kind == ContractKind::Requires {
                let cond = self.gen_expr(&c.expr, Ctx::Normal);
                out.push_str(&format!(
                    "    if (!ax_truth({})) ax_contract_fail(\"requires\", \"{}\", \"{}\", \"{}\", {});\n",
                    cond,
                    c_escape(&f.name),
                    c_escape(&c.text),
                    c_escape(&c.file),
                    c.line
                ));
            }
        }
        if f.is_extern {
            // The body is the native call; contracts wrap it.
            let args = f
                .params
                .iter()
                .map(|(pname, _)| format!("v_{}", pname))
                .collect::<Vec<_>>()
                .join(", ");
            out.push_str(&format!("    __result = axn_{}({});\n", f.name, args));
        }
        for stmt in &f.body {
            out.push_str(&self.gen_stmt(stmt, 1));
        }
        if self.gc && self.scope_roots[0] > 0 {
            // Fall-through path: body-level lets leave the root
            // stack here. Return statements pop them themselves
            // and jump over this line. Ensures clauses see only
            // parameters and result, so this is safe.
            out.push_str(&format!("    ax_root_pop({});\n", self.scope_roots[0]));
        }
        out.push_str("    __epilogue: ;\n");
        for c in &f.contracts {
            if c.kind == ContractKind::Ensures {
                let cond = self.gen_expr(&c.expr, Ctx::Normal);
                out.push_str(&format!(
                    "    if (!ax_truth({})) ax_contract_fail(\"ensures\", \"{}\", \"{}\", \"{}\", {});\n",
                    cond,
                    c_escape(&f.name),
                    c_escape(&c.text),
                    c_escape(&c.file),
                    c.line
                ));
            }
        }
        if self.gc && !f.params.is_empty() {
            out.push_str(&format!("    ax_root_pop({});\n", f.params.len()));
        }
        out.push_str("    return __result;\n}\n");
        self.scopes.pop();
        self.scope_roots.pop();
        out
    }

    fn gen_block(&mut self, stmts: &[TStmt], indent: usize) -> String {
        self.scopes.push(HashSet::new());
        self.scope_roots.push(0);
        let mut out = String::new();
        for s in stmts {
            out.push_str(&self.gen_stmt(s, indent));
        }
        let n = self.scope_roots.pop().unwrap_or(0);
        self.scopes.pop();
        if self.gc && n > 0 {
            out.push_str(&format!("{}ax_root_pop({});\n", "    ".repeat(indent), n));
        }
        out
    }

    fn gen_stmt(&mut self, stmt: &TStmt, indent: usize) -> String {
        let pad = "    ".repeat(indent);
        // GC: collection may only run here, at a statement
        // boundary, where every live value is reachable from
        // globals, the root stack, or this statement's temp
        // roots. root_sum / loop_pop are the root-stack pops the
        // control-flow statements must emit before jumping.
        let tm = if self.gc { Some(self.fresh()) } else { None };
        let root_sum: usize = self.scope_roots.iter().sum();
        let loop_pop: usize = match self.loop_marks.last() {
            Some(&m) => self.scope_roots[m..].iter().sum(),
            None => 0,
        };
        let core = match stmt {
            TStmt::Let(name, init) => {
                let e = self.gen_expr(init, Ctx::Normal);
                self.scopes.last_mut().unwrap().insert(name.clone());
                if self.gc {
                    *self.scope_roots.last_mut().unwrap() += 1;
                    format!(
                        "{}AxVal v_{} = {};\n{}ax_root_push(&v_{});\n",
                        pad, name, e, pad, name
                    )
                } else {
                    format!("{}AxVal v_{} = {};\n", pad, name, e)
                }
            }
            TStmt::Assign(name, value) => {
                let v = self.gen_expr(value, Ctx::Normal);
                let target = if self.is_local(name) {
                    format!("v_{}", name)
                } else {
                    format!("g_{}", name)
                };
                format!("{}{} = {};\n", pad, target, v)
            }
            TStmt::FieldAssign(obj, idx, inv_owner, value) => {
                let o = self.gen_expr(obj, Ctx::Normal);
                let v = self.gen_expr(value, Ctx::Normal);
                let n = self.fresh();
                let mut out = format!(
                    "{}{{ AxVal __t{} = {}; ax_obj_set(__t{}, {}, {});",
                    pad, n, o, n, idx, v
                );
                if let Some(sname) = inv_owner {
                    out.push_str(&format!(" ax_check_inv_{}(__t{});", sname, n));
                }
                out.push_str(" }\n");
                out
            }
            TStmt::IndexAssign(arr, idx, value, is_map) => {
                let a = self.gen_expr(arr, Ctx::Normal);
                let i = self.gen_expr(idx, Ctx::Normal);
                let v = self.gen_expr(value, Ctx::Normal);
                let n = self.fresh();
                if *is_map {
                    format!(
                        "{}{{ AxVal __t{} = {}; ax_map_set(__t{}, {}, {}); }}\n",
                        pad, n, a, n, i, v
                    )
                } else {
                    format!(
                        "{}{{ AxVal __t{} = {}; ax_arr_set(__t{}, ({}).u.i, {}); }}\n",
                        pad, n, a, n, i, v
                    )
                }
            }
            TStmt::If(cond, then, els) => {
                let c = self.gen_expr(cond, Ctx::Normal);
                let mut out = format!("{}if (ax_truth({})) {{\n", pad, c);
                out.push_str(&self.gen_block(then, indent + 1));
                out.push_str(&format!("{}}}", pad));
                if let Some(els) = els {
                    out.push_str(" else {\n");
                    out.push_str(&self.gen_block(els, indent + 1));
                    out.push_str(&format!("{}}}", pad));
                }
                out.push('\n');
                out
            }
            TStmt::While(cond, body) => {
                let c = self.gen_expr(cond, Ctx::Normal);
                let mut out = format!("{}while (ax_truth({})) {{\n", pad, c);
                self.loop_marks.push(self.scopes.len());
                out.push_str(&self.gen_block(body, indent + 1));
                self.loop_marks.pop();
                out.push_str(&format!("{}}}\n", pad));
                out
            }
            TStmt::For(var, iter, body) => {
                let raw = self.gen_expr(iter, Ctx::Normal);
                let it = if matches!(iter.ty, Type::Map(_, _)) {
                    format!("ax_map_keys({})", raw)
                } else {
                    raw
                };
                let n = self.fresh();
                let mut out = format!(
                    "{}{{ AxVal __arr{n} = {it};",
                    pad, n = n, it = it
                );
                if self.gc {
                    // The iterated array may be a fresh value
                    // referenced by nothing else; root it for the
                    // whole loop. Break jumps land before the pop
                    // after the loop, so this stays balanced.
                    out.push_str(&format!(" ax_root_push(&__arr{});", n));
                }
                out.push_str(&format!(
                    " int64_t __n{n} = ax_len(__arr{n}); for (int64_t __i{n} = 0; __i{n} < __n{n}; __i{n}++) {{\n",
                    n = n
                ));
                self.scopes.push(HashSet::from([var.clone()]));
                self.scope_roots.push(1);
                self.loop_marks.push(self.scopes.len() - 1);
                out.push_str(&format!(
                    "{}    AxVal v_{} = ax_arr_get(__arr{}, __i{});\n",
                    pad, var, n, n
                ));
                if self.gc {
                    out.push_str(&format!("{}    ax_root_push(&v_{});\n", pad, var));
                }
                for s in body {
                    out.push_str(&self.gen_stmt(s, indent + 1));
                }
                self.loop_marks.pop();
                let k = self.scope_roots.pop().unwrap_or(0);
                self.scopes.pop();
                if self.gc {
                    if k > 0 {
                        out.push_str(&format!(
                            "{}    ax_root_pop({});\n",
                            pad, k
                        ));
                    }
                    out.push_str(&format!("{}}}\n", pad));
                    out.push_str(&format!("{}    ax_root_pop(1);\n", pad));
                    out.push_str(&format!("{}}}\n", pad));
                } else {
                    out.push_str(&format!("{}}} }}\n", pad));
                }
                out
            }
            TStmt::Return(value) => match value {
                Some(e) => {
                    let v = self.gen_expr(e, Ctx::Normal);
                    if self.gc {
                        format!(
                            "{}{{ __result = {}; ax_temp_pop(__tm{});{} goto __epilogue; }}\n",
                            pad,
                            v,
                            tm.unwrap(),
                            if root_sum > 0 {
                                format!(" ax_root_pop({});", root_sum)
                            } else {
                                String::new()
                            }
                        )
                    } else {
                        format!("{}{{ __result = {}; goto __epilogue; }}\n", pad, v)
                    }
                }
                None => {
                    if self.gc {
                        format!(
                            "{}ax_temp_pop(__tm{});{} goto __epilogue;\n",
                            pad,
                            tm.unwrap(),
                            if root_sum > 0 {
                                format!(" ax_root_pop({});", root_sum)
                            } else {
                                String::new()
                            }
                        )
                    } else {
                        format!("{}goto __epilogue;\n", pad)
                    }
                }
            },
            TStmt::Break => {
                if self.gc {
                    format!(
                        "{}ax_temp_pop(__tm{});{} break;\n",
                        pad,
                        tm.unwrap(),
                        if loop_pop > 0 {
                            format!(" ax_root_pop({});", loop_pop)
                        } else {
                            String::new()
                        }
                    )
                } else {
                    format!("{}break;\n", pad)
                }
            }
            TStmt::Continue => {
                if self.gc {
                    format!(
                        "{}ax_temp_pop(__tm{});{} continue;\n",
                        pad,
                        tm.unwrap(),
                        if loop_pop > 0 {
                            format!(" ax_root_pop({});", loop_pop)
                        } else {
                            String::new()
                        }
                    )
                } else {
                    format!("{}continue;\n", pad)
                }
            }
            TStmt::Check(expr, text, file, line) => {
                let e = self.gen_expr(expr, Ctx::Normal);
                format!(
                    "{}if (!ax_truth({})) ax_contract_fail(\"check\", \"\", \"{}\", \"{}\", {});\n",
                    pad,
                    e,
                    c_escape(text),
                    c_escape(file),
                    line
                )
            }
            TStmt::Expr(e) => {
                let v = self.gen_expr(e, Ctx::Normal);
                format!("{}{};\n", pad, v)
            }
        };
        match tm {
            Some(t) => {
                let mut out = format!(
                    "{}ax_gc_maybe();\n{}long __tm{} = ax_temp_mark();\n",
                    pad, pad, t
                );
                out.push_str(&core);
                // Return/Break/Continue emit their own temp pop
                // before jumping; all other statements pop here,
                // once the statement has fully executed.
                if !matches!(stmt, TStmt::Return(_) | TStmt::Break | TStmt::Continue) {
                    out.push_str(&format!("{}ax_temp_pop(__tm{});\n", pad, t));
                }
                out
            }
            None => core,
        }
    }

    fn gen_expr(&mut self, e: &TExpr, ctx: Ctx) -> String {
        match &e.kind {
            TExprKind::Int(v) => format!("ax_int({})", v),
            TExprKind::Float(v) => {
                let mut s = format!("{:?}", v);
                if !s.contains('.') && !s.contains('e') && !s.contains("inf") && !s.contains("NaN") {
                    s.push_str(".0");
                }
                format!("ax_float({})", s)
            }
            TExprKind::Bool(v) => format!("ax_bool({})", if *v { 1 } else { 0 }),
            TExprKind::Str(s) => format!("ax_str(\"{}\")", c_escape(s)),
            TExprKind::Ident(name) => self.ident_c(name, ctx),
            TExprKind::Array(elems) => {
                let n = self.fresh();
                let mut out = format!("({{ AxVal __t{} = ax_arr_new();", n);
                if self.gc && ctx == Ctx::Normal {
                    out.push_str(&format!(" ax_temp_push(&__t{});", n));
                }
                for el in elems {
                    let v = self.gen_expr(el, ctx);
                    out.push_str(&format!(" ax_arr_push(__t{}, {});", n, v));
                }
                out.push_str(&format!(" __t{}; ", n));
                out.push_str("})");
                out
            }
            TExprKind::StructLit(id, name, fields) => {
                let n = self.fresh();
                let nfields = self.structs[*id].fields.len();
                let has_inv = !self.structs[*id].invariants.is_empty();
                let mut out = format!(
                    "({{ AxVal __t{} = ax_obj_new({}, {});",
                    n,
                    id,
                    nfields
                );
                if self.gc && ctx == Ctx::Normal {
                    out.push_str(&format!(" ax_temp_push(&__t{});", n));
                }
                for (idx, fe) in fields {
                    let v = self.gen_expr(fe, ctx);
                    out.push_str(&format!(" ax_obj_set(__t{}, {}, {});", n, idx, v));
                }
                if has_inv {
                    out.push_str(&format!(" ax_check_inv_{}(__t{});", name, n));
                }
                out.push_str(&format!(" __t{}; ", n));
                out.push_str("})");
                out
            }
            TExprKind::Call(name, args) => {
                if self.gc && ctx == Ctx::Normal {
                    // Root each argument as soon as it is
                    // evaluated and root the result: the callee's
                    // statements may collect, and sibling
                    // subexpressions run after this call returns.
                    let base = self.fresh();
                    let mut out = String::from("({ ");
                    let mut names = Vec::new();
                    for (i, a) in args.iter().enumerate() {
                        let s = self.gen_expr(a, ctx);
                        out.push_str(&format!(
                            "AxVal __a{}_{} = {}; ax_temp_push(&__a{}_{}); ",
                            base, i, s, base, i
                        ));
                        names.push(format!("__a{}_{}", base, i));
                    }
                    out.push_str(&format!(
                        "AxVal __r{} = axf_{}({}); ax_temp_push(&__r{}); __r{}; }})",
                        base,
                        name,
                        names.join(", "),
                        base,
                        base
                    ));
                    out
                } else {
                    let mut parts = Vec::new();
                    for a in args {
                        parts.push(self.gen_expr(a, ctx));
                    }
                    format!("axf_{}({})", name, parts.join(", "))
                }
            }
            TExprKind::Builtin(name, args) => {
                let mut parts = Vec::new();
                for a in args {
                    parts.push(self.gen_expr(a, ctx));
                }
                match name.as_str() {
                    "print" => format!("ax_print_line({}, {})", parts.len(), parts.join(", "))
                        .replace(", )", ")")
                        .replace("(0, )", "(0)"),
                    "len" => format!("ax_int(ax_len({}))", parts[0]),
                    "push" => format!("ax_arr_push({}, {})", parts[0], parts[1]),
                    "range" => {
                        if parts.len() == 1 {
                            format!("ax_range(0, ({}).u.i)", parts[0])
                        } else {
                            format!("ax_range(({}).u.i, ({}).u.i)", parts[0], parts[1])
                        }
                    }
                    "str" => format!("ax_str(ax_str_of({}))", parts[0]),
                    "int" => format!("ax_to_int({})", parts[0]),
                    "float" => format!("ax_to_float({})", parts[0]),
                    "has" => format!("ax_bool(ax_map_has({}, {}))", parts[0], parts[1]),
                    "keys" => format!("ax_map_keys({})", parts[0]),
                    "del" => format!("ax_map_del({}, {})", parts[0], parts[1]),
                    "split" => format!("ax_split({}, {})", parts[0], parts[1]),
                    "join" => format!("ax_join({}, {})", parts[0], parts[1]),
                    "replace" => format!("ax_replace({}, {}, {})", parts[0], parts[1], parts[2]),
                    "substr" => format!("ax_substr({}, ({}).u.i, ({}).u.i)", parts[0], parts[1], parts[2]),
                    "index_of" => format!("ax_int(ax_index_of({}, {}))", parts[0], parts[1]),
                    "starts_with" => format!("ax_bool(ax_starts_with({}, {}))", parts[0], parts[1]),
                    "raise" => format!("({{ ax_raise({}); ax_void(); }})", parts[0]),
                    "gc_collect" => "ax_gc_collect_builtin()".to_string(),
                    "chr" => format!("ax_chr(({}).u.i)", parts[0]),
                    "ord" => format!("ax_int(ax_ord_of({}))", parts[0]),
                    _ => "ax_void()".to_string(),
                }
            }
            TExprKind::Field(obj, idx) => {
                let o = self.gen_expr(obj, ctx);
                format!("ax_obj_get({}, {})", o, idx)
            }
            TExprKind::Index(arr, idx) => {
                let a = self.gen_expr(arr, ctx);
                let i = self.gen_expr(idx, ctx);
                if matches!(arr.ty, Type::Map(_, _)) {
                    format!("ax_map_get({}, {})", a, i)
                } else {
                    format!("ax_arr_get({}, ({}).u.i)", a, i)
                }
            }
            TExprKind::MapLit(pairs) => {
                let n = self.fresh();
                let mut out = format!("({{ AxVal __t{} = ax_map_new();", n);
                if self.gc && ctx == Ctx::Normal {
                    out.push_str(&format!(" ax_temp_push(&__t{});", n));
                }
                for (ke, ve) in pairs {
                    let k = self.gen_expr(ke, ctx);
                    let v = self.gen_expr(ve, ctx);
                    out.push_str(&format!(" ax_map_set(__t{}, {}, {});", n, k, v));
                }
                out.push_str(&format!(" __t{}; ", n));
                out.push_str("})");
                out
            }
            TExprKind::Bin(op, l, r) => {
                let lv = self.gen_expr(l, ctx);
                let rv = self.gen_expr(r, ctx);
                let inner = match op {
                    BinOp::Add => format!("ax_add({}, {})", lv, rv),
                    BinOp::Sub => format!("ax_sub({}, {})", lv, rv),
                    BinOp::Mul => format!("ax_mul({}, {})", lv, rv),
                    BinOp::Div => format!("ax_div({}, {})", lv, rv),
                    BinOp::Mod => format!("ax_mod({}, {})", lv, rv),
                    BinOp::Eq => format!("ax_eq({}, {})", lv, rv),
                    BinOp::Ne => format!("ax_bool(!ax_truth(ax_eq({}, {})))", lv, rv),
                    BinOp::Lt => format!("ax_lt({}, {})", lv, rv),
                    BinOp::Le => format!("ax_le({}, {})", lv, rv),
                    BinOp::Gt => format!("ax_gt({}, {})", lv, rv),
                    BinOp::Ge => format!("ax_ge({}, {})", lv, rv),
                    BinOp::And => format!("ax_bool(({}.u.b) && ({}.u.b))", lv, rv),
                    BinOp::Or => format!("ax_bool(({}.u.b) || ({}.u.b))", lv, rv),
                };
                if self.gc
                    && ctx == Ctx::Normal
                    && matches!(
                        e.ty,
                        Type::Str | Type::Array(_) | Type::Map(_, _) | Type::Struct(_)
                    )
                {
                    // A fresh heap result can sit in this
                    // expression while a later sibling call
                    // collects; root it until the statement ends.
                    let n = self.fresh();
                    format!(
                        "({{ AxVal __b{} = {}; ax_temp_push(&__b{}); __b{}; }})",
                        n, inner, n, n
                    )
                } else {
                    inner
                }
            }
            TExprKind::Un(op, e) => {
                let v = self.gen_expr(e, ctx);
                match op {
                    UnOp::Neg => format!("ax_neg({})", v),
                    UnOp::Not => format!("ax_bool(!({}.u.b))", v),
                }
            }
        }
    }
}

/* ===== bounded verification (axiomc verify) =====
   Eligible functions: pure (no extern calls, no print/raise, no
   mutation of globals, fields, arrays, or maps, directly or
   through callees) with only int/bool parameters. Each function
   is called on every combination of a bounded input domain;
   literals from its requires clauses join the domain so boundary
   values are exercised. Inputs failing requires are excluded,
   not counterexamples. */

pub struct VerifyFn {
    pub name: String,
    pub domains: Vec<Vec<i64>>,
    pub bool_params: Vec<bool>,
    pub skipped: bool,
}

fn walk_expr(e: &TExpr, calls: &mut Vec<String>, builtins: &mut Vec<String>, impure: &mut bool) {
    match &e.kind {
        TExprKind::Call(n, args) => {
            calls.push(n.clone());
            for a in args {
                walk_expr(a, calls, builtins, impure);
            }
        }
        TExprKind::Builtin(n, args) => {
            builtins.push(n.clone());
            for a in args {
                walk_expr(a, calls, builtins, impure);
            }
        }
        TExprKind::Array(es) => {
            for a in es {
                walk_expr(a, calls, builtins, impure);
            }
        }
        TExprKind::StructLit(_, _, fields) => {
            for (_, a) in fields {
                walk_expr(a, calls, builtins, impure);
            }
        }
        TExprKind::MapLit(pairs) => {
            for (k, v) in pairs {
                walk_expr(k, calls, builtins, impure);
                walk_expr(v, calls, builtins, impure);
            }
        }
        TExprKind::Field(o, _) => walk_expr(o, calls, builtins, impure),
        TExprKind::Index(a, i) => {
            walk_expr(a, calls, builtins, impure);
            walk_expr(i, calls, builtins, impure);
        }
        TExprKind::Bin(_, l, r) => {
            walk_expr(l, calls, builtins, impure);
            walk_expr(r, calls, builtins, impure);
        }
        TExprKind::Un(_, x) => walk_expr(x, calls, builtins, impure),
        _ => {}
    }
}

fn walk_stmt(
    s: &TStmt,
    globals: &HashSet<String>,
    calls: &mut Vec<String>,
    builtins: &mut Vec<String>,
    impure: &mut bool,
) {
    match s {
        TStmt::Let(_, e) | TStmt::Expr(e) | TStmt::Check(e, _, _, _) => {
            walk_expr(e, calls, builtins, impure)
        }
        TStmt::Assign(name, e) => {
            if globals.contains(name) {
                *impure = true;
            }
            walk_expr(e, calls, builtins, impure);
        }
        TStmt::FieldAssign(..) | TStmt::IndexAssign(..) => {
            *impure = true;
        }
        TStmt::If(c, t, el) => {
            walk_expr(c, calls, builtins, impure);
            for x in t {
                walk_stmt(x, globals, calls, builtins, impure);
            }
            if let Some(el) = el {
                for x in el {
                    walk_stmt(x, globals, calls, builtins, impure);
                }
            }
        }
        TStmt::While(c, b) | TStmt::For(_, c, b) => {
            walk_expr(c, calls, builtins, impure);
            for x in b {
                walk_stmt(x, globals, calls, builtins, impure);
            }
        }
        TStmt::Return(Some(e)) => walk_expr(e, calls, builtins, impure),
        _ => {}
    }
}

fn harvest_literals(e: &TExpr, param: &str, out: &mut Vec<i64>) {
    if let TExprKind::Bin(_, l, r) = &e.kind {
        match (&l.kind, &r.kind) {
            (TExprKind::Ident(n), TExprKind::Int(v)) if n == param => {
                out.push(v - 1);
                out.push(*v);
                out.push(v + 1);
            }
            (TExprKind::Int(v), TExprKind::Ident(n)) if n == param => {
                out.push(v - 1);
                out.push(*v);
                out.push(v + 1);
            }
            _ => {}
        }
        harvest_literals(l, param, out);
        harvest_literals(r, param, out);
    } else if let TExprKind::Un(_, x) = &e.kind {
        harvest_literals(x, param, out);
    }
}

pub fn verify_plan(tp: &TProgram) -> Vec<VerifyFn> {
    let globals: HashSet<String> = tp.globals.iter().map(|g| g.name.clone()).collect();
    let mut impure: HashMap<String, bool> = HashMap::new();
    let mut callees: HashMap<String, Vec<String>> = HashMap::new();
    for f in &tp.fns {
        let mut bad = f.is_extern;
        let mut calls = Vec::new();
        let mut builtins = Vec::new();
        for c in &f.contracts {
            walk_expr(&c.expr, &mut calls, &mut builtins, &mut bad);
        }
        for s in &f.body {
            walk_stmt(s, &globals, &mut calls, &mut builtins, &mut bad);
        }
        if builtins.iter().any(|b| matches!(b.as_str(), "print" | "raise" | "push" | "del")) {
            bad = true;
        }
        impure.insert(f.name.clone(), bad);
        callees.insert(f.name.clone(), calls);
    }
    // Propagate impurity through the call graph to a fixpoint.
    loop {
        let mut changed = false;
        for f in &tp.fns {
            if impure[f.name.as_str()] {
                continue;
            }
            if callees[f.name.as_str()]
                .iter()
                .any(|c| impure.get(c).copied().unwrap_or(true))
            {
                impure.insert(f.name.clone(), true);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    let mut plan = Vec::new();
    for f in &tp.fns {
        if f.name == "main" || impure[f.name.as_str()] {
            continue;
        }
        let mut ok = matches!(f.ret, Type::Int | Type::Bool | Type::Void);
        let mut domains: Vec<Vec<i64>> = Vec::new();
        let mut bool_params = Vec::new();
        for (pname, pty) in &f.params {
            match pty {
                Type::Bool => {
                    domains.push(vec![0, 1]);
                    bool_params.push(true);
                }
                Type::Int => {
                    let mut dom: Vec<i64> = vec![-3, -2, -1, 0, 1, 2, 3];
                    for c in &f.contracts {
                        if c.kind == ContractKind::Requires {
                            harvest_literals(&c.expr, pname, &mut dom);
                        }
                    }
                    dom.sort_unstable();
                    dom.dedup();
                    if dom.len() > 12 {
                        let mut keep: Vec<i64> = vec![-1, 0, 1];
                        keep.extend(dom.iter().copied());
                        keep.sort_unstable();
                        keep.dedup();
                        keep.truncate(12);
                        dom = keep;
                    }
                    domains.push(dom);
                    bool_params.push(false);
                }
                _ => {
                    ok = false;
                    break;
                }
            }
        }
        if !ok {
            continue;
        }
        let combos: usize = domains.iter().map(|d| d.len()).product();
        let mut skipped = false;
        if combos > 20000 {
            for (i, d) in domains.iter_mut().enumerate() {
                if !bool_params[i] {
                    *d = vec![-1, 0, 1];
                }
            }
            let combos2: usize = domains.iter().map(|d| d.len()).product();
            if combos2 > 20000 {
                skipped = true;
            }
        }
        plan.push(VerifyFn { name: f.name.clone(), domains, bool_params, skipped });
    }
    plan
}

impl<'a> CodeGen<'a> {
    fn gen_requires_helper(&mut self, f: &TFn) -> String {
        self.scopes.clear();
        let mut scope = HashSet::new();
        let mut params = Vec::new();
        for (pname, _) in &f.params {
            scope.insert(pname.clone());
            params.push(format!("AxVal v_{}", pname));
        }
        self.scopes.push(scope);
        let mut conds = Vec::new();
        for c in &f.contracts {
            if c.kind == ContractKind::Requires {
                let e = self.gen_expr(&c.expr, Ctx::Normal);
                conds.push(format!("ax_truth({})", e));
            }
        }
        self.scopes.pop();
        format!(
            "static int axreq_{}({}) {{ return {}; }}\n",
            f.name,
            if params.is_empty() { "void".to_string() } else { params.join(", ") },
            if conds.is_empty() { "1".to_string() } else { conds.join(" && ") }
        )
    }

    fn gen_verify_loops(&self, vf: &VerifyFn, depth: usize, pad: &str) -> String {
        if depth == vf.domains.len() {
            let args = (0..vf.domains.len())
                .map(|i| format!("a{}", i))
                .collect::<Vec<_>>()
                .join(", ");
            let vals = (0..vf.domains.len())
                .map(|i| format!("(long long)dom{}[i{}]", i, i))
                .collect::<Vec<_>>()
                .join(", ");
            let fmts = (0..vf.domains.len()).map(|_| "%lld").collect::<Vec<_>>().join(", ");
            let val_args = if vals.is_empty() {
                String::new()
            } else {
                format!(", {}", vals)
            };
            return format!(
                "{p}if (!axreq_{n}({a})) {{ excl++; }}\n\
                 {p}else {{\n\
                 {p}    cases++;\n\
                 {p}    fflush(NULL);\n\
                 {p}    pid_t pid = fork();\n\
                 {p}    if (pid == 0) {{ axf_{n}({a}); _exit(0); }}\n\
                 {p}    int st = 0;\n\
                 {p}    waitpid(pid, &st, 0);\n\
                 {p}    if (!WIFEXITED(st) || WEXITSTATUS(st) != 0) {{\n\
                 {p}        fail++;\n\
                 {p}        if (fail <= 3) printf(\"  counterexample: {n}({f}) exited %d\\n\"{va}, WIFEXITED(st) ? WEXITSTATUS(st) : -1);\n\
                 {p}    }}\n\
                 {p}}}\n",
                p = pad, n = vf.name, a = args, f = fmts, va = val_args
            );
        }
        let mut out = format!(
            "{p}for (int64_t i{d} = 0; i{d} < {n}; i{d}++) {{\n{p}    AxVal a{d} = {ctor}(dom{d}[i{d}]);\n",
            p = pad,
            d = depth,
            n = vf.domains[depth].len(),
            ctor = if vf.bool_params[depth] { "ax_bool" } else { "ax_int" }
        );
        out.push_str(&self.gen_verify_loops(vf, depth + 1, &format!("{}    ", pad)));
        out.push_str(&format!("{}}}\n", pad));
        out
    }

    pub fn gen_verify_program(&mut self, tp: &TProgram, plan: &[VerifyFn]) -> String {
        let mut out = self.gen_core(tp);
        let by_name: HashMap<&str, &TFn> =
            tp.fns.iter().map(|f| (f.name.as_str(), f)).collect();
        for vf in plan {
            if !vf.skipped {
                out.push_str(&self.gen_requires_helper(by_name[vf.name.as_str()]));
            }
        }
        out.push_str("\n#include <unistd.h>\n#include <sys/types.h>\n#include <sys/wait.h>\n");
        out.push_str("static long v_total_fail = 0;\n");
        for vf in plan {
            if vf.skipped {
                continue;
            }
            out.push_str(&format!("static void verify_{}(void) {{\n", vf.name));
            for (i, dom) in vf.domains.iter().enumerate() {
                let vals = dom.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(", ");
                out.push_str(&format!(
                    "    static const int64_t dom{}[] = {{ {} }};\n",
                    i, vals
                ));
            }
            out.push_str("    long cases = 0, excl = 0, fail = 0;\n");
            out.push_str(&self.gen_verify_loops(vf, 0, "    "));
            out.push_str(&format!(
                "    printf(\"VERIFY fn=%s cases=%ld excluded=%ld passed=%ld failed=%ld\\n\", \"{}\", cases, excl, cases - fail, fail);\n",
                vf.name
            ));
            out.push_str("    v_total_fail += fail;\n}\n");
        }
        out.push_str("int main(void) {\n");
        for g in &tp.globals {
            let init = self.gen_expr(&g.init, Ctx::Normal);
            out.push_str(&format!("    g_{} = {};\n", g.name, init));
        }
        for vf in plan {
            if vf.skipped {
                out.push_str(&format!(
                    "    printf(\"VERIFY fn=%s skipped=too-many-combinations\\n\", \"{}\");\n",
                    vf.name
                ));
            } else {
                out.push_str(&format!("    verify_{}();\n", vf.name));
            }
        }
        out.push_str(
            "    if (v_total_fail > 0) { printf(\"VERIFY FAILED: %ld counterexample(s)\\n\", v_total_fail); return 1; }\n",
        );
        out.push_str("    printf(\"VERIFY OK (bounded)\\n\");\n    return 0;\n}\n");
        out
    }
}

/* ===== separate compilation (axiomc build --separate) =====
   One C file per Axiom source file, compiled to separate
   objects and linked. Every module carries its own private copy
   of the runtime helpers (they are stateless; values are plain
   malloc'd memory, so copies interoperate), while generated
   functions, invariant checkers, and globals get external
   linkage through a shared header. The checker still runs on
   the whole program: module interfaces are the checked program
   itself, not per-module signatures. */

pub struct ModuleOut {
    pub stem: String,
    pub source: String,
}

fn module_stem(file: &str) -> String {
    let mut s: String = file
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    if s.ends_with("_ax") {
        s.truncate(s.len() - 3);
    }
    s
}

const RUNTIME_SPLIT: &str = "static void *ax_alloc";

impl<'a> CodeGen<'a> {
    pub fn gen_header(&mut self, tp: &TProgram) -> String {
        let prefix = RUNTIME.split(RUNTIME_SPLIT).next().unwrap();
        let mut out = String::new();
        out.push_str("#pragma once\n");
        out.push_str(prefix);
        out.push_str("/* ===== axiom generated declarations ===== */\n");
        for f in &tp.fns {
            out.push_str(&Self::fn_prototype(f, ""));
        }
        for s in self.structs.iter() {
            if !s.invariants.is_empty() {
                out.push_str(&format!("void ax_check_inv_{}(AxVal self);\n", s.name));
            }
        }
        for g in &tp.globals {
            out.push_str(&format!("extern AxVal g_{};\n", g.name));
        }
        out
    }

    pub fn gen_modules(&mut self, tp: &TProgram, entry_file: &str) -> Vec<ModuleOut> {
        let (_, runtime_body) = RUNTIME.split_once(RUNTIME_SPLIT).unwrap();
        let runtime_body = format!("{}{}", RUNTIME_SPLIT, runtime_body);
        let structs = self.structs;
        let mut files: Vec<String> = vec![entry_file.to_string()];
        for f in &tp.fns {
            if !files.contains(&f.file) {
                files.push(f.file.clone());
            }
        }
        for s in structs.iter() {
            if !files.contains(&s.file) {
                files.push(s.file.clone());
            }
        }
        let mut used: HashSet<String> = HashSet::new();
        let mut modules = Vec::new();
        for file in &files {
            let mut stem = module_stem(file);
            if used.contains(&stem) {
                let mut i = 2;
                while used.contains(&format!("{}_{}", stem, i)) {
                    i += 1;
                }
                stem = format!("{}_{}", stem, i);
            }
            used.insert(stem.clone());
            let mut src = String::new();
            src.push_str("#include \"axiom_gen.h\"\n");
            src.push_str(&runtime_body);
            src.push_str("\n/* ===== generated by axiomc (separate) ===== */\n");
            for s in structs.iter() {
                if s.file == *file && !s.invariants.is_empty() {
                    let c = self.gen_inv_checker(s);
                    src.push_str(&c);
                }
            }
            for f in &tp.fns {
                if f.file == *file && f.is_extern {
                    let params = (0..f.params.len())
                        .map(|_| "AxVal")
                        .collect::<Vec<_>>()
                        .join(", ");
                    src.push_str(&format!(
                        "static AxVal axn_{}({});\n",
                        f.name,
                        if params.is_empty() { "void".to_string() } else { params }
                    ));
                }
            }
            if file == entry_file {
                for g in &tp.globals {
                    src.push_str(&format!("AxVal g_{};\n", g.name));
                }
            }
            for f in &tp.fns {
                if f.file == *file {
                    src.push_str(&self.gen_fn(f));
                    src.push('\n');
                }
            }
            if file == entry_file {
                src.push_str("int main(void) {\n");
                for g in &tp.globals {
                    let init = self.gen_expr(&g.init, Ctx::Normal);
                    src.push_str(&format!("    g_{} = {};\n", g.name, init));
                }
                src.push_str("    axf_main();\n    return 0;\n}\n");
            }
            modules.push(ModuleOut { stem, source: src });
        }
        modules
    }
}
