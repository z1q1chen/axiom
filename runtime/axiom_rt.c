/* Axiom runtime (v0.1). Emitted into every generated C file by axiomc.
   Uniform tagged values; arena allocation (never freed in v0.1). */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>
#include <stdarg.h>

typedef enum { AX_INT, AX_FLOAT, AX_BOOL, AX_STR, AX_ARR, AX_OBJ, AX_VOID } AxTag;

typedef struct AxVal AxVal;
typedef struct { int64_t len; int64_t cap; AxVal *data; } AxArrData;
typedef struct { int32_t type_id; int32_t nfields; AxVal *fields; } AxObjData;

struct AxVal {
    AxTag tag;
    union {
        int64_t i;
        double f;
        int b;
        char *s;
        AxArrData *arr;
        AxObjData *obj;
    } u;
};

static void *ax_alloc(size_t n) {
    void *p = malloc(n ? n : 1);
    if (!p) { fprintf(stderr, "axiom: out of memory\n"); exit(70); }
    return p;
}

static void ax_fail(const char *msg) {
    fprintf(stderr, "axiom: %s\n", msg);
    exit(70);
}

static void ax_contract_fail(const char *kind, const char *owner,
                             const char *text, const char *file, int line) {
    fprintf(stderr, "axiom: contract violated: %s (%s) at %s:%d\n  clause: %s\n",
            kind, owner, file, line, text);
    exit(70);
}

static AxVal ax_int(int64_t v)  { AxVal r; r.tag = AX_INT;   r.u.i = v; return r; }
static AxVal ax_float(double v) { AxVal r; r.tag = AX_FLOAT; r.u.f = v; return r; }
static AxVal ax_bool(int v)     { AxVal r; r.tag = AX_BOOL;  r.u.b = !!v; return r; }
static AxVal ax_void(void)      { AxVal r; r.tag = AX_VOID;  r.u.i = 0; return r; }

static char *ax_strdup(const char *s) {
    size_t n = strlen(s);
    char *p = ax_alloc(n + 1);
    memcpy(p, s, n + 1);
    return p;
}

static AxVal ax_str(const char *s) {
    AxVal r; r.tag = AX_STR; r.u.s = ax_strdup(s); return r;
}

static int ax_truth(AxVal v) {
    if (v.tag != AX_BOOL) ax_fail("type error: expected bool");
    return v.u.b;
}

/* ---- arrays ---- */
static AxVal ax_arr_new(void) {
    AxVal r; AxArrData *a = ax_alloc(sizeof(AxArrData));
    a->len = 0; a->cap = 8; a->data = ax_alloc(sizeof(AxVal) * 8);
    r.tag = AX_ARR; r.u.arr = a; return r;
}
static AxVal ax_arr_push(AxVal arr, AxVal v) {
    if (arr.tag != AX_ARR) ax_fail("type error: push on non-array");
    AxArrData *a = arr.u.arr;
    if (a->len >= a->cap) {
        a->cap *= 2;
        AxVal *nd = ax_alloc(sizeof(AxVal) * (size_t)a->cap);
        memcpy(nd, a->data, sizeof(AxVal) * (size_t)a->len);
        a->data = nd;
    }
    a->data[a->len++] = v;
    return arr;
}
static AxVal ax_arr_get(AxVal arr, int64_t idx) {
    if (arr.tag != AX_ARR) ax_fail("type error: index on non-array");
    if (idx < 0 || idx >= arr.u.arr->len) ax_fail("index out of bounds");
    return arr.u.arr->data[idx];
}
static void ax_arr_set(AxVal arr, int64_t idx, AxVal v) {
    if (arr.tag != AX_ARR) ax_fail("type error: index on non-array");
    if (idx < 0 || idx >= arr.u.arr->len) ax_fail("index out of bounds");
    arr.u.arr->data[idx] = v;
}
static int64_t ax_len(AxVal v) {
    if (v.tag == AX_ARR) return v.u.arr->len;
    if (v.tag == AX_STR) return (int64_t)strlen(v.u.s);
    ax_fail("type error: len of unsupported value");
    return 0;
}

