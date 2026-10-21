// Axiom type checker: AST -> typed AST (TProgram) or first error.
use crate::ast::*;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug, PartialEq)]
pub enum Type {
    Int,
    Float,
    Bool,
    Str,
    Void,
    Array(Box<Type>),
    Map(Box<Type>, Box<Type>),
    Struct(String),
    // A type parameter inside a generic template (never survives
    // into a checked program: instantiation substitutes it away).
    Param(String),
    // A generic struct applied to arguments that still contain
    // parameters. Concrete applications become Type::Struct of the
    // mangled instantiation name.
    GenericApp(String, Vec<Type>),
}

impl Type {
    pub fn name(&self) -> String {
        match self {
            Type::Int => "int".into(),
            Type::Float => "float".into(),
            Type::Bool => "bool".into(),
            Type::Str => "string".into(),
            Type::Void => "void".into(),
            Type::Array(t) => format!("[{}]", t.name()),
            Type::Map(k, v) => format!("{{{}: {}}}", k.name(), v.name()),
            Type::Struct(n) => n.clone(),
            Type::Param(n) => n.clone(),
            Type::GenericApp(n, args) => format!(
                "{}<{}>",
                n,
                args.iter().map(|a| a.name()).collect::<Vec<_>>().join(", ")
            ),
        }
    }
}

fn mangle_type(ty: &Type) -> String {
    match ty {
        Type::Int => "int".into(),
        Type::Float => "float".into(),
        Type::Bool => "bool".into(),
        Type::Str => "string".into(),
        Type::Void => "void".into(),
        Type::Array(t) => format!("arr_{}", mangle_type(t)),
        Type::Map(k, v) => format!("map_{}_{}", mangle_type(k), mangle_type(v)),
        Type::Struct(n) => n.clone(),
        Type::Param(n) => format!("param_{}", n),
        Type::GenericApp(n, args) => format!(
            "{}__{}",
            n,
            args.iter().map(mangle_type).collect::<Vec<_>>().join("__")
        ),
    }
}

fn type_is_concrete(ty: &Type) -> bool {
    match ty {
        Type::Param(_) => false,
        Type::Array(t) => type_is_concrete(t),
        Type::Map(k, v) => type_is_concrete(k) && type_is_concrete(v),
        Type::GenericApp(_, args) => args.iter().all(type_is_concrete),
        _ => true,
    }
}

fn subst_type(ty: &Type, subst: &HashMap<String, Type>) -> Type {
    match ty {
        Type::Param(p) => subst.get(p).cloned().unwrap_or_else(|| ty.clone()),
        Type::Array(t) => Type::Array(Box::new(subst_type(t, subst))),
        Type::Map(k, v) => Type::Map(
            Box::new(subst_type(k, subst)),
            Box::new(subst_type(v, subst)),
        ),
        Type::GenericApp(n, args) => Type::GenericApp(
            n.clone(),
            args.iter().map(|a| subst_type(a, subst)).collect(),
        ),
        other => other.clone(),
    }
}

#[derive(Clone, Debug)]
pub struct TExpr {
    pub kind: TExprKind,
    pub ty: Type,
}

#[derive(Clone, Debug)]
pub enum TExprKind {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
    Ident(String),
    Array(Vec<TExpr>),
    MapLit(Vec<(TExpr, TExpr)>),
    StructLit(usize, String, Vec<(usize, TExpr)>),
    Call(String, Vec<TExpr>),
    Builtin(String, Vec<TExpr>),
    Field(Box<TExpr>, usize),
    Index(Box<TExpr>, Box<TExpr>),
    Bin(BinOp, Box<TExpr>, Box<TExpr>),
    Un(UnOp, Box<TExpr>),
}

#[derive(Clone, Debug)]
pub enum TStmt {
    Let(String, TExpr),
    Assign(String, TExpr),
    FieldAssign(TExpr, usize, Option<String>, TExpr),
    IndexAssign(TExpr, TExpr, TExpr, bool), // bool: target is a map
    If(TExpr, Vec<TStmt>, Option<Vec<TStmt>>),
    While(TExpr, Vec<TStmt>),
    For(String, TExpr, Vec<TStmt>),
    Return(Option<TExpr>),
    Break,
    Continue,
    Check(TExpr, String, String, u32),
    Expr(TExpr),
}

#[derive(Clone, Debug)]
pub struct TInvariant {
    pub expr: TExpr,
    pub text: String,
    pub file: String,
    pub line: u32,
}

#[derive(Clone, Debug)]
pub struct TContract {
    pub kind: ContractKind,
    pub expr: TExpr,
    pub text: String,
    pub file: String,
    pub line: u32,
}

#[derive(Clone, Debug)]
pub struct TStruct {
    pub name: String,
    pub fields: Vec<(String, Type)>,
    pub invariants: Vec<TInvariant>,
    pub id: usize,
}

#[derive(Clone, Debug)]
pub struct TFn {
    pub name: String,
    pub params: Vec<(String, Type)>,
    pub ret: Type,
    pub contracts: Vec<TContract>,
    pub body: Vec<TStmt>,
    pub file: String,
    pub line: u32,
    pub is_extern: bool,
    pub capability: Option<String>,
}

#[derive(Clone, Debug)]
pub struct TGlobal {
    pub name: String,
    pub ty: Type,
    pub init: TExpr,
}

#[derive(Clone, Debug)]
pub struct TProgram {
    pub structs: Vec<TStruct>,
    pub fns: Vec<TFn>,
    pub globals: Vec<TGlobal>,
}

const BUILTINS: &[&str] = &[
    "print", "len", "push", "range", "str", "int", "float", "raise", "has", "keys", "del",
    "split", "join", "replace", "substr", "index_of", "starts_with",
];

struct FnSig {
    params: Vec<(String, Type)>,
    ret: Type,
}

pub struct Checker {
    struct_fields: HashMap<String, Vec<(String, Type)>>,
    struct_ids: HashMap<String, usize>,
    struct_has_inv: HashMap<String, bool>,
    fns: HashMap<String, FnSig>,
    methods: HashMap<(String, String), String>,
    globals: HashMap<String, Type>,
    scopes: Vec<HashMap<String, Type>>,
    cur_ret: Type,
    cur_fn: String,
    loop_depth: u32,
    // Generic struct templates by base name, and the type
    // substitution active while resolving inside a template or an
    // instantiation (type parameter name -> type).
    generic_structs: HashMap<String, StructDef>,
    type_subst: HashMap<String, Type>,
    // Generic function templates and their pattern signatures,
    // plus the instantiation work queue (definition, substitution,
    // mangled name) drained after pass 4.
    generic_fns: HashMap<String, FnDef>,
    generic_sigs: HashMap<String, FnSig>,
    fn_pending: Vec<(FnDef, HashMap<String, Type>, String)>,
    fn_seen: HashSet<String>,
    // Mangled instantiation name -> (base name, type arguments).
    inst_args: HashMap<String, (String, Vec<Type>)>,
    // TStructs created by instantiation, merged at the end.
    extra_tstructs: Vec<TStruct>,
}

// The name a function is registered and generated under: methods
// are mangled to Struct__method so codegen sees plain identifiers.
fn reg_name(f: &FnDef) -> String {
    match &f.owner {
        Some(o) => format!("{}__{}", o, f.name),
        None => f.name.clone(),
    }
}

fn te(kind: TExprKind, ty: Type) -> TExpr {
    TExpr { kind, ty }
}

impl Checker {
    pub fn new() -> Self {
        Checker {
            struct_fields: HashMap::new(),
            struct_ids: HashMap::new(),
            struct_has_inv: HashMap::new(),
            fns: HashMap::new(),
            methods: HashMap::new(),
            globals: HashMap::new(),
            scopes: Vec::new(),
            cur_ret: Type::Void,
            cur_fn: String::new(),
            loop_depth: 0,
            generic_structs: HashMap::new(),
            type_subst: HashMap::new(),
            generic_fns: HashMap::new(),
            generic_sigs: HashMap::new(),
            fn_pending: Vec::new(),
            fn_seen: HashSet::new(),
            inst_args: HashMap::new(),
            extra_tstructs: Vec::new(),
        }
    }

