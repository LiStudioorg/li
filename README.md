# li 语言编译器

li 是一门面向专业开发者的通用编程语言，使用中文关键字与中文符号，同时兼容英文写法。
编译器采用**前端统一、IR 中立、多后端**的架构，当前已实现：

- 词法分析、语法分析与语义分析；
- 严格编译期检查：不可变变量重赋值、可选值未判空、结果值未匹配、禁止隐式类型转换；
- 保留类型信息的中立 IR，使用三地址码与控制流图（CFG）；
- JavaScript 后端，可把 `.li` 编译为 Node.js 可直接运行的 `.js`；
- 字节码后端与栈式虚拟机，可把 `.li` 编译为便于阅读的 `.lic` 文本并执行。

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
    ├── li-codegen-bytecode # 字节码后端与 .lic 文本格式
    ├── li-vm               # 栈式字节码虚拟机
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

把 li 源码编译为字节码并交给虚拟机执行：

```bash
cargo run -p li-cli -- 编译 crates/li-parser/tests/fixtures/fibonacci.li
cargo run -p li-cli -- 执行 crates/li-parser/tests/fixtures/fibonacci.lic
# 输出：55
```

也可以把 li 源码编译为同名 JavaScript 文件：

```bash
cargo run -p li-cli -- 编译-js 示例.li
node 示例.js
```

`打印(值)` 在 JavaScript 后端映射为 `console.log(值)`，在字节码后端映射为
`PRINT` 指令并由虚拟机输出到控制台。

## 字节码与虚拟机

`li-codegen-bytecode` 把中立 IR 的 CFG 展开为线性指令序列，并把基本块标签
回填为函数内的指令地址。指令集包括：

```text
PUSH_CONST  LOAD  STORE  POP  ADD  SUB  MUL  DIV  NEG  NOT
LT  GT  EQ  AND  OR  JUMP  JUMP_IF_FALSE  CALL  RETURN  RETURN_VOID
HALT  PRINT  INPUT  MAKE_LIST  GET_FIELD  SET_FIELD  GET_INDEX  SET_INDEX
WRAP_OPTIONAL  WRAP_RESULT  IS_SUCCESS  UNWRAP  UNWRAP_ERROR  NOP
```

每个函数拥有常量池引用、局部槽位表与独立的操作数栈；`CALL` 创建新的调用帧，
`RETURN` 把返回值送回调用者。`.lic` 使用文本格式，跳转目标直接写成指令下标，
每条指令末尾的 `@行:列` 指向对应的 li 源码位置，便于调试运行时错误。

斐波那契函数生成的字节码片段：

```text
.constants 4
0 整数 2
1 整数 1
2 整数 10
3 整数 0
.end

.function 斐波那契 1
.slots 项数 t0 t1 t2 t3 t4 t5
.code
0000 LOAD 0 @3:8
0001 PUSH_CONST 0 @3:8
0002 LT @3:8
0003 STORE 1 @3:8
0004 LOAD 1 @3:5
0005 JUMP_IF_FALSE 9 @3:5
0006 JUMP 7 @3:5
0007 LOAD 0 @4:9
0008 RETURN @4:9
...
.end
```

虚拟机运行时会重新校验常量下标、槽位下标、跳转目标与函数调用，并用
`第 N 行第 M 列：运行时错误：...；函数「...」，指令 ...` 的格式报告错误。

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
6. **里程碑 6（已完成）**：新增 `li-codegen-bytecode` 与 `li-vm`，实现常量池、
   栈式指令集、CFG 到指令地址的回填、`.lic` 文本序列化/解析、调用栈与运行时错误定位；
   CLI 支持 `li 编译 <文件.li>` 与 `li 执行 <文件.lic>`，斐波那契样例输出 `55`。
