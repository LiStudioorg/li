# li 语言编译器

li 是一门面向专业开发者的通用编程语言，使用中文关键字与中文符号，同时兼容英文写法。
编译器采用**前端统一、IR 中立、多后端**的架构，当前已实现：

- 词法分析、语法分析与语义分析；
- 严格编译期检查：不可变变量重赋值、可选值未判空、结果值未匹配、禁止隐式类型转换；
- 保留类型信息的中立 IR，使用三地址码与控制流图（CFG）；
- JavaScript 后端，可把 `.li` 编译为 Node.js 可直接运行的 `.js`。

> 字节码 VM 后端将在后续里程碑接入。

## 目录结构

```text
.
├── Cargo.toml              # workspace 配置
├── AGENTS.md               # 项目核心开发规范
├── LICENSE-MIT
├── LICENSE-APACHE
└── crates
    ├── li-errors           # 统一中文诊断、行列定位与源文件辅助
    ├── li-lexer            # 词法与 Token 定义
    ├── li-ast              # AST 节点定义
    ├── li-parser           # 递归下降语法分析器
    ├── li-sema             # 语义分析与严格类型检查
    ├── li-ir               # 中立 IR、三地址码与 AST 降级
    ├── li-codegen-js       # JavaScript 后端
    └── li-cli              # 命令行入口
```

## 构建与测试

```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

所有 crate 均采用 `MIT OR Apache-2.0` 双协议。

## 使用

把 li 源码编译为同名 JavaScript 文件：

```bash
cargo run -p li-cli -- 编译-js 示例.li
node 示例.js
```

例如斐波那契样例：

```bash
cargo run -p li-cli -- 编译-js crates/li-parser/tests/fixtures/fibonacci.li
node crates/li-parser/tests/fixtures/fibonacci.js
# 输出：55
```

`打印(值)` 会映射为 JavaScript 的 `console.log(值)`。

## 中立 IR

`li-ir` 把 AST 降级为与后端无关的中间表示：

- 数据流使用三地址码，每条指令显式保留结果类型；
- 控制流使用基本块与 CFG；
- `如果`、`当`、`跳出`、`继续`、`返回` 会转换为
  `LABEL`、`JUMP`、`BRANCH`、`RETURN` 等显式控制流指令；
- `可选<T>` 与 `结果<T, E>` 使用独立的包装/解包指令，保证后端能正确处理匹配；
- JavaScript 后端只消费 IR，不反向依赖 AST。

## 统一诊断格式

`li-errors` 提供带行列号的中文诊断，例如：

```text
第 3 行第 5 列：类型不匹配，期望 整数，实际 字符串
```

错误信息同时支持期望值、实际值、稳定错误码和附注，词法、语法、语义与降级阶段共用
`Diagnostic` 与 `DiagnosticBag`。所有用户可见错误均为简体中文。

## 里程碑

1. **里程碑 1（已完成）**：初始化 workspace、六个 crate、双协议与 `li-errors` 基础定义。
2. **里程碑 2（已完成）**：定义 Token 与 AST 骨架，实现词法分析器，覆盖中文/英文关键字、全角/半角符号、
   `「」` 字符串、`#` 与 `#* ... *#` 可嵌套注释及行列定位。
3. **里程碑 3（已完成）**：实现递归下降解析器，表达式使用 Pratt 优先级攀升，
   语句级错误使用恐慌模式恢复到 `;` 或 `}`；覆盖变量、函数、结构体、模块、控制流、
   字段访问、下标、函数调用与 `成功` / `失败` 匹配语法，并通过斐波那契样例。
4. **里程碑 4（已完成）**：实现嵌套作用域符号表、表达式类型推导、函数签名与结构体字段检查，
   并强制检查不可变变量重赋值、可选类型判空、结果类型匹配与禁止隐式转换。
5. **里程碑 5（已完成）**：新增 `li-ir` 与 `li-codegen-js`，把 AST 降级为保留类型的
   CFG/三地址码 IR，CLI 支持 `li 编译-js <文件.li>`；斐波那契样例生成的 JavaScript
   可由 Node.js 直接运行并输出 `55`。
6. **里程碑 6**：实现字节码生成与虚拟机后端，复用同一套中立 IR。