    fn resolve(&mut self, t: &TypeExpr, span: &Span) -> Result<Type, String> {
        Ok(match t {
            TypeExpr::Int => Type::Int,
            TypeExpr::Float => Type::Float,
            TypeExpr::Bool => Type::Bool,
            TypeExpr::Str => Type::Str,
            TypeExpr::Void => Type::Void,
            TypeExpr::Array(inner) => Type::Array(Box::new(self.resolve(inner, span)?)),
            TypeExpr::Map(k, v) => {
                let kt = self.resolve(k, span)?;
                if kt != Type::Int && kt != Type::Str {
                    return Err(format!(
                        "{}:{}: map key type must be int or string, got {}",
                        span.file,
                        span.line,
                        kt.name()
                    ));
                }
                Type::Map(Box::new(kt), Box::new(self.resolve(v, span)?))
            }
            TypeExpr::Named(name) => {
                if let Some(t) = self.type_subst.get(name) {
                    t.clone()
                } else if self.generic_structs.contains_key(name) {
                    return Err(format!(
                        "{}:{}: generic struct '{}' needs type arguments, e.g. {}<...>",
                        span.file, span.line, name, name
                    ));
                } else if self.struct_fields.contains_key(name) || self.struct_ids.contains_key(name) {
                    Type::Struct(name.clone())
                } else {
                    return Err(format!(
                        "{}:{}: unknown type '{}'",
                        span.file, span.line, name
                    ));
                }
            }
            TypeExpr::Generic(name, arg_exprs) => {
                let template = match self.generic_structs.get(name) {
                    Some(t) => t.clone(),
                    None => {
                        return Err(format!(
                            "{}:{}: unknown generic struct '{}'",
                            span.file, span.line, name
                        ))
                    }
                };
                if arg_exprs.len() != template.type_params.len() {
                    return Err(format!(
                        "{}:{}: generic struct '{}' takes {} type arguments, got {}",
                        span.file,
                        span.line,
                        name,
                        template.type_params.len(),
                        arg_exprs.len()
                    ));
                }
                let mut args = Vec::new();
                for a in arg_exprs {
                    args.push(self.resolve(a, span)?);
                }
                if args.iter().all(type_is_concrete) {
                    Type::Struct(self.instantiate_struct(name, &args, span)?)
                } else {
                    Type::GenericApp(name.clone(), args)
                }
            }
        })
    }

    // Monomorphize a generic struct for concrete type arguments:
    // register the mangled instantiation (fields substituted,
    // invariants checked) exactly once and return its name.
    fn instantiate_struct(
        &mut self,
        base: &str,
        args: &[Type],
        span: &Span,
    ) -> Result<String, String> {
        let mangled = format!(
            "{}__{}",
            base,
            args.iter().map(mangle_type).collect::<Vec<_>>().join("__")
        );
        if self.struct_ids.contains_key(&mangled) {
            return Ok(mangled);
        }
        let template = self.generic_structs[base].clone();
        // Register the id before resolving fields so recursive
        // instantiations (Node<T> containing Node<T>) terminate.
        let id = self.struct_ids.len();
        self.struct_ids.insert(mangled.clone(), id);
        self.struct_has_inv
            .insert(mangled.clone(), !template.invariants.is_empty());
        self.inst_args
            .insert(mangled.clone(), (base.to_string(), args.to_vec()));
        let subst: HashMap<String, Type> = template
            .type_params
            .iter()
            .cloned()
            .zip(args.iter().cloned())
            .collect();
        let saved = std::mem::replace(&mut self.type_subst, subst);
        let mut fields = Vec::new();
        let mut field_res: Result<(), String> = Ok(());
        for (fname, fty) in &template.fields {
            match self.resolve(fty, span) {
                Ok(t) => fields.push((fname.clone(), t)),
                Err(e) => {
                    field_res = Err(e);
                    break;
                }
            }
        }
        self.type_subst = saved;
        field_res?;
        self.struct_fields.insert(mangled.clone(), fields.clone());
        let invariants = self.check_invariants_for(&template, &fields)?;
        self.extra_tstructs.push(TStruct {
            name: mangled.clone(),
            fields,
            invariants,
            id,
        });
        Ok(mangled)
    }

    fn check_invariants_for(
        &mut self,
        def: &StructDef,
        fields: &[(String, Type)],
    ) -> Result<Vec<TInvariant>, String> {
        let mut scope = HashMap::new();
        for (fname, fty) in fields {
            scope.insert(fname.clone(), fty.clone());
        }
        self.scopes.push(scope);
        let mut out = Vec::new();
        let mut res: Result<(), String> = Ok(());
        for inv in &def.invariants {
            match self.check_expr(&inv.expr, None) {
                Ok(e) => {
                    if e.ty != Type::Bool {
                        res = self.err(&inv.span, "invariant must be a bool expression");
                        break;
                    }
                    out.push(TInvariant {
                        expr: e,
                        text: inv.text.clone(),
                        file: inv.span.file.clone(),
                        line: inv.span.line,
                    });
                }
                Err(e) => {
                    res = Err(e);
                    break;
                }
            }
        }
        self.scopes.pop();
        res?;
        Ok(out)
    }

    // Unify a pattern type (may contain type parameters) with a
    // concrete type, recording parameter bindings. Returns false
    // when the shapes cannot match.
    fn unify(
        &self,
        pat: &Type,
        conc: &Type,
        bindings: &mut HashMap<String, Type>,
    ) -> bool {
        match (pat, conc) {
            (Type::Param(p), c) => match bindings.get(p) {
                Some(prev) => prev == c,
                None => {
                    bindings.insert(p.clone(), c.clone());
                    true
                }
            },
            (Type::Array(a), Type::Array(b)) => self.unify(a, b, bindings),
            (Type::Map(k1, v1), Type::Map(k2, v2)) => {
                self.unify(k1, k2, bindings) && self.unify(v1, v2, bindings)
            }
            (Type::GenericApp(n1, a1), Type::GenericApp(n2, a2)) => {
                n1 == n2
                    && a1.len() == a2.len()
                    && a1.iter()
                        .zip(a2.iter())
                        .all(|(x, y)| self.unify(x, y, bindings))
            }
            (Type::GenericApp(n, pats), Type::Struct(m)) => {
                match self.inst_args.get(m) {
                    Some((base, cargs)) => {
                        base == n
                            && pats.len() == cargs.len()
                            && pats.iter()
                                .zip(cargs.iter())
                                .all(|(x, y)| self.unify(x, y, bindings))
                    }
                    None => false,
                }
            }
            (a, b) => a == b,
        }
    }

    fn lookup(&self, name: &str) -> Option<Type> {
        for scope in self.scopes.iter().rev() {
            if let Some(t) = scope.get(name) {
                return Some(t.clone());
            }
        }
        self.globals.get(name).cloned()
    }

