/* Axiom runtime (v0.1). Emitted into every generated C file by axiomc.
   Uniform tagged values; arena allocation (never freed in v0.1). */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>
#include <stdarg.h>

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
    if (v.tag == AX_MAP) return v.u.map->len;
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
    AxVal r; AxMapData *m = ax_alloc(sizeof(AxMapData));
    m->len = 0; m->cap = 8;
    m->keys = ax_alloc(sizeof(AxVal) * 8);
    m->vals = ax_alloc(sizeof(AxVal) * 8);
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
        AxVal *nk = ax_alloc(sizeof(AxVal) * (size_t)m->cap);
        AxVal *nv = ax_alloc(sizeof(AxVal) * (size_t)m->cap);
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
/* end of Axiom runtime */
