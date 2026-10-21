# Axiom 与 Rust 的语法区别（契约层为重点）

一句话：两者表面语法有亲戚感（都是 `fn`、花括号、强类型），但设计目标正相反。Rust 的语法为**内存与性能**服务，核心是所有权。Axiom 的语法为**可验证与问责**服务，核心是契约。下面先讲契约层（requires 这一级），再过一遍通用语法。

## 一、契约层：最根本的区别

### 1. 前置条件 requires

Axiom 里 `requires` 是语言的一等语法，写在函数体开头。编译器把它从函数体里提出来，在入口处检查，失败时打印条款原文和源码位置并以固定退出码终止。它同时进入证据 ledger。

```axiom
fn record_payment(s: Store, invoice_id: int, amount: int,
                  method: string, day: int, success: bool) -> bool {
    requires amount > 0;
    // ...
}
```

Rust 语言本身没有 requires。最接近的写法是手写断言，它和普通代码没有区别，不进任何账本，优化构建里还可能被关掉：

```rust
fn record_payment(s: &mut Store, invoice_id: u64, amount: i64,
                  method: &str, day: i64, success: bool) -> bool {
    assert!(amount > 0, "amount must be positive");
    // debug 构建才查的版本是 debug_assert!(...)
}
```

第三方 crate（如 `contracts`）提供 `#[requires(...)]` 属性宏，本质是把表达式展开成断言。它是库，不是语言：编译器不认识它，工具链不围绕它组织。

### 2. 后置条件 ensures 与 result

Axiom 的 `ensures` 在**每条返回路径**上都检查，可以引用参数和返回值 `result`：

```axiom
fn clamp(x: int, lo: int, hi: int) -> int {
    requires lo <= hi;
    ensures result >= lo && result <= hi;
    if x < lo { return lo; }
    if x > hi { return hi; }
    return x;
}
```

Rust 没有对应语法。要模拟，只能在每个 `return` 前手动重复断言（容易漏），或把函数包一层包装函数在出口统一断言（样板代码，且内层函数仍可被别处直接调用）。`contracts` crate 的 `#[ensures(...)]` 能做到出口检查，同样是宏展开。

### 3. 结构体 invariant

Axiom 把不变量写进结构体定义，编译器在**字面量构造后和每一次字段赋值后**自动检查。字段在子句里用裸名引用：

```axiom
struct Invoice {
    subtotal: int;
    tax: int;
    total: int;
    invariant total == subtotal + tax;
    invariant amount_paid >= credit_applied && amount_paid <= total;
}
```

Rust 没有 invariant 语法。惯用法是把字段设为私有、只允许通过方法修改、再在方法里自己写 `debug_assert!`。这是一种纪律约定：任何一个方法漏写，任何一条修改路径绕开，机器都不会提醒你——检查点由人维护，不是由编译器按构造规则强制。

### 4. 证据分级：Rust 侧没有对应概念

Axiom 每次构建产出 `<程序>.ledger.json`，逐条记录每个契约的原文、位置和证据等级：

| 等级 | 含义 |
|---|---|
| `runtime-checked` | 运行时检查，跑到才算数 |
| `bound` | extern 原生绑定的契约，在调用边界检查 |
| `bounded` | `axiomc verify` 在声明的定义域内穷举无反例（不是全称证明） |

Rust 侧的验证工具是外挂的：Kani（模型检验）、Prusti、Creusot（演绎证明）各管一摊，报告格式互不相通，没有一个随每次构建自动产出、按等级记账的统一台账。Axiom 的立场是：**等级必须诚实**，bounded 不许写成 proven。

### 5. 外部能力：capability 绑定 vs unsafe FFI

Axiom 的外部能力先在语言里声明成带契约的边界，缺原生实现时编译器直接拒绝构建：

```axiom
capability file {
    extern fn read_file(path: string) -> string {
        requires len(path) > 0;
    }
}
```

Rust 的 FFI 是 `unsafe extern "C" { fn read_file(...) }`：没有契约，调用方进入 unsafe 块后安全责任全在人。Axiom 的 extern 调用是普通安全调用，契约在边界上照常检查，绑定本身进 ledger（等级 bound）。

## 二、通用语法速览

| 方面 | Axiom | Rust |
|---|---|---|
| 变量 | `let x = 0;` 默认可变，直接 `x = x + 1;` | `let x = 0;` 默认不可变，可变要写 `let mut x` |
| 函数 | `fn f(a: int) -> int` | `fn f(a: i64) -> i64`（形状最像的一处） |
| 整数类型 | 只有一个 `int`（64 位）和 `float` | i8…i128、u8…u128、f32/f64，宽度要逐个选 |
| 结构体字面量 | `Point { x: 1, y: 2 }` | `Point { x: 1, y: 2 }`（几乎一样） |
| 方法 | `impl Point { fn norm(self) -> int }`，调用 `p.norm()` | `impl Point { fn norm(&self) -> i64 }`，多一层借用要写 |
| 泛型 | `fn first<T>(xs: [T]) -> T`、`Pair<A, B>`，单态化，无 trait bounds | `fn first<T>(xs: &[T]) -> &T`，带 trait bounds、生命周期，表达力大得多，复杂度也大得多 |
| 字符串 | 字节字符串，一等拼接 `a + b`，插值 `"{name} 共 {n} 件"` | `String`/`&str` 分家，拼接要 `.to_string()`/`format!`，插值是 `format!("{name} 共 {n} 件")` 宏 |
| 数组/map | `[1, 2, 3]`、`{"a": 1}` 字面量直接用，引用语义 | `vec![1, 2, 3]`、`HashMap` 要构造，无 map 字面量 |
| 错误处理 | `check` 断言、`raise` 终止，退出码即契约 | `Result<T, E>` 逐层传播 + `?` 运算符，`panic!` 是另一条路 |
| 内存 | arena 或可选 GC（`--gc`），没有所有权概念 | 所有权 + 借用检查是语言的中心，编译期解决内存安全 |
| 模块 | `use "path.ax";` 按文件路径 | `mod` + `use` + crate 体系 |
| 宏 | 没有 | 声明宏 + 过程宏，是语言的一部分 |
| 并发/async | 没有（有意的） | async/await、线程、Send/Sync  trait 体系 |

## 三、为什么长这样

Rust 的复杂度大部分来自一个承诺：不带 GC、不带运行时，还要内存安全。这个承诺必须由所有权系统在编译期兑现，语法就得把借用、生命周期、宽度全暴露给程序员。

Axiom 的承诺是另一个：每条声明都能被机器检查、被记账，程序要小到能被人和 AI 一起审完。它不需要所有权，因为它接受 GC；它不需要宽度类型，因为它不做贴硬件的活；它把省下来的语法预算全花在了契约上。

所以区别不是谁更先进，是**预算花在哪**：Rust 花在内存与性能，Axiom 花在验证与问责。
