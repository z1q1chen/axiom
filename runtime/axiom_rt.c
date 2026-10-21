/* Axiom runtime (v0.1). Emitted into every generated C file by axiomc.
   Uniform tagged values; arena allocation (never freed in v0.1). */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>
#include <stdarg.h>
#include <unistd.h>
#include <strings.h>
#include <sys/socket.h>
#include <netinet/in.h>

typedef enum { AX_INT, AX_FLOAT, AX_BOOL, AX_STR, AX_ARR, AX_OBJ, AX_MAP, AX_VOID } AxTag;

typedef struct AxVal AxVal;
typedef struct { int64_t len; int64_t cap; AxVal *data; } AxArrData;
typedef struct { int32_t type_id; int32_t nfields; AxVal *fields; } AxObjData;
typedef struct { int64_t len; int64_t cap; AxVal *keys; AxVal *vals; } AxMapData;

struct AxVal {
    AxTag tag;
    union {
        int64_t i;
        double f;
        int b;
        char *s;
        AxArrData *arr;
        AxObjData *obj;
        AxMapData *map;
    } u;
};

/* ---- allocation registry and garbage collection ----
   Every heap block carries a header recording its kind, so the
   marker can traverse values precisely:
     AXK_BYTES  leaf bytes (string payloads, scratch buffers)
     AXK_VALS   an array of AxVal (array/object/map buffers)
     AXK_ARR    AxArrData  (traverse its data buffer)
     AXK_OBJ    AxObjData  (traverse its fields buffer)
     AXK_MAP    AxMapData  (traverse its key/value buffers)
   Collection only runs at generated statement boundaries when
   the program was built with --gc: at those points every live
   value is reachable from registered globals, the shadow root
   stack (parameters and locals), or the temp root stack
   (intermediate values of the current statement). */
#define AXK_BYTES 0
#define AXK_VALS 1
#define AXK_ARR 2
#define AXK_OBJ 3
#define AXK_MAP 4

typedef struct AxBlock {
    struct AxBlock *next;
    int kind;
    int marked;
    size_t size;
} AxBlock;

static AxBlock *ax_blocks = NULL;
static long ax_alloc_since_gc = 0;
static long ax_gc_threshold = 10000;
static int ax_gc_on = 0;
static long ax_gc_collections = 0;

static void *ax_alloc_kind(size_t n, int kind) {
    AxBlock *b = (AxBlock *)malloc(sizeof(AxBlock) + (n ? n : 1));
    if (!b) { fprintf(stderr, "axiom: out of memory\n"); exit(70); }
    b->next = ax_blocks;
    b->kind = kind;
    b->marked = 0;
    b->size = n;
    ax_blocks = b;
    ax_alloc_since_gc++;
    void *user = (void *)(b + 1);
    if (kind == AXK_VALS) {
        // Value buffers hold spare capacity beyond the live
        // length; zero it so the marker sees harmless AX_INT
        // slots (tag 0) instead of uninitialized memory.
        memset(user, 0, n);
    }
    return user;
}

static void *ax_alloc(size_t n) { return ax_alloc_kind(n, AXK_BYTES); }

static AxBlock *ax_block_of(void *user_ptr) {
    return ((AxBlock *)user_ptr) - 1;
}

/* Root registration: globals (once, from the entry point) and
   two runtime stacks maintained by generated code. */
#define AX_ROOTS_MAX 4096
static AxVal *ax_global_roots[AX_ROOTS_MAX];
static int ax_global_root_count = 0;
static void ax_root_global(AxVal *slot) {
    if (ax_global_root_count < AX_ROOTS_MAX)
        ax_global_roots[ax_global_root_count++] = slot;
}

static AxVal **ax_root_stack = NULL;
static long ax_root_sp = 0, ax_root_cap = 0;
static void ax_root_push(AxVal *slot) {
    if (ax_root_sp == ax_root_cap) {
        ax_root_cap = ax_root_cap ? ax_root_cap * 2 : 1024;
        ax_root_stack = (AxVal **)realloc(ax_root_stack, sizeof(AxVal *) * (size_t)ax_root_cap);
        if (!ax_root_stack) { fprintf(stderr, "axiom: out of memory\n"); exit(70); }
    }
    ax_root_stack[ax_root_sp++] = slot;
}
static void ax_root_pop(long n) { ax_root_sp -= n; }