    // A literal of a generic struct: determine the type arguments
    // (explicit, from the expected type, or inferred from field
    // values), instantiate, then re-check as a concrete literal.
    fn check_generic_struct_lit(
        &mut self,
        name: &str,
        targs: &[TypeExpr],
        fields: &[(String, Expr)],
        span: &Span,
        expected: Option<&Type>,
    ) -> Result<TExpr, String> {
        let template = self.generic_structs[name].clone();
        let mut bindings: HashMap<String, Type> = HashMap::new();
        if !targs.is_empty() {
            if targs.len() != template.type_params.len() {
                return self.err(
                    span,
                    &format!(
                        "generic struct '{}' takes {} type arguments, got {}",
                        name,
                        template.type_params.len(),
                        targs.len()
                    ),
                );
            }
            for (p, te) in template.type_params.iter().zip(targs.iter()) {
                let t = self.resolve(te, span)?;
                bindings.insert(p.clone(), t);
            }
        } else if let Some(exp) = expected {
            let from: Option<Vec<Type>> = match exp {
                Type::GenericApp(b, args) if b == name => Some(args.clone()),
                Type::Struct(m) => match self.inst_args.get(m) {
                    Some((b, args)) if b == name => Some(args.clone()),
                    _ => None,
                },
                _ => None,
            };
            if let Some(args) = from {
                for (p, a) in template.type_params.iter().zip(args.iter()) {
                    bindings.insert(p.clone(), a.clone());
                }
            }
        }
        if bindings.len() < template.type_params.len() {
            // Infer from field values against pattern field types.
            let saved = std::mem::replace(
                &mut self.type_subst,
                template
                    .type_params
                    .iter()
                    .map(|p| (p.clone(), Type::Param(p.clone())))
                    .collect(),
            );
            let mut pat_fields: Vec<(String, Type)> = Vec::new();
            let mut pat_res: Result<(), String> = Ok(());
            for (fname, fty) in &template.fields {
                match self.resolve(fty, span) {
                    Ok(t) => pat_fields.push((fname.clone(), t)),
                    Err(e) => {
                        pat_res = Err(e);
                        break;
                    }
                }
            }
            self.type_subst = saved;
            pat_res?;
            for (fname, fexpr) in fields {
                let pat = match pat_fields.iter().find(|(n, _)| n == fname) {
                    Some((_, t)) => t.clone(),
                    None => {
                        return self.err(
                            span,
                            &format!("struct '{}' has no field '{}'", name, fname),
                        )
                    }
                };
                let v = self.check_expr(fexpr, None)?;
                if !self.unify(&pat, &v.ty, &mut bindings) {
                    return self.err(
                        span,
                        &format!(
                            "field '{}' of '{}' does not determine the type arguments",
                            fname, name
                        ),
                    );
                }
            }
        }
        let mut args = Vec::new();
        for p in &template.type_params {
            match bindings.get(p) {
                Some(t) => args.push(t.clone()),
                None => {
                    return self.err(
                        span,
                        &format!(
                            "cannot infer type argument '{}' for struct '{}'; write it explicitly, e.g. {}<...>",
                            p, name, name
                        ),
                    )
                }
            }
        }
        if !args.iter().all(type_is_concrete) {
            return self.err(span, "generic struct type arguments must be concrete here");
        }
        let mangled = self.instantiate_struct(name, &args, span)?;
        let lit = Expr::StructLit(mangled, Vec::new(), fields.to_vec(), span.clone());
        self.check_expr(&lit, expected)
    }

    fn err<T>(&self, span: &Span, msg: &str) -> Result<T, String> {
        Err(format!("{}:{}: {}", span.file, span.line, msg))
    }

    pub fn check(mut self, prog: &Program) -> Result<TProgram, String> {
        // Pass 1: register struct names and function signatures.
        let mut struct_defs: Vec<&StructDef> = Vec::new();
        let mut fn_defs: Vec<&FnDef> = Vec::new();
        let mut global_defs: Vec<(&String, &Option<TypeExpr>, &Expr, &Span)> = Vec::new();
        for item in &prog.items {
            match item {
                Item::Struct(s) => {
                    if self.struct_ids.contains_key(&s.name)
                        || self.generic_structs.contains_key(&s.name) {
                        return Err(format!(
                            "{}:{}: duplicate struct '{}'",
                            s.span.file, s.span.line, s.name
                        ));
                    }
                    if s.type_params.is_empty() {
                        let id = self.struct_ids.len();
                        self.struct_ids.insert(s.name.clone(), id);
                        self.struct_has_inv.insert(s.name.clone(), !s.invariants.is_empty());
                        struct_defs.push(s);
                    } else {
                        for (i, (fname, _)) in s.fields.iter().enumerate() {
                            if s.fields[..i].iter().any(|(n, _)| n == fname) {
                                return Err(format!(
                                    "{}:{}: duplicate field '{}' in struct '{}'",
                                    s.span.file, s.span.line, fname, s.name
                                ));
                            }
                        }
                        self.generic_structs.insert(s.name.clone(), s.clone());
                    }
                }
                Item::Fn(f) => {
                    if BUILTINS.contains(&f.name.as_str()) {
                        return Err(format!(
                            "{}:{}: '{}' is a builtin and cannot be redefined",
                            f.span.file, f.span.line, f.name
                        ));
                    }
                    if self.fns.contains_key(&f.name) {
                        return Err(format!(
                            "{}:{}: duplicate function '{}'",
                            f.span.file, f.span.line, f.name
                        ));
                    }
                    fn_defs.push(f);
                }
                Item::Global(name, ann, expr, span) => {
                    if self.globals.contains_key(name) {
                        return Err(format!(
                            "{}:{}: duplicate global '{}'",
                            span.file, span.line, name
                        ));
                    }
                    // Placeholder; real type set in pass 3.
                    self.globals.insert(name.clone(), Type::Void);
                    global_defs.push((name, ann, expr, span));
                }
            }
        }
        // Struct field tables (needs all names registered first).
        for s in &struct_defs {
            let mut fields = Vec::new();
            for (fname, fty) in &s.fields {
                if fields.iter().any(|(n, _): &(String, Type)| n == fname) {
                    return Err(format!(
                        "{}:{}: duplicate field '{}' in struct '{}'",
                        s.span.file, s.span.line, fname, s.name
                    ));
                }
                fields.push((fname.clone(), self.resolve(fty, &s.span)?));
            }
            self.struct_fields.insert(s.name.clone(), fields);
        }
        for f in &fn_defs {
            if !f.type_params.is_empty() {
                if f.owner.is_some() {
                    return Err(format!(
                        "{}:{}: generic methods are not supported yet",
                        f.span.file, f.span.line
                    ));
                }
                // Pattern signature: type parameters resolve to
                // Type::Param under a self-substitution.
                let saved = std::mem::replace(
                    &mut self.type_subst,
                    f.type_params
                        .iter()
                        .map(|p| (p.clone(), Type::Param(p.clone())))
                        .collect(),
                );
                let mut params = Vec::new();
                let mut res: Result<(), String> = Ok(());
                for (pname, pty) in &f.params {
                    match self.resolve(pty, &f.span) {
                        Ok(t) => params.push((pname.clone(), t)),
                        Err(e) => {
                            res = Err(e);
                            break;
                        }
                    }
                }
                let mut ret = Type::Void;
                if res.is_ok() {
                    match self.resolve(&f.ret, &f.span) {
                        Ok(t) => ret = t,
                        Err(e) => res = Err(e),
                    }
                }
                self.type_subst = saved;
                res?;
                self.generic_sigs
                    .insert(f.name.clone(), FnSig { params, ret });
                self.generic_fns.insert(f.name.clone(), (*f).clone());
                continue;
            }
            let mut params = Vec::new();
            for (pname, pty) in &f.params {
                params.push((pname.clone(), self.resolve(pty, &f.span)?));
            }
            let ret = self.resolve(&f.ret, &f.span)?;
            let reg = reg_name(f);
            if let Some(o) = &f.owner {
                if self.generic_structs.contains_key(o) {
                    return Err(format!(
                        "{}:{}: methods on generic structs are not supported yet ('{}')",
                        f.span.file, f.span.line, o
                    ));
                }
                if params.is_empty() || params[0].0 != "self"
                    || params[0].1 != Type::Struct(o.clone()) {
                    return Err(format!(
                        "{}:{}: method '{}' must take 'self' of type {} as its first parameter",
                        f.span.file, f.span.line, f.name, o
                    ));
                }
                self.methods.insert((o.clone(), f.name.clone()), reg.clone());
            }
            self.fns.insert(reg, FnSig { params, ret });
        }

        // Pass 2: struct invariants.
        let mut tstructs = Vec::new();
        for s in &struct_defs {
            let fields = self.struct_fields[s.name.as_str()].clone();
            let invariants = self.check_invariants_for(s, &fields)?;
            tstructs.push(TStruct {
                name: s.name.clone(),
                fields,
                invariants,
                id: self.struct_ids[s.name.as_str()],
            });
        }

        // Pass 3: globals, in order.
        let mut tglobals = Vec::new();
        for (name, ann, expr, span) in &global_defs {
            let expected = match ann {
                Some(t) => Some(self.resolve(t, span)?),
                None => None,
            };
            let init = self.check_expr(expr, expected.as_ref())?;
            if init.ty == Type::Void {
                return self.err(span, "global cannot have void type");
            }
            if let Some(exp) = &expected {
                if *exp != init.ty {
                    return self.err(
                        span,
                        &format!(
                            "global '{}' declared {} but initialized with {}",
                            name,
                            exp.name(),
                            init.ty.name()
                        ),
                    );
                }
            }
            self.globals.insert(name.to_string(), init.ty.clone());
            tglobals.push(TGlobal { name: name.to_string(), ty: init.ty.clone(), init });
        }

        // Pass 4: functions.
        if self.generic_fns.contains_key("main") {
            return Err("main cannot be generic".to_string());
        }
        if !self.fns.contains_key("main") {
            return Err("program has no main function".to_string());
        }
        if !self.fns["main"].params.is_empty() {
            return Err("main must not take parameters".to_string());
        }
        let mut tfns = Vec::new();
        for f in &fn_defs {
            if !f.type_params.is_empty() {
                continue;
            }
            let reg = reg_name(f);
            let sig = self.fns[reg.as_str()].clone_sig();
            tfns.push(self.check_one_fn(f, &sig, &reg)?);
        }
        // Drain the generic instantiation queue; checking one
        // instance may queue more.
        let mut qi = 0;
        while qi < self.fn_pending.len() {
            let (def, subst, reg) = self.fn_pending[qi].clone();
            qi += 1;
            let sig = self.fns[reg.as_str()].clone_sig();
            let saved = std::mem::replace(&mut self.type_subst, subst);
            let r = self.check_one_fn(&def, &sig, &reg);
            self.type_subst = saved;
            tfns.push(r?);
        }

        let mut all_structs = tstructs;
        all_structs.extend(self.extra_tstructs.clone());
        all_structs.sort_by_key(|s| s.id);
        Ok(TProgram { structs: all_structs, fns: tfns, globals: tglobals })
    }