/* ---- objects (structs) ---- */
static AxVal ax_obj_new(int32_t type_id, int32_t nfields) {
    AxVal r; AxObjData *o = ax_alloc(sizeof(AxObjData));
    o->type_id = type_id; o->nfields = nfields;
    o->fields = ax_alloc(sizeof(AxVal) * (size_t)(nfields ? nfields : 1));
    for (int32_t i = 0; i < nfields; i++) o->fields[i] = ax_void();
    r.tag = AX_OBJ; r.u.obj = o; return r;
}
static AxVal ax_obj_get(AxVal obj, int32_t idx) {
    if (obj.tag != AX_OBJ) ax_fail("type error: field access on non-struct");
    return obj.u.obj->fields[idx];
}
static void ax_obj_set(AxVal obj, int32_t idx, AxVal v) {
    if (obj.tag != AX_OBJ) ax_fail("type error: field access on non-struct");
    obj.u.obj->fields[idx] = v;
}

/* ---- arithmetic ---- */
static AxVal ax_add(AxVal a, AxVal b) {
    if (a.tag == AX_INT && b.tag == AX_INT) return ax_int(a.u.i + b.u.i);
    if (a.tag == AX_FLOAT && b.tag == AX_FLOAT) return ax_float(a.u.f + b.u.f);
    if (a.tag == AX_STR && b.tag == AX_STR) {
        size_t na = strlen(a.u.s), nb = strlen(b.u.s);
        char *p = ax_alloc(na + nb + 1);
        memcpy(p, a.u.s, na); memcpy(p + na, b.u.s, nb + 1);
        AxVal r; r.tag = AX_STR; r.u.s = p; return r;
    }
    ax_fail("type error: bad operands for +");
    return ax_void();
}
static AxVal ax_sub(AxVal a, AxVal b) {
    if (a.tag == AX_INT && b.tag == AX_INT) return ax_int(a.u.i - b.u.i);
    if (a.tag == AX_FLOAT && b.tag == AX_FLOAT) return ax_float(a.u.f - b.u.f);
    ax_fail("type error: bad operands for -");
    return ax_void();
}
static AxVal ax_mul(AxVal a, AxVal b) {
    if (a.tag == AX_INT && b.tag == AX_INT) return ax_int(a.u.i * b.u.i);
    if (a.tag == AX_FLOAT && b.tag == AX_FLOAT) return ax_float(a.u.f * b.u.f);
    ax_fail("type error: bad operands for *");
    return ax_void();
}
static AxVal ax_div(AxVal a, AxVal b) {
    if (a.tag == AX_INT && b.tag == AX_INT) {
        if (b.u.i == 0) ax_fail("division by zero");
        return ax_int(a.u.i / b.u.i);
    }
    if (a.tag == AX_FLOAT && b.tag == AX_FLOAT) {
        if (b.u.f == 0.0) ax_fail("division by zero");
        return ax_float(a.u.f / b.u.f);
    }
    ax_fail("type error: bad operands for /");
    return ax_void();
}
static AxVal ax_mod(AxVal a, AxVal b) {
    if (a.tag == AX_INT && b.tag == AX_INT) {
        if (b.u.i == 0) ax_fail("division by zero");
        return ax_int(a.u.i % b.u.i);
    }
    ax_fail("type error: bad operands for %");
    return ax_void();
}
static AxVal ax_neg(AxVal a) {
    if (a.tag == AX_INT) return ax_int(-a.u.i);
    if (a.tag == AX_FLOAT) return ax_float(-a.u.f);
    ax_fail("type error: bad operand for unary -");
    return ax_void();
}