static AxVal **ax_temp_stack = NULL;
static long ax_temp_sp = 0, ax_temp_cap = 0;
static void ax_temp_push(AxVal *slot) {
    if (ax_temp_sp == ax_temp_cap) {
        ax_temp_cap = ax_temp_cap ? ax_temp_cap * 2 : 1024;
        ax_temp_stack = (AxVal **)realloc(ax_temp_stack, sizeof(AxVal *) * (size_t)ax_temp_cap);
        if (!ax_temp_stack) { fprintf(stderr, "axiom: out of memory\n"); exit(70); }
    }
    ax_temp_stack[ax_temp_sp++] = slot;
}
static long ax_temp_mark(void) { return ax_temp_sp; }
static void ax_temp_pop(long mark) { ax_temp_sp = mark; }

static void ax_mark_value(AxVal v);
static void ax_mark_ptr(void *user_ptr) {
    if (!user_ptr) return;
    AxBlock *b = ax_block_of(user_ptr);
    if (b->marked) return;
    b->marked = 1;
    if (b->kind == AXK_VALS) {
        AxVal *vals = (AxVal *)user_ptr;
        size_t count = b->size / sizeof(AxVal);
        for (size_t i = 0; i < count; i++) ax_mark_value(vals[i]);
    } else if (b->kind == AXK_ARR) {
        AxArrData *a = (AxArrData *)user_ptr;
        ax_mark_ptr(a->data);
    } else if (b->kind == AXK_OBJ) {
        AxObjData *o = (AxObjData *)user_ptr;
        ax_mark_ptr(o->fields);
    } else if (b->kind == AXK_MAP) {
        AxMapData *m = (AxMapData *)user_ptr;
        ax_mark_ptr(m->keys);
        ax_mark_ptr(m->vals);
    }
}
static void ax_mark_value(AxVal v) {
    switch (v.tag) {
        case AX_STR: ax_mark_ptr(v.u.s); break;
        case AX_ARR: ax_mark_ptr(v.u.arr); break;
        case AX_OBJ: ax_mark_ptr(v.u.obj); break;
        case AX_MAP: ax_mark_ptr(v.u.map); break;
        default: break;
    }
}

static long ax_gc_collect_now(void) {
    for (int i = 0; i < ax_global_root_count; i++)
        ax_mark_value(*ax_global_roots[i]);
    for (long i = 0; i < ax_root_sp; i++)
        ax_mark_value(*ax_root_stack[i]);
    for (long i = 0; i < ax_temp_sp; i++)
        ax_mark_value(*ax_temp_stack[i]);
    long live = 0;
    AxBlock **pp = &ax_blocks;
    while (*pp) {
        AxBlock *b = *pp;
        if (b->marked) {
            b->marked = 0;
            live++;
            pp = &b->next;
        } else {
            *pp = b->next;
            free(b);
        }
    }
    ax_alloc_since_gc = 0;
    ax_gc_collections++;
    return live;
}

static void ax_gc_enable(void) {
    const char *t = getenv("AXIOM_GC_THRESHOLD");
    if (t && t[0]) {
        long v = strtol(t, NULL, 10);
        if (v > 0) ax_gc_threshold = v;
    }
    ax_gc_on = 1;
}

static void ax_gc_maybe(void) {
    if (ax_gc_on && ax_alloc_since_gc >= ax_gc_threshold)
        ax_gc_collect_now();
}

static AxVal ax_int(int64_t v)  { AxVal r; r.tag = AX_INT;   r.u.i = v; return r; }
static AxVal ax_float(double v) { AxVal r; r.tag = AX_FLOAT; r.u.f = v; return r; }
static AxVal ax_bool(int v)     { AxVal r; r.tag = AX_BOOL;  r.u.b = !!v; return r; }
static AxVal ax_void(void)      { AxVal r; r.tag = AX_VOID;  r.u.i = 0; return r; }