    // Check one function (concrete or a generic instantiation)
    // against a concrete signature.
    fn check_one_fn(
        &mut self,
        f: &FnDef,
        sig: &FnSig,
        reg: &str,
    ) -> Result<TFn, String> {
        self.cur_ret = sig.ret.clone();
        self.cur_fn = reg.to_string();
        self.loop_depth = 0;
        let mut scope: HashMap<String, Type> = HashMap::new();
        for (pname, pty) in &sig.params {
            scope.insert(pname.clone(), pty.clone());
        }
        self.scopes.push(scope);
        let mut tcontracts = Vec::new();
        for c in &f.contracts {
            if c.kind == ContractKind::Ensures {
                if sig.ret == Type::Void {
                    return self.err(&c.span, "ensures requires a non-void return type");
                }
                self.scopes
                    .last_mut()
                    .unwrap()
                    .insert("result".to_string(), sig.ret.clone());
            }
            let e = self.check_expr(&c.expr, None)?;
            if e.ty != Type::Bool {
                return self.err(&c.span, "contract clause must be a bool expression");
            }
            if c.kind == ContractKind::Ensures {
                self.scopes.last_mut().unwrap().remove("result");
            }
            tcontracts.push(TContract {
                kind: c.kind,
                expr: e,
                text: c.text.clone(),
                file: c.span.file.clone(),
                line: c.span.line,
            });
        }
        let mut tbody = Vec::new();
        if !f.is_extern {
            for stmt in &f.body {
                tbody.push(self.check_stmt(stmt)?);
            }
        }
        self.scopes.pop();
        if !f.is_extern && sig.ret != Type::Void && !ends_with_return(&f.body) {
            return Err(format!(
                "{}:{}: function '{}' may end without returning a value",
                f.span.file, f.span.line, f.name
            ));
        }
        Ok(TFn {
            name: reg.to_string(),
            params: sig.params.clone(),
            ret: sig.ret.clone(),
            contracts: tcontracts,
            body: tbody,
            file: f.span.file.clone(),
            line: f.span.line,
            is_extern: f.is_extern,
            capability: f.capability.clone(),
        })
    }

    // Replace any fully-applied generic struct in a type with its
    // monomorphized instantiation.
    fn concretize(&mut self, ty: &Type, span: &Span) -> Result<Type, String> {
        Ok(match ty {
            Type::Array(t) => Type::Array(Box::new(self.concretize(t, span)?)),
            Type::Map(k, v) => Type::Map(
                Box::new(self.concretize(k, span)?),
                Box::new(self.concretize(v, span)?),
            ),
            Type::GenericApp(n, args) => {
                let mut cargs = Vec::new();
                for a in args {
                    cargs.push(self.concretize(a, span)?);
                }
                if cargs.iter().all(type_is_concrete) {
                    Type::Struct(self.instantiate_struct(n, &cargs, span)?)
                } else {
                    Type::GenericApp(n.clone(), cargs)
                }
            }
            other => other.clone(),
        })
    }

    // Register (once) the monomorphization of a generic function
    // for concrete type bindings and queue its body for checking.
    fn instantiate_fn(
        &mut self,
        name: &str,
        bindings: &HashMap<String, Type>,
        span: &Span,
    ) -> Result<String, String> {
        let template = self.generic_fns[name].clone();
        let ordered: Vec<Type> = template
            .type_params
            .iter()
            .map(|p| bindings[p].clone())
            .collect();
        let reg = format!(
            "{}__{}",
            name,
            ordered.iter().map(mangle_type).collect::<Vec<_>>().join("__")
        );
        if self.fn_seen.contains(&reg) {
            return Ok(reg);
        }
        self.fn_seen.insert(reg.clone());
        let pat = self.generic_sigs[name].clone_sig();
        let mut params = Vec::new();
        for (pname, pty) in &pat.params {
            let t = self.concretize(&subst_type(pty, bindings), span)?;
            params.push((pname.clone(), t));
        }
        let ret = self.concretize(&subst_type(&pat.ret, bindings), span)?;
        self.fns.insert(reg.clone(), FnSig { params, ret });
        self.fn_pending
            .push((template, bindings.clone(), reg.clone()));
        Ok(reg)
    }

    // A call to a generic function: infer the type bindings from
    // explicit type arguments, argument types, and the expected
    // result type, then instantiate.
    fn check_generic_call(
        &mut self,
        name: &str,
        targs: &[TypeExpr],
        args: &[Expr],
        span: &Span,
        expected: Option<&Type>,
    ) -> Result<TExpr, String> {
        let template = self.generic_fns[name].clone();
        let pat = self.generic_sigs[name].clone_sig();
        if !targs.is_empty() && targs.len() != template.type_params.len() {
            return self.err(
                span,
                &format!(
                    "'{}' takes {} type arguments, got {}",
                    name,
                    template.type_params.len(),
                    targs.len()
                ),
            );
        }
        let mut bindings: HashMap<String, Type> = HashMap::new();
        for (p, te) in template.type_params.iter().zip(targs.iter()) {
            let t = self.resolve(te, span)?;
            bindings.insert(p.clone(), t);
        }
        if args.len() != pat.params.len() {
            return self.err(
                span,
                &format!(
                    "'{}' takes {} arguments, got {}",
                    name,
                    pat.params.len(),
                    args.len()
                ),
            );
        }
        let mut targs_out = Vec::new();
        for (arg, (_, pty)) in args.iter().zip(pat.params.iter()) {
            let a = self.check_expr(arg, None)?;
            if !self.unify(pty, &a.ty, &mut bindings) {
                return self.err(
                    span,
                    &format!(
                        "argument of '{}' has type {}, which does not match {}",
                        name,
                        a.ty.name(),
                        pty.name()
                    ),
                );
            }
            targs_out.push(a);
        }
        if let Some(exp) = expected {
            if !self.unify(&pat.ret, exp, &mut bindings) {
                return self.err(
                    span,
                    &format!("'{}' cannot return {} here", name, exp.name()),
                );
            }
        }
        for p in &template.type_params {
            if !bindings.contains_key(p) {
                return self.err(
                    span,
                    &format!(
                        "cannot infer type parameter '{}' of '{}'; use explicit type arguments",
                        p, name
                    ),
                );
            }
        }
        let reg = self.instantiate_fn(name, &bindings, span)?;
        let sig = self.fns[reg.as_str()].clone_sig();
        for (a, (_, pty)) in targs_out.iter().zip(sig.params.iter()) {
            if a.ty != *pty {
                return self.err(
                    span,
                    &format!(
                        "argument of '{}' has type {}, expected {}",
                        name,
                        a.ty.name(),
                        pty.name()
                    ),
                );
            }
        }
        Ok(te(TExprKind::Call(reg, targs_out), sig.ret.clone()))
    }