/* ---- comparison ---- */
static int ax_cmp_num(AxVal a, AxVal b) {
    double x = (a.tag == AX_INT) ? (double)a.u.i : a.u.f;
    double y = (b.tag == AX_INT) ? (double)b.u.i : b.u.f;
    return (x < y) ? -1 : (x > y) ? 1 : 0;
}
static AxVal ax_eq(AxVal a, AxVal b) {
    if (a.tag != b.tag) return ax_bool(0);
    switch (a.tag) {
        case AX_INT:   return ax_bool(a.u.i == b.u.i);
        case AX_FLOAT: return ax_bool(a.u.f == b.u.f);
        case AX_BOOL:  return ax_bool(a.u.b == b.u.b);
        case AX_STR:   return ax_bool(strcmp(a.u.s, b.u.s) == 0);
        case AX_ARR:   return ax_bool(a.u.arr == b.u.arr);
        case AX_OBJ:   return ax_bool(a.u.obj == b.u.obj);
        case AX_VOID:  return ax_bool(1);
    }
    return ax_bool(0);
}
static AxVal ax_lt(AxVal a, AxVal b) {
    if (a.tag == AX_STR && b.tag == AX_STR) return ax_bool(strcmp(a.u.s, b.u.s) < 0);
    if ((a.tag == AX_INT || a.tag == AX_FLOAT) && (b.tag == AX_INT || b.tag == AX_FLOAT))
        return ax_bool(ax_cmp_num(a, b) < 0);
    ax_fail("type error: bad operands for <");
    return ax_void();
}
static AxVal ax_le(AxVal a, AxVal b) {
    if (a.tag == AX_STR && b.tag == AX_STR) return ax_bool(strcmp(a.u.s, b.u.s) <= 0);
    if ((a.tag == AX_INT || a.tag == AX_FLOAT) && (b.tag == AX_INT || b.tag == AX_FLOAT))
        return ax_bool(ax_cmp_num(a, b) <= 0);
    ax_fail("type error: bad operands for <=");
    return ax_void();
}
static AxVal ax_gt(AxVal a, AxVal b) { return ax_lt(b, a); }
static AxVal ax_ge(AxVal a, AxVal b) { return ax_le(b, a); }

/* ---- conversions and printing ---- */
static char *ax_str_of(AxVal v) {
    char buf[64];
    switch (v.tag) {
        case AX_INT:   snprintf(buf, sizeof buf, "%lld", (long long)v.u.i); return ax_strdup(buf);
        case AX_FLOAT: snprintf(buf, sizeof buf, "%g", v.u.f); return ax_strdup(buf);
        case AX_BOOL:  return ax_strdup(v.u.b ? "true" : "false");
        case AX_STR:   return v.u.s;
        case AX_VOID:  return ax_strdup("void");
        case AX_ARR: {
            /* "[a, b, c]" built with a growable buffer */
            size_t cap = 128, n = 0;
            char *out = ax_alloc(cap);
            out[n++] = '[';
            for (int64_t i = 0; i < v.u.arr->len; i++) {
                char *s = ax_str_of(v.u.arr->data[i]);
                size_t m = strlen(s);
                while (n + m + 4 > cap) { cap *= 2; char *nd = ax_alloc(cap); memcpy(nd, out, n); out = nd; }
                if (i) { out[n++] = ','; out[n++] = ' '; }
                memcpy(out + n, s, m); n += m;
            }
            if (n + 2 > cap) { cap += 8; char *nd = ax_alloc(cap); memcpy(nd, out, n); out = nd; }
            out[n++] = ']'; out[n] = 0;
            return out;
        }
        case AX_OBJ: {
            snprintf(buf, sizeof buf, "struct#%d", (int)v.u.obj->type_id);
            return ax_strdup(buf);
        }
    }
    return ax_strdup("?");
}

static void ax_print_val(AxVal v) { fputs(ax_str_of(v), stdout); }

static void ax_print_line(int n, ...) {
    va_list ap; va_start(ap, n);
    for (int i = 0; i < n; i++) {
        AxVal v = va_arg(ap, AxVal);
        if (i) putchar(' ');
        ax_print_val(v);
    }
    va_end(ap);
    putchar('\n');
}

static AxVal ax_to_int(AxVal v) {
    switch (v.tag) {
        case AX_INT:   return v;
        case AX_FLOAT: return ax_int((int64_t)v.u.f);
        case AX_BOOL:  return ax_int(v.u.b ? 1 : 0);
        case AX_STR:   return ax_int(strtoll(v.u.s, NULL, 10));
        default: ax_fail("type error: int() on unsupported value"); return ax_void();
    }
}
static AxVal ax_to_float(AxVal v) {
    switch (v.tag) {
        case AX_FLOAT: return v;
        case AX_INT:   return ax_float((double)v.u.i);
        case AX_BOOL:  return ax_float(v.u.b ? 1.0 : 0.0);
        case AX_STR:   return ax_float(strtod(v.u.s, NULL));
        default: ax_fail("type error: float() on unsupported value"); return ax_void();
    }
}
static AxVal ax_range(int64_t a, int64_t b) {
    AxVal r = ax_arr_new();
    for (int64_t i = a; i < b; i++) ax_arr_push(r, ax_int(i));
    return r;
}
static void ax_raise(AxVal msg) {
    if (msg.tag == AX_STR) ax_fail(msg.u.s);
    ax_fail("raised");
}
/* end of Axiom runtime */