/* Builtin gc_collect(): forces a collection and returns the
   number of live blocks. Only meaningful in --gc builds (the
   root stacks exist only there); elsewhere it is a no-op
   returning -1 so a stray call can never free live memory. */
static AxVal ax_gc_collect_builtin(void) {
    if (!ax_gc_on) return ax_int(-1);
    return ax_int(ax_gc_collect_now());
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
    AxVal r; AxArrData *a = ax_alloc_kind(sizeof(AxArrData), AXK_ARR);
    a->len = 0; a->cap = 8; a->data = ax_alloc_kind(sizeof(AxVal) * 8, AXK_VALS);
    r.tag = AX_ARR; r.u.arr = a; return r;
}
static AxVal ax_arr_push(AxVal arr, AxVal v) {
    if (arr.tag != AX_ARR) ax_fail("type error: push on non-array");
    AxArrData *a = arr.u.arr;
    if (a->len >= a->cap) {
        a->cap *= 2;
        AxVal *nd = ax_alloc_kind(sizeof(AxVal) * (size_t)a->cap, AXK_VALS);
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
    if (v.tag == AX_MAP) return v.u.map->len;
    ax_fail("type error: len of unsupported value");
    return 0;
}

/* ---- objects (structs) ---- */
static AxVal ax_obj_new(int32_t type_id, int32_t nfields) {
    AxVal r; AxObjData *o = ax_alloc_kind(sizeof(AxObjData), AXK_OBJ);
    o->type_id = type_id; o->nfields = nfields;
    o->fields = ax_alloc_kind(sizeof(AxVal) * (size_t)(nfields ? nfields : 1), AXK_VALS);
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
        case AX_MAP:   return ax_bool(a.u.map == b.u.map);
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

/* ---- maps (linear scan; v0.2 scale) ---- */
static AxVal ax_map_new(void) {
    AxVal r; AxMapData *m = ax_alloc_kind(sizeof(AxMapData), AXK_MAP);
    m->len = 0; m->cap = 8;
    m->keys = ax_alloc_kind(sizeof(AxVal) * 8, AXK_VALS);
    m->vals = ax_alloc_kind(sizeof(AxVal) * 8, AXK_VALS);
    r.tag = AX_MAP; r.u.map = m; return r;
}
static int64_t ax_map_find(AxVal map, AxVal key) {
    AxMapData *m = map.u.map;
    for (int64_t i = 0; i < m->len; i++) {
        if (ax_truth(ax_eq(m->keys[i], key))) return i;
    }
    return -1;
}
static AxVal ax_map_set(AxVal map, AxVal key, AxVal val) {
    if (map.tag != AX_MAP) ax_fail("type error: index assignment on non-map");
    if (key.tag != AX_INT && key.tag != AX_STR)
        ax_fail("type error: map key must be int or string");
    AxMapData *m = map.u.map;
    int64_t at = ax_map_find(map, key);
    if (at >= 0) { m->vals[at] = val; return map; }
    if (m->len >= m->cap) {
        m->cap *= 2;
        AxVal *nk = ax_alloc_kind(sizeof(AxVal) * (size_t)m->cap, AXK_VALS);
        AxVal *nv = ax_alloc_kind(sizeof(AxVal) * (size_t)m->cap, AXK_VALS);
        memcpy(nk, m->keys, sizeof(AxVal) * (size_t)m->len);
        memcpy(nv, m->vals, sizeof(AxVal) * (size_t)m->len);
        m->keys = nk; m->vals = nv;
    }
    m->keys[m->len] = key;
    m->vals[m->len] = val;
    m->len++;
    return map;
}
static AxVal ax_map_get(AxVal map, AxVal key) {
    if (map.tag != AX_MAP) ax_fail("type error: index on non-map");
    int64_t at = ax_map_find(map, key);
    if (at < 0) ax_fail("map key not found");
    return map.u.map->vals[at];
}
static int ax_map_has(AxVal map, AxVal key) {
    if (map.tag != AX_MAP) ax_fail("type error: has() on non-map");
    return ax_map_find(map, key) >= 0;
}
static AxVal ax_map_del(AxVal map, AxVal key) {
    if (map.tag != AX_MAP) ax_fail("type error: del() on non-map");
    AxMapData *m = map.u.map;
    int64_t at = ax_map_find(map, key);
    if (at >= 0) {
        for (int64_t i = at; i + 1 < m->len; i++) {
            m->keys[i] = m->keys[i + 1];
            m->vals[i] = m->vals[i + 1];
        }
        m->len--;
    }
    return map;
}
static AxVal ax_map_keys(AxVal map) {
    if (map.tag != AX_MAP) ax_fail("type error: keys() on non-map");
    AxVal r = ax_arr_new();
    AxMapData *m = map.u.map;
    for (int64_t i = 0; i < m->len; i++) ax_arr_push(r, m->keys[i]);
    return r;
}

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
        case AX_MAP: {
            size_t cap = 128, n = 0;
            char *out = ax_alloc(cap);
            out[n++] = '{';
            AxMapData *m = v.u.map;
            for (int64_t i = 0; i < m->len; i++) {
                char *ks = ax_str_of(m->keys[i]);
                char *vs = ax_str_of(m->vals[i]);
                size_t need = strlen(ks) + strlen(vs) + 4;
                while (n + need + 2 > cap) { cap *= 2; char *nd = ax_alloc(cap); memcpy(nd, out, n); out = nd; }
                if (i) { out[n++] = ','; out[n++] = ' '; }
                size_t kl = strlen(ks); memcpy(out + n, ks, kl); n += kl;
                out[n++] = ':'; out[n++] = ' ';
                size_t vl = strlen(vs); memcpy(out + n, vs, vl); n += vl;
            }
            if (n + 2 > cap) { cap += 8; char *nd = ax_alloc(cap); memcpy(nd, out, n); out = nd; }
            out[n++] = '}'; out[n] = 0;
            return out;
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
static char *ax_dup_range(const char *s, size_t n) {
    char *p = ax_alloc(n + 1);
    memcpy(p, s, n);
    p[n] = 0;
    return p;
}
static AxVal ax_split(AxVal str, AxVal sep) {
    if (str.tag != AX_STR || sep.tag != AX_STR)
        ax_fail("type error: split() needs two strings");
    if (sep.u.s[0] == 0) ax_fail("split() separator must not be empty");
    AxVal r = ax_arr_new();
    const char *s = str.u.s;
    size_t sl = strlen(sep.u.s);
    const char *cur = s;
    for (;;) {
        const char *hit = strstr(cur, sep.u.s);
        AxVal part;
        part.tag = AX_STR;
        if (!hit) {
            part.u.s = ax_dup_range(cur, strlen(cur));
            ax_arr_push(r, part);
            break;
        }
        part.u.s = ax_dup_range(cur, (size_t)(hit - cur));
        ax_arr_push(r, part);
        cur = hit + sl;
    }
    return r;
}
static void ax_raise(AxVal msg) {
    if (msg.tag == AX_STR) ax_fail(msg.u.s);
    ax_fail("raised");
}
static AxVal ax_join(AxVal arr, AxVal sep) {
    if (arr.tag != AX_ARR || sep.tag != AX_STR)
        ax_fail("type error: join() needs [string] and a string");
    size_t cap = 128, n = 0;
    char *out = ax_alloc(cap);
    size_t sl = strlen(sep.u.s);
    for (int64_t i = 0; i < arr.u.arr->len; i++) {
        AxVal el = arr.u.arr->data[i];
        if (el.tag != AX_STR) ax_fail("type error: join() element is not a string");
        size_t m = strlen(el.u.s);
        while (n + m + sl + 1 > cap) {
            cap *= 2;
            char *nd = ax_alloc(cap);
            memcpy(nd, out, n);
            out = nd;
        }
        if (i) { memcpy(out + n, sep.u.s, sl); n += sl; }
        memcpy(out + n, el.u.s, m); n += m;
    }
    out[n] = 0;
    AxVal r; r.tag = AX_STR; r.u.s = out; return r;
}
static AxVal ax_replace(AxVal s, AxVal from, AxVal to) {
    if (s.tag != AX_STR || from.tag != AX_STR || to.tag != AX_STR)
        ax_fail("type error: replace() needs three strings");
    if (from.u.s[0] == 0) return s;
    size_t cap = strlen(s.u.s) + 64, n = 0;
    char *out = ax_alloc(cap);
    size_t fl = strlen(from.u.s), tl = strlen(to.u.s);
    const char *cur = s.u.s;
    for (;;) {
        const char *hit = strstr(cur, from.u.s);
        size_t chunk = hit ? (size_t)(hit - cur) : strlen(cur);
        while (n + chunk + tl + 1 > cap) {
            cap *= 2;
            char *nd = ax_alloc(cap);
            memcpy(nd, out, n);
            out = nd;
        }
        memcpy(out + n, cur, chunk); n += chunk;
        if (!hit) break;
        memcpy(out + n, to.u.s, tl); n += tl;
        cur = hit + fl;
    }
    out[n] = 0;
    AxVal r; r.tag = AX_STR; r.u.s = out; return r;
}

static AxVal ax_substr(AxVal s, int64_t start, int64_t n) {
    if (s.tag != AX_STR) ax_fail("type error: substr() on non-string");
    int64_t len = (int64_t)strlen(s.u.s);
    if (start < 0) start = 0;
    if (start > len) start = len;
    if (n < 0) n = 0;
    if (start + n > len) n = len - start;
    AxVal r; r.tag = AX_STR; r.u.s = ax_dup_range(s.u.s + start, (size_t)n);
    return r;
}
static int64_t ax_index_of(AxVal s, AxVal sub) {
    if (s.tag != AX_STR || sub.tag != AX_STR)
        ax_fail("type error: index_of() needs two strings");
    if (sub.u.s[0] == 0) return 0;
    const char *hit = strstr(s.u.s, sub.u.s);
    return hit ? (int64_t)(hit - s.u.s) : -1;
}
static int ax_starts_with(AxVal s, AxVal prefix) {
    if (s.tag != AX_STR || prefix.tag != AX_STR)
        ax_fail("type error: starts_with() needs two strings");
    return strncmp(s.u.s, prefix.u.s, strlen(prefix.u.s)) == 0;
}
/* chr(code): the one-byte string for a byte value (code & 0xFF).
   ord(s): the first byte of s as an int, -1 for "". Byte-level
   on purpose: they exist for parsers (JSON, URL decoding). */
static AxVal ax_chr(int64_t code) {
    char buf[2];
    buf[0] = (char)(code & 0xFF);
    buf[1] = 0;
    return ax_str(buf);
}
static int64_t ax_ord_of(AxVal s) {
    if (s.tag != AX_STR) ax_fail("type error: ord() needs a string");
    if (s.u.s[0] == 0) return -1;
    return (unsigned char)s.u.s[0];
}

/* ---- native bindings (extern fn implementations) ----
   Every `extern fn name(...)` in Axiom binds to axn_name here.
   axiomc refuses to build when one is missing. */
static char *ax_read_all(FILE *fp, size_t *out_len) {
    size_t cap = 8192, n = 0;
    char *buf = ax_alloc(cap);
    for (;;) {
        if (n == cap) {
            cap *= 2;
            char *nd = ax_alloc(cap);
            memcpy(nd, buf, n);
            buf = nd;
        }
        size_t got = fread(buf + n, 1, cap - n, fp);
        n += got;
        if (got == 0) break;
    }
    if (out_len) *out_len = n;
    char *done = ax_alloc(n + 1);
    memcpy(done, buf, n);
    done[n] = 0;
    return done;
}
static AxVal axn_read_file(AxVal path) {
    if (path.tag != AX_STR) ax_fail("type error: read_file path");
    FILE *fp = fopen(path.u.s, "rb");
    if (!fp) {
        fprintf(stderr, "axiom: cannot read file '%s'\n", path.u.s);
        exit(70);
    }
    char *data = ax_read_all(fp, NULL);
    fclose(fp);
    AxVal r; r.tag = AX_STR; r.u.s = data; return r;
}
static AxVal axn_write_file(AxVal path, AxVal data) {
    if (path.tag != AX_STR || data.tag != AX_STR)
        ax_fail("type error: write_file arguments");
    FILE *fp = fopen(path.u.s, "wb");
    if (!fp) return ax_bool(0);
    size_t n = strlen(data.u.s);
    int ok = fwrite(data.u.s, 1, n, fp) == n;
    if (fclose(fp) != 0) ok = 0;
    return ax_bool(ok);
}
static AxVal axn_append_file(AxVal path, AxVal data) {
    if (path.tag != AX_STR || data.tag != AX_STR)
        ax_fail("type error: append_file arguments");
    FILE *fp = fopen(path.u.s, "ab");
    if (!fp) return ax_bool(0);
    size_t n = strlen(data.u.s);
    int ok = fwrite(data.u.s, 1, n, fp) == n;
    if (fclose(fp) != 0) ok = 0;
    return ax_bool(ok);
}
static AxVal axn_file_exists(AxVal path) {
    if (path.tag != AX_STR) ax_fail("type error: file_exists path");
    FILE *fp = fopen(path.u.s, "rb");
    if (!fp) return ax_bool(0);
    fclose(fp);
    return ax_bool(1);
}
static AxVal axn_read_line(void) {
    size_t cap = 256, n = 0;
    char *buf = ax_alloc(cap);
    int c;
    while ((c = getchar()) != EOF && c != '\n') {
        if (n + 1 >= cap) {
            cap *= 2;
            char *nd = ax_alloc(cap);
            memcpy(nd, buf, n);
            buf = nd;
        }
        buf[n++] = (char)c;
    }
    if (c == EOF && n == 0) {
        AxVal r; r.tag = AX_STR; r.u.s = (char *)""; return r;
    }
    if (n > 0 && buf[n - 1] == '\r') n--;
    buf[n] = 0;
    AxVal r; r.tag = AX_STR; r.u.s = buf; return r;
}
static AxVal axn_env_get(AxVal name) {
    if (name.tag != AX_STR) ax_fail("type error: env_get name");
    const char *v = getenv(name.u.s);
    return ax_str(v ? v : "");
}

/* ==== capability: http (pull-model server) ====
   The Axiom program owns the accept loop: http_listen returns
   a server handle, http_accept blocks for one request and
   returns a request handle, the program reads the request
   through the http_req_* natives, then answers once with
   http_respond, which sends the response and closes the
   connection (HTTP/1.1 with Connection: close; one request per
   connection). Internal buffers use plain malloc and are freed
   on respond; only copies handed to Axiom use ax_alloc, so the
   GC sees exactly the strings the program holds. Limits:
   64 KB of headers, 1 MB of body, 128 pending request slots.
   The path is NOT percent-decoded (query values are decoded
   Axiom-side by the framework); the server is single-threaded
   and sequential by design. */

#define AX_HTTP_SLOTS 128
#define AX_HTTP_HDR_MAX 65536
#define AX_HTTP_BODY_MAX 1048576

typedef struct {
    int used;
    int fd;
    char *method;
    char *path;
    char *query;
    char *headers; /* raw header block, request line excluded */
    char *body;
} AxHttpSlot;

static AxHttpSlot ax_http_slots[AX_HTTP_SLOTS];

static AxHttpSlot *ax_http_get(AxVal h) {
    if (h.tag != AX_INT) return NULL;
    int64_t i = h.u.i;
    if (i < 1 || i > AX_HTTP_SLOTS) return NULL;
    AxHttpSlot *s = &ax_http_slots[i - 1];
    return s->used ? s : NULL;
}

static char *ax_http_dup_range(const char *p, size_t n) {
    char *r = (char *)malloc(n + 1);
    if (!r) { fprintf(stderr, "axiom: out of memory\n"); exit(70); }
    memcpy(r, p, n);
    r[n] = 0;
    return r;
}

/* Read until needle appears in buf or the cap is hit.
   Returns the total bytes read, or -1 on error/EOF. */
static long ax_http_read_until(int fd, char *buf, size_t cap, const char *needle) {
    size_t n = 0;
    size_t nlen = strlen(needle);
    while (n < cap) {
        ssize_t r = recv(fd, buf + n, cap - n, 0);
        if (r <= 0) return n > 0 ? (long)n : -1;
        n += (size_t)r;
        buf[n] = 0;
        if (n >= nlen && strstr(buf, needle)) break;
    }
    return (long)n;
}

static AxVal axn_http_listen(AxVal port) {
    if (port.tag != AX_INT) ax_fail("type error: http_listen port");
    int fd = socket(AF_INET, SOCK_STREAM, 0);
    if (fd < 0) return ax_int(-1);
    int one = 1;
    setsockopt(fd, SOL_SOCKET, SO_REUSEADDR, &one, sizeof(one));
    struct sockaddr_in addr;
    memset(&addr, 0, sizeof(addr));
    addr.sin_family = AF_INET;
    addr.sin_addr.s_addr = INADDR_ANY;
    addr.sin_port = htons((uint16_t)port.u.i);
    if (bind(fd, (struct sockaddr *)&addr, sizeof(addr)) != 0 ||
        listen(fd, 32) != 0) {
        close(fd);
        return ax_int(-1);
    }
    return ax_int(fd);
}

static AxVal axn_http_accept(AxVal server) {
    if (server.tag != AX_INT) ax_fail("type error: http_accept server");
    int cfd = accept((int)server.u.i, NULL, NULL);
    if (cfd < 0) return ax_int(0);
    char *raw = (char *)malloc(AX_HTTP_HDR_MAX + AX_HTTP_BODY_MAX + 2);
    if (!raw) { close(cfd); return ax_int(0); }
    long n = ax_http_read_until(cfd, raw, AX_HTTP_HDR_MAX + 1, "\r\n\r\n");
    if (n <= 0) { free(raw); close(cfd); return ax_int(0); }
    char *hdr_end = strstr(raw, "\r\n\r\n");
    size_t hdr_len = (size_t)(hdr_end - raw) + 4;
    /* Content-Length, case-insensitive key scan. */
    long content_len = 0;
    {
        const char *p = raw;
        while (p < hdr_end) {
            const char *eol = strstr(p, "\r\n");
            if (!eol) break;
            if (strncasecmp(p, "Content-Length:", 15) == 0)
                content_len = strtol(p + 15, NULL, 10);
            p = eol + 2;
        }
    }
    if (content_len < 0) content_len = 0;
    if (content_len > AX_HTTP_BODY_MAX) content_len = AX_HTTP_BODY_MAX;
    size_t have_body = (size_t)n - hdr_len;
    while ((long)have_body < content_len) {
        ssize_t r = recv(cfd, raw + hdr_len + have_body,
                         (size_t)content_len - have_body, 0);
        if (r <= 0) break;
        have_body += (size_t)r;
    }
    raw[hdr_len + have_body] = 0;
    /* Request line: METHOD SP TARGET SP VERSION */
    char *sp1 = strchr(raw, ' ');
    char *sp2 = sp1 ? strchr(sp1 + 1, ' ') : NULL;
    char *le = strstr(raw, "\r\n");
    if (!sp1 || !sp2 || !le) { free(raw); close(cfd); return ax_int(0); }
    char *method = ax_http_dup_range(raw, (size_t)(sp1 - raw));
    char *target = ax_http_dup_range(sp1 + 1, (size_t)(sp2 - sp1 - 1));
    char *headers = ax_http_dup_range(le + 2, (size_t)(hdr_end - (le + 2)));
    char *body = ax_http_dup_range(raw + hdr_len, have_body);
    free(raw);
    char *q = strchr(target, '?');
    char *path = target;
    char *query = (char *)"";
    if (q) { *q = 0; query = q + 1; }
    int slot = -1;
    for (int i = 0; i < AX_HTTP_SLOTS; i++) {
        if (!ax_http_slots[i].used) { slot = i; break; }
    }
    if (slot < 0) {
        free(method); free(target); free(headers); free(body);
        close(cfd);
        return ax_int(0);
    }
    AxHttpSlot *s = &ax_http_slots[slot];
    s->used = 1;
    s->fd = cfd;
    s->method = method;
    s->path = path;
    s->query = ax_http_dup_range(query, strlen(query));
    s->headers = headers;
    s->body = body;
    return ax_int(slot + 1);
}

static const char *ax_http_reason(int64_t code) {
    switch (code) {
        case 200: return "OK";
        case 201: return "Created";
        case 204: return "No Content";
        case 400: return "Bad Request";
        case 404: return "Not Found";
        case 405: return "Method Not Allowed";
        case 409: return "Conflict";
        case 422: return "Unprocessable Entity";
        case 500: return "Internal Server Error";
        default: return "Status";
    }
}

static AxVal axn_http_req_method(AxVal h) {
    AxHttpSlot *s = ax_http_get(h);
    return ax_str(s ? s->method : "");
}
static AxVal axn_http_req_path(AxVal h) {
    AxHttpSlot *s = ax_http_get(h);
    return ax_str(s ? s->path : "");
}
static AxVal axn_http_req_query(AxVal h) {
    AxHttpSlot *s = ax_http_get(h);
    return ax_str(s ? s->query : "");
}
static AxVal axn_http_req_body(AxVal h) {
    AxHttpSlot *s = ax_http_get(h);
    return ax_str(s ? s->body : "");
}
static AxVal axn_http_req_header(AxVal h, AxVal name) {
    AxHttpSlot *s = ax_http_get(h);
    if (!s || name.tag != AX_STR) return ax_str("");
    size_t nl = strlen(name.u.s);
    const char *p = s->headers;
    while (p && *p) {
        const char *eol = strstr(p, "\r\n");
        size_t linelen = eol ? (size_t)(eol - p) : strlen(p);
        if (linelen > nl + 1 && p[nl] == ':' &&
            strncasecmp(p, name.u.s, nl) == 0) {
            const char *v = p + nl + 1;
            const char *vend = p + linelen;
            while (v < vend && (*v == ' ' || *v == '\t')) v++;
            while (vend > v && (vend[-1] == ' ' || vend[-1] == '\t')) vend--;
            char *tmp = ax_http_dup_range(v, (size_t)(vend - v));
            AxVal r = ax_str(tmp);
            free(tmp);
            return r;
        }
        p = eol ? eol + 2 : NULL;
    }
    return ax_str("");
}

static AxVal axn_http_respond(AxVal h, AxVal status, AxVal ctype, AxVal body) {
    AxHttpSlot *s = ax_http_get(h);
    if (!s || status.tag != AX_INT || ctype.tag != AX_STR || body.tag != AX_STR)
        return ax_bool(0);
    size_t blen = strlen(body.u.s);
    char head[512];
    int hn = snprintf(head, sizeof(head),
        "HTTP/1.1 %lld %s\r\nContent-Type: %s\r\nContent-Length: %zu\r\nConnection: close\r\n\r\n",
        (long long)status.u.i, ax_http_reason(status.u.i), ctype.u.s, blen);
    size_t off = 0;
    const char *parts[2] = { head, body.u.s };
    size_t lens[2] = { (size_t)hn, blen };
    int ok = 1;
    for (int pi = 0; pi < 2 && ok; pi++) {
        off = 0;
        while (off < lens[pi]) {
            ssize_t w = send(s->fd, parts[pi] + off, lens[pi] - off, MSG_NOSIGNAL);
            if (w <= 0) { ok = 0; break; }
            off += (size_t)w;
        }
    }
    close(s->fd);
    free(s->method); free(s->path); free(s->query);
    free(s->headers); free(s->body);
    s->used = 0;
    s->method = s->path = s->query = s->headers = s->body = NULL;
    return ax_bool(ok);
}
/* end of Axiom runtime */