    fn check_block(&mut self, stmts: &[Stmt]) -> Result<Vec<TStmt>, String> {
        self.scopes.push(HashMap::new());
        let mut out = Vec::new();
        for s in stmts {
            out.push(self.check_stmt(s)?);
        }
        self.scopes.pop();
        Ok(out)
    }

    fn declare(&mut self, name: &str, ty: Type, span: &Span) -> Result<(), String> {
        let scope = self.scopes.last_mut().unwrap();
        if scope.contains_key(name) {
            return self.err(span, &format!("'{}' is already defined in this scope", name));
        }
        scope.insert(name.to_string(), ty);
        Ok(())
    }

    fn check_stmt(&mut self, stmt: &Stmt) -> Result<TStmt, String> {
        match stmt {
            Stmt::Let(name, ann, expr, span) => {
                let expected = match ann {
                    Some(t) => Some(self.resolve(t, span)?),
                    None => None,
                };
                let init = self.check_expr(expr, expected.as_ref())?;
                if init.ty == Type::Void {
                    return self.err(span, "cannot bind a void value");
                }
                if let Some(exp) = &expected {
                    if *exp != init.ty {
                        return self.err(
                            span,
                            &format!(
                                "'{}' declared {} but initialized with {}",
                                name,
                                exp.name(),
                                init.ty.name()
                            ),
                        );
                    }
                }
                let ty = init.ty.clone();
                self.declare(name, ty, span)?;
                Ok(TStmt::Let(name.clone(), init))
            }
            Stmt::Assign(name, expr, span) => {
                let ty = match self.lookup(name) {
                    Some(t) => t,
                    None => return self.err(span, &format!("unknown variable '{}'", name)),
                };
                let v = self.check_expr(expr, Some(&ty))?;
                if v.ty != ty {
                    return self.err(
                        span,
                        &format!(
                            "cannot assign {} to '{}' of type {}",
                            v.ty.name(),
                            name,
                            ty.name()
                        ),
                    );
                }
                Ok(TStmt::Assign(name.clone(), v))
            }
            Stmt::FieldAssign(obj, fname, value, span) => {
                let tobj = self.check_expr(obj, None)?;
                let sname = match &tobj.ty {
                    Type::Struct(n) => n.clone(),
                    other => {
                        return self.err(
                            span,
                            &format!("field access on non-struct type {}", other.name()),
                        )
                    }
                };
                let fields = self.struct_fields[sname.as_str()].clone();
                let idx = match fields.iter().position(|(n, _)| n == fname) {
                    Some(i) => i,
                    None => {
                        return self.err(
                            span,
                            &format!("struct '{}' has no field '{}'", sname, fname),
                        )
                    }
                };
                let fty = fields[idx].1.clone();
                let v = self.check_expr(value, Some(&fty))?;
                if v.ty != fty {
                    return self.err(
                        span,
                        &format!(
                            "cannot assign {} to field '{}' of type {}",
                            v.ty.name(),
                            fname,
                            fty.name()
                        ),
                    );
                }
                let inv_owner = if self.struct_has_inv[sname.as_str()] {
                    Some(sname)
                } else {
                    None
                };
                Ok(TStmt::FieldAssign(tobj, idx, inv_owner, v))
            }
            Stmt::IndexAssign(arr, idx, value, span) => {
                let tarr = self.check_expr(arr, None)?;
                match &tarr.ty {
                    Type::Map(k, v) => {
                        let (k, v) = ((**k).clone(), (**v).clone());
                        let tidx = self.check_expr(idx, Some(&k))?;
                        if tidx.ty != k {
                            return self.err(
                                span,
                                &format!("map key has type {}, expected {}", tidx.ty.name(), k.name()),
                            );
                        }
                        let tv = self.check_expr(value, Some(&v))?;
                        if tv.ty != v {
                            return self.err(
                                span,
                                &format!(
                                    "cannot assign {} to map value of type {}",
                                    tv.ty.name(),
                                    v.name()
                                ),
                            );
                        }
                        Ok(TStmt::IndexAssign(tarr, tidx, tv, true))
                    }
                    Type::Array(t) => {
                        let elem = (**t).clone();
                        let tidx = self.check_expr(idx, Some(&Type::Int))?;
                        if tidx.ty != Type::Int {
                            return self.err(span, "array index must be int");
                        }
                        let v = self.check_expr(value, Some(&elem))?;
                        if v.ty != elem {
                            return self.err(
                                span,
                                &format!(
                                    "cannot assign {} to array element of type {}",
                                    v.ty.name(),
                                    elem.name()
                                ),
                            );
                        }
                        Ok(TStmt::IndexAssign(tarr, tidx, v, false))
                    }
                    other => self.err(
                        span,
                        &format!("index assignment on non-indexable type {}", other.name()),
                    ),
                }
            }
            Stmt::If(cond, then, els, span) => {
                let c = self.check_expr(cond, Some(&Type::Bool))?;
                if c.ty != Type::Bool {
                    return self.err(span, "if condition must be bool");
                }
                let t = self.check_block(then)?;
                let e = match els {
                    Some(stmts) => Some(self.check_block(stmts)?),
                    None => None,
                };
                Ok(TStmt::If(c, t, e))
            }
            Stmt::While(cond, body, span) => {
                let c = self.check_expr(cond, Some(&Type::Bool))?;
                if c.ty != Type::Bool {
                    return self.err(span, "while condition must be bool");
                }
                self.loop_depth += 1;
                let b = self.check_block(body)?;
                self.loop_depth -= 1;
                Ok(TStmt::While(c, b))
            }
            Stmt::For(var, iter, body, span) => {
                let it = self.check_expr(iter, None)?;
                let elem = match &it.ty {
                    Type::Array(t) => (**t).clone(),
                    Type::Map(k, _) => (**k).clone(),
                    other => {
                        return self.err(
                            span,
                            &format!("for loop over non-iterable type {}", other.name()),
                        )
                    }
                };
                self.scopes.push(HashMap::new());
                self.declare(var, elem, span)?;
                self.loop_depth += 1;
                let mut b = Vec::new();
                for s in body {
                    b.push(self.check_stmt(s)?);
                }
                self.loop_depth -= 1;
                self.scopes.pop();
                Ok(TStmt::For(var.clone(), it, b))
            }
            Stmt::Return(expr, span) => {
                match (expr, &self.cur_ret) {
                    (None, Type::Void) => Ok(TStmt::Return(None)),
                    (None, other) => self.err(
                        span,
                        &format!(
                            "function '{}' must return {}",
                            self.cur_fn,
                            other.name()
                        ),
                    ),
                    (Some(_), Type::Void) => {
                        self.err(span, "void function cannot return a value")
                    }
                    (Some(e), ret) => {
                        let ret = ret.clone();
                        let v = self.check_expr(e, Some(&ret))?;
                        if v.ty != ret {
                            return self.err(
                                span,
                                &format!(
                                    "function '{}' returns {}, expected {}",
                                    self.cur_fn,
                                    v.ty.name(),
                                    ret.name()
                                ),
                            );
                        }
                        Ok(TStmt::Return(Some(v)))
                    }
                }
            }
            Stmt::Break(span) => {
                if self.loop_depth == 0 {
                    return self.err(span, "break outside a loop");
                }
                Ok(TStmt::Break)
            }
            Stmt::Continue(span) => {
                if self.loop_depth == 0 {
                    return self.err(span, "continue outside a loop");
                }
                Ok(TStmt::Continue)
            }
            Stmt::Check(expr, text, span) => {
                let e = self.check_expr(expr, Some(&Type::Bool))?;
                if e.ty != Type::Bool {
                    return self.err(span, "check requires a bool expression");
                }
                Ok(TStmt::Check(e, text.clone(), span.file.clone(), span.line))
            }
            Stmt::Expr(expr, _) => {
                let e = self.check_expr(expr, None)?;
                Ok(TStmt::Expr(e))
            }
        }
    }

    fn check_expr(&mut self, expr: &Expr, expected: Option<&Type>) -> Result<TExpr, String> {
        match expr {
            Expr::Int(v, _) => Ok(te(TExprKind::Int(*v), Type::Int)),
            Expr::Float(v, _) => Ok(te(TExprKind::Float(*v), Type::Float)),
            Expr::Bool(v, _) => Ok(te(TExprKind::Bool(*v), Type::Bool)),
            Expr::Str(s, _) => Ok(te(TExprKind::Str(s.clone()), Type::Str)),
            Expr::Ident(name, span) => match self.lookup(name) {
                Some(t) => Ok(te(TExprKind::Ident(name.clone()), t)),
                None => self.err(span, &format!("unknown variable '{}'", name)),
            },
            Expr::Array(elems, span) => {
                let expected_elem = match expected {
                    Some(Type::Array(t)) => Some((**t).clone()),
                    _ => None,
                };
                if elems.is_empty() {
                    return match expected_elem {
                        Some(t) => Ok(te(TExprKind::Array(vec![]), Type::Array(Box::new(t)))),
                        None => self.err(
                            span,
                            "cannot infer the element type of an empty array; add a type annotation",
                        ),
                    };
                }
                let first = self.check_expr(&elems[0], expected_elem.as_ref())?;
                let elem_ty = first.ty.clone();
                if elem_ty == Type::Void {
                    return self.err(span, "array elements cannot be void");
                }
                let mut out = vec![first];
                for e in &elems[1..] {
                    let t = self.check_expr(e, Some(&elem_ty))?;
                    if t.ty != elem_ty {
                        return self.err(
                            span,
                            &format!(
                                "array element has type {}, expected {}",
                                t.ty.name(),
                                elem_ty.name()
                            ),
                        );
                    }
                    out.push(t);
                }
                Ok(te(TExprKind::Array(out), Type::Array(Box::new(elem_ty))))
            }
            Expr::StructLit(name, targs, fields, span) => {
                if self.generic_structs.contains_key(name.as_str()) {
                    return self.check_generic_struct_lit(
                        name, targs, fields, span, expected,
                    );
                }
                if !targs.is_empty() {
                    return self.err(
                        span,
                        &format!("struct '{}' is not generic", name),
                    );
                }
                if !self.struct_fields.contains_key(name.as_str()) {
                    return self.err(span, &format!("unknown struct '{}'", name));
                }
                let decl = self.struct_fields[name.as_str()].clone();
                if fields.len() != decl.len() {
                    return self.err(
                        span,
                        &format!(
                            "struct '{}' literal must set all {} fields",
                            name,
                            decl.len()
                        ),
                    );
                }
                let mut by_index: Vec<Option<TExpr>> = vec![None; decl.len()];
                for (fname, fexpr) in fields {
                    let idx = match decl.iter().position(|(n, _)| n == fname) {
                        Some(i) => i,
                        None => {
                            return self.err(
                                span,
                                &format!("struct '{}' has no field '{}'", name, fname),
                            )
                        }
                    };
                    if by_index[idx].is_some() {
                        return self.err(
                            span,
                            &format!("field '{}' is set twice", fname),
                        );
                    }
                    let fty = decl[idx].1.clone();
                    let v = self.check_expr(fexpr, Some(&fty))?;
                    if v.ty != fty {
                        return self.err(
                            span,
                            &format!(
                                "field '{}' has type {}, expected {}",
                                fname,
                                v.ty.name(),
                                fty.name()
                            ),
                        );
                    }
                    by_index[idx] = Some(v);
                }
                let pairs: Vec<(usize, TExpr)> = by_index
                    .into_iter()
                    .enumerate()
                    .map(|(i, v)| (i, v.unwrap()))
                    .collect();
                let id = self.struct_ids[name.as_str()];
                Ok(te(
                    TExprKind::StructLit(id, name.clone(), pairs),
                    Type::Struct(name.clone()),
                ))
            }
            Expr::GenericCall(name, targs, args, span) => {
                if self.generic_fns.contains_key(name.as_str()) {
                    return self.check_generic_call(name, targs, args, span, expected);
                }
                if BUILTINS.contains(&name.as_str()) {
                    return self.err(
                        span,
                        &format!("builtin '{}' does not take type arguments", name),
                    );
                }
                if self.fns.contains_key(name.as_str()) {
                    return self.err(
                        span,
                        &format!("'{}' is not a generic function", name),
                    );
                }
                self.err(span, &format!("unknown function '{}'", name))
            }
            Expr::Call(callee, args, span) => {
                // Method call: base.method(args) resolves through the
                // impl table to the mangled function, with the base
                // expression checked as the self argument.
                if let Expr::Field(obj, mname, _) = callee.as_ref() {
                    let tobj = self.check_expr(obj, None)?;
                    let sname = match &tobj.ty {
                        Type::Struct(n) => n.clone(),
                        other => {
                            return self.err(
                                span,
                                &format!(
                                    "method call on non-struct type {}",
                                    other.name()
                                ),
                            )
                        }
                    };
                    let reg = match self.methods.get(&(sname.clone(), mname.clone())) {
                        Some(r) => r.clone(),
                        None => {
                            return self.err(
                                span,
                                &format!(
                                    "struct '{}' has no method '{}'",
                                    sname, mname
                                ),
                            )
                        }
                    };
                    let sig = self.fns[reg.as_str()].clone_sig();
                    if args.len() + 1 != sig.params.len() {
                        return self.err(
                            span,
                            &format!(
                                "method '{}' takes {} arguments, got {}",
                                mname,
                                sig.params.len() - 1,
                                args.len()
                            ),
                        );
                    }
                    let mut targs = vec![tobj];
                    for (arg, (_, pty)) in args.iter().zip(sig.params.iter().skip(1)) {
                        let a = self.check_expr(arg, Some(pty))?;
                        if a.ty != *pty {
                            return self.err(
                                span,
                                &format!(
                                    "argument of method '{}' has type {}, expected {}",
                                    mname,
                                    a.ty.name(),
                                    pty.name()
                                ),
                            );
                        }
                        targs.push(a);
                    }
                    let ret = sig.ret.clone();
                    return Ok(te(TExprKind::Call(reg, targs), ret));
                }
                let name = match callee.as_ref() {
                    Expr::Ident(n, _) => n.clone(),
                    _ => {
                        return self.err(
                            span,
                            "only direct calls to named functions are supported",
                        )
                    }
                };
                if let Some(sig) = self.fns.get(name.as_str()).map(|s| s.clone_sig()) {
                    if args.len() != sig.params.len() {
                        return self.err(
                            span,
                            &format!(
                                "'{}' takes {} arguments, got {}",
                                name,
                                sig.params.len(),
                                args.len()
                            ),
                        );
                    }
                    let mut targs = Vec::new();
                    for (arg, (_, pty)) in args.iter().zip(sig.params.iter()) {
                        let a = self.check_expr(arg, Some(pty))?;
                        if a.ty != *pty {
                            return self.err(
                                span,
                                &format!(
                                    "argument of '{}' has type {}, expected {}",
                                    name,
                                    a.ty.name(),
                                    pty.name()
                                ),
                            );
                        }
                        targs.push(a);
                    }
                    let ret = sig.ret.clone();
                    return Ok(te(TExprKind::Call(name, targs), ret));
                }
                if self.generic_fns.contains_key(&name) {
                    return self.check_generic_call(&name, &[], args, span, expected);
                }
                if BUILTINS.contains(&name.as_str()) {
                    return self.check_builtin(&name, args, span);
                }
                self.err(span, &format!("unknown function '{}'", name))
            }
            Expr::Field(obj, fname, span) => {
                let tobj = self.check_expr(obj, None)?;
                let sname = match &tobj.ty {
                    Type::Struct(n) => n.clone(),
                    other => {
                        return self.err(
                            span,
                            &format!("field access on non-struct type {}", other.name()),
                        )
                    }
                };
                let fields = self.struct_fields[sname.as_str()].clone();
                match fields.iter().position(|(n, _)| n == fname) {
                    Some(idx) => {
                        let ty = fields[idx].1.clone();
                        Ok(te(TExprKind::Field(Box::new(tobj), idx), ty))
                    }
                    None => self.err(
                        span,
                        &format!("struct '{}' has no field '{}'", sname, fname),
                    ),
                }
            }
            Expr::Index(arr, idx, span) => {
                let tarr = self.check_expr(arr, None)?;
                match &tarr.ty {
                    Type::Map(k, v) => {
                        let (k, v) = ((**k).clone(), (**v).clone());
                        let tidx = self.check_expr(idx, Some(&k))?;
                        if tidx.ty != k {
                            return self.err(
                                span,
                                &format!(
                                    "map key has type {}, expected {}",
                                    tidx.ty.name(),
                                    k.name()
                                ),
                            );
                        }
                        Ok(te(TExprKind::Index(Box::new(tarr), Box::new(tidx)), v))
                    }
                    Type::Array(t) => {
                        let elem = (**t).clone();
                        let tidx = self.check_expr(idx, Some(&Type::Int))?;
                        if tidx.ty != Type::Int {
                            return self.err(span, "array index must be int");
                        }
                        Ok(te(TExprKind::Index(Box::new(tarr), Box::new(tidx)), elem))
                    }
                    other => self.err(
                        span,
                        &format!("indexing a non-indexable type {}", other.name()),
                    ),
                }
            }
            Expr::MapLit(pairs, span) => {
                let expected_kv = match expected {
                    Some(Type::Map(k, v)) => Some(((**k).clone(), (**v).clone())),
                    _ => None,
                };
                if pairs.is_empty() {
                    return match expected_kv {
                        Some((k, v)) => Ok(te(
                            TExprKind::MapLit(vec![]),
                            Type::Map(Box::new(k), Box::new(v)),
                        )),
                        None => self.err(
                            span,
                            "cannot infer the key/value types of an empty map; add a type annotation",
                        ),
                    };
                }
                let (exp_k, exp_v) = match &expected_kv {
                    Some((k, v)) => (Some(k.clone()), Some(v.clone())),
                    None => (None, None),
                };
                let first_k = self.check_expr(&pairs[0].0, exp_k.as_ref())?;
                let first_v = self.check_expr(&pairs[0].1, exp_v.as_ref())?;
                let (kt, vt) = (first_k.ty.clone(), first_v.ty.clone());
                if kt != Type::Int && kt != Type::Str {
                    return self.err(
                        span,
                        &format!("map key type must be int or string, got {}", kt.name()),
                    );
                }
                if vt == Type::Void {
                    return self.err(span, "map values cannot be void");
                }
                let mut out = vec![(first_k, first_v)];
                for (ke, ve) in &pairs[1..] {
                    let tk = self.check_expr(ke, Some(&kt))?;
                    if tk.ty != kt {
                        return self.err(
                            span,
                            &format!("map key has type {}, expected {}", tk.ty.name(), kt.name()),
                        );
                    }
                    let tv = self.check_expr(ve, Some(&vt))?;
                    if tv.ty != vt {
                        return self.err(
                            span,
                            &format!(
                                "map value has type {}, expected {}",
                                tv.ty.name(),
                                vt.name()
                            ),
                        );
                    }
                    out.push((tk, tv));
                }
                Ok(te(
                    TExprKind::MapLit(out),
                    Type::Map(Box::new(kt), Box::new(vt)),
                ))
            }
            Expr::Bin(op, l, r, span) => {
                let tl = self.check_expr(l, None)?;
                let tr = self.check_expr(r, None)?;
                let ty = bin_result(*op, &tl.ty, &tr.ty).ok_or_else(|| {
                    format!(
                        "{}:{}: operator cannot be applied to {} and {}",
                        span.file,
                        span.line,
                        tl.ty.name(),
                        tr.ty.name()
                    )
                })?;
                Ok(te(
                    TExprKind::Bin(*op, Box::new(tl), Box::new(tr)),
                    ty,
                ))
            }
            Expr::Un(op, e, span) => {
                let t = self.check_expr(e, None)?;
                let ty = match (op, &t.ty) {
                    (UnOp::Neg, Type::Int) => Type::Int,
                    (UnOp::Neg, Type::Float) => Type::Float,
                    (UnOp::Not, Type::Bool) => Type::Bool,
                    _ => {
                        return self.err(
                            span,
                            &format!("unary operator cannot be applied to {}", t.ty.name()),
                        )
                    }
                };
                Ok(te(TExprKind::Un(*op, Box::new(t)), ty))
            }
        }
    }

    fn check_builtin(
        &mut self,
        name: &str,
        args: &[Expr],
        span: &Span,
    ) -> Result<TExpr, String> {
        let arity = |want: &str, got: usize| -> String {
            format!(
                "{}:{}: builtin '{}' expects {} argument(s), got {}",
                span.file, span.line, name, want, got
            )
        };
        match name {
            "print" => {
                let mut targs = Vec::new();
                for a in args {
                    targs.push(self.check_expr(a, None)?);
                }
                Ok(te(TExprKind::Builtin(name.into(), targs), Type::Void))
            }
            "len" => {
                if args.len() != 1 {
                    return Err(arity("1", args.len()));
                }
                let a = self.check_expr(&args[0], None)?;
                match a.ty {
                    Type::Str | Type::Array(_) | Type::Map(_, _) => {
                        Ok(te(TExprKind::Builtin(name.into(), vec![a]), Type::Int))
                    }
                    ref other => self.err(
                        span,
                        &format!(
                            "len() needs a string, an array, or a map, got {}",
                            other.name()
                        ),
                    ),
                }
            }
            "push" => {
                if args.len() != 2 {
                    return Err(arity("2", args.len()));
                }
                let arr = self.check_expr(&args[0], None)?;
                let elem = match &arr.ty {
                    Type::Array(t) => (**t).clone(),
                    other => {
                        return self.err(
                            span,
                            &format!("push() needs an array, got {}", other.name()),
                        )
                    }
                };
                let v = self.check_expr(&args[1], Some(&elem))?;
                if v.ty != elem {
                    return self.err(
                        span,
                        &format!(
                            "push() value has type {}, expected {}",
                            v.ty.name(),
                            elem.name()
                        ),
                    );
                }
                let ty = arr.ty.clone();
                Ok(te(TExprKind::Builtin(name.into(), vec![arr, v]), ty))
            }
            "range" => {
                if args.len() != 1 && args.len() != 2 {
                    return Err(arity("1 or 2", args.len()));
                }
                let mut targs = Vec::new();
                for a in args {
                    let t = self.check_expr(a, Some(&Type::Int))?;
                    if t.ty != Type::Int {
                        return self.err(span, "range() arguments must be int");
                    }
                    targs.push(t);
                }
                Ok(te(
                    TExprKind::Builtin(name.into(), targs),
                    Type::Array(Box::new(Type::Int)),
                ))
            }
            "str" => {
                if args.len() != 1 {
                    return Err(arity("1", args.len()));
                }
                let a = self.check_expr(&args[0], None)?;
                Ok(te(TExprKind::Builtin(name.into(), vec![a]), Type::Str))
            }
            "int" => {
                if args.len() != 1 {
                    return Err(arity("1", args.len()));
                }
                let a = self.check_expr(&args[0], None)?;
                match a.ty {
                    Type::Int | Type::Float | Type::Bool | Type::Str => {
                        Ok(te(TExprKind::Builtin(name.into(), vec![a]), Type::Int))
                    }
                    ref other => self.err(
                        span,
                        &format!("int() cannot convert {}", other.name()),
                    ),
                }
            }
            "float" => {
                if args.len() != 1 {
                    return Err(arity("1", args.len()));
                }
                let a = self.check_expr(&args[0], None)?;
                match a.ty {
                    Type::Int | Type::Float | Type::Bool | Type::Str => {
                        Ok(te(TExprKind::Builtin(name.into(), vec![a]), Type::Float))
                    }
                    ref other => self.err(
                        span,
                        &format!("float() cannot convert {}", other.name()),
                    ),
                }
            }
            "has" => {
                if args.len() != 2 {
                    return Err(arity("2", args.len()));
                }
                let m = self.check_expr(&args[0], None)?;
                let k = match &m.ty {
                    Type::Map(k, _) => (**k).clone(),
                    other => {
                        return self.err(
                            span,
                            &format!("has() needs a map, got {}", other.name()),
                        )
                    }
                };
                let key = self.check_expr(&args[1], Some(&k))?;
                if key.ty != k {
                    return self.err(span, "has() key has the wrong type");
                }
                Ok(te(TExprKind::Builtin(name.into(), vec![m, key]), Type::Bool))
            }
            "keys" => {
                if args.len() != 1 {
                    return Err(arity("1", args.len()));
                }
                let m = self.check_expr(&args[0], None)?;
                match &m.ty {
                    Type::Map(k, _) => {
                        let kt = (**k).clone();
                        Ok(te(
                            TExprKind::Builtin(name.into(), vec![m]),
                            Type::Array(Box::new(kt)),
                        ))
                    }
                    other => self.err(
                        span,
                        &format!("keys() needs a map, got {}", other.name()),
                    ),
                }
            }
            "del" => {
                if args.len() != 2 {
                    return Err(arity("2", args.len()));
                }
                let m = self.check_expr(&args[0], None)?;
                let (k, ty) = match &m.ty {
                    Type::Map(k, _) => ((**k).clone(), m.ty.clone()),
                    other => {
                        return self.err(
                            span,
                            &format!("del() needs a map, got {}", other.name()),
                        )
                    }
                };
                let key = self.check_expr(&args[1], Some(&k))?;
                if key.ty != k {
                    return self.err(span, "del() key has the wrong type");
                }
                Ok(te(TExprKind::Builtin(name.into(), vec![m, key]), ty))
            }
            "split" => {
                if args.len() != 2 {
                    return Err(arity("2", args.len()));
                }
                let s = self.check_expr(&args[0], Some(&Type::Str))?;
                let sep = self.check_expr(&args[1], Some(&Type::Str))?;
                if s.ty != Type::Str || sep.ty != Type::Str {
                    return self.err(span, "split() needs two strings");
                }
                Ok(te(
                    TExprKind::Builtin(name.into(), vec![s, sep]),
                    Type::Array(Box::new(Type::Str)),
                ))
            }
            "raise" => {
                if args.len() != 1 {
                    return Err(arity("1", args.len()));
                }
                let a = self.check_expr(&args[0], Some(&Type::Str))?;
                if a.ty != Type::Str {
                    return self.err(span, "raise() needs a string message");
                }
                Ok(te(TExprKind::Builtin(name.into(), vec![a]), Type::Void))
            }
            "join" => {
                if args.len() != 2 {
                    return Err(arity("2", args.len()));
                }
                let arr = self.check_expr(&args[0], None)?;
                if arr.ty != Type::Array(Box::new(Type::Str)) {
                    return self.err(
                        span,
                        &format!("join() needs a [string] first argument, got {}", arr.ty.name()),
                    );
                }
                let sep = self.check_expr(&args[1], Some(&Type::Str))?;
                if sep.ty != Type::Str {
                    return self.err(span, "join() needs a string separator");
                }
                Ok(te(TExprKind::Builtin(name.into(), vec![arr, sep]), Type::Str))
            }
            "replace" => {
                if args.len() != 3 {
                    return Err(arity("3", args.len()));
                }
                let mut targs = Vec::new();
                for a in args {
                    let t = self.check_expr(a, Some(&Type::Str))?;
                    if t.ty != Type::Str {
                        return self.err(span, "replace() needs three strings");
                    }
                    targs.push(t);
                }
                Ok(te(TExprKind::Builtin(name.into(), targs), Type::Str))
            }
            "substr" => {
                if args.len() != 3 {
                    return Err(arity("3", args.len()));
                }
                let s = self.check_expr(&args[0], Some(&Type::Str))?;
                if s.ty != Type::Str {
                    return self.err(span, "substr() needs a string first argument");
                }
                let mut targs = vec![s];
                for a in &args[1..] {
                    let t = self.check_expr(a, Some(&Type::Int))?;
                    if t.ty != Type::Int {
                        return self.err(span, "substr() start and length must be int");
                    }
                    targs.push(t);
                }
                Ok(te(TExprKind::Builtin(name.into(), targs), Type::Str))
            }
            "index_of" => {
                if args.len() != 2 {
                    return Err(arity("2", args.len()));
                }
                let mut targs = Vec::new();
                for a in args {
                    let t = self.check_expr(a, Some(&Type::Str))?;
                    if t.ty != Type::Str {
                        return self.err(span, "index_of() needs two strings");
                    }
                    targs.push(t);
                }
                Ok(te(TExprKind::Builtin(name.into(), targs), Type::Int))
            }
            "starts_with" => {
                if args.len() != 2 {
                    return Err(arity("2", args.len()));
                }
                let mut targs = Vec::new();
                for a in args {
                    let t = self.check_expr(a, Some(&Type::Str))?;
                    if t.ty != Type::Str {
                        return self.err(span, "starts_with() needs two strings");
                    }
                    targs.push(t);
                }
                Ok(te(TExprKind::Builtin(name.into(), targs), Type::Bool))
            }
            _ => self.err(span, &format!("unknown builtin '{}'", name)),
        }
    }
}

fn bin_result(op: BinOp, l: &Type, r: &Type) -> Option<Type> {
    use Type::*;
    match op {
        BinOp::Add => match (l, r) {
            (Int, Int) => Some(Int),
            (Float, Float) => Some(Float),
            (Str, Str) => Some(Str),
            _ => None,
        },
        BinOp::Sub | BinOp::Mul | BinOp::Div => match (l, r) {
            (Int, Int) => Some(Int),
            (Float, Float) => Some(Float),
            _ => None,
        },
        BinOp::Mod => match (l, r) {
            (Int, Int) => Some(Int),
            _ => None,
        },
        BinOp::Eq | BinOp::Ne => {
            if l == r && *l != Void {
                Some(Bool)
            } else {
                None
            }
        }
        BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => match (l, r) {
            (Int, Int) | (Float, Float) | (Str, Str) => Some(Bool),
            _ => None,
        },
        BinOp::And | BinOp::Or => match (l, r) {
            (Bool, Bool) => Some(Bool),
            _ => None,
        },
    }
}

fn ends_with_return(stmts: &[Stmt]) -> bool {
    match stmts.last() {
        Some(Stmt::Return(Some(_), _)) => true,
        Some(Stmt::If(_, then, Some(els), _)) => {
            ends_with_return(then) && ends_with_return(els)
        }
        _ => false,
    }
}

// Small helper so FnSig can be cloned out of the map.
impl FnSig {
    fn clone_sig(&self) -> FnSig {
        FnSig { params: self.params.clone(), ret: self.ret.clone() }
    }
}
