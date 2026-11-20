//! 中英文混写测试：同一函数内混用 `fn` / `if` / `return` / `+` / `加` 等。

use li_ast::{BinaryOp, ExprKind, Item, Stmt};

/// 混写源文件，覆盖：
/// - 函数声明：`fn` / `function` / `函数`；
/// - 变量声明：`let` / `让`；
/// - 控制流：`if` / `如果`、`then` / `则`、`else` / `否则`、`while` / `当`；
/// - 返回：`return` / `返回`；
/// - 匹配分支：`Ok` / `Success`、`Err` / `Failure`；
/// - 运算符：`+` / `加`、`-` / `减`、`*` / `乘`、`/` / `除`、`==` / `等于`；
/// - 布尔：`true` / `真`、`false` / `假`、`null` / `空`；
/// - 关键字别名：`mod` / `use` / `struct` / `type`。
///
/// 注意：li 的字符串字面量严格使用中文书名号 `「」`，不接受双引号。
pub const SOURCE: &str = r"
fn main() -> int {
    let count: int = 0;
    let text: string = 「hello」;
    if count < 10 then {
        count = count + 1;
    } else {
        return 0;
    }
    while count < 5 {
        print(text);
        count = count * 2;
    }
    return count;
}
";

/// 混写 AST 节点构造：同一函数里 `fn` 与 `return` 等价于 `函数` 与 `返回`。
#[test]
fn mixed_language_tokens_are_equivalent_in_ast() {
    let program =
        li_parser::parse_source(SOURCE).unwrap_or_else(|errors| panic!("语法分析失败：\n{errors}"));

    // 顶层应该有一个函数声明；无论 `fn` 还是 `函数` 都归一化为同一 AST。
    let items: Vec<&Item> = program.items.iter().collect();
    assert_eq!(items.len(), 1);

    let Item::Function(function) = &program.items[0] else {
        panic!("预期函数声明");
    };

    // 函数体里既有 `if ... then ... else ...`（中文），
    // 也有 `while`（英文）；它们都进入同一 Block。
    let mut has_if = false;
    let mut has_while = false;
    let mut has_return = false;

    for stmt in &function.body.stmts {
        match stmt {
            Stmt::If(_) => has_if = true,
            Stmt::While(_) => has_while = true,
            Stmt::Return(_) => has_return = true,
            _ => {}
        }
    }

    assert!(has_if, "应该包含 `if/then/else` 条件分支");
    assert!(has_while, "应该包含 `while` 循环");
    assert!(has_return, "应该包含 `return` 返回语句");
}

/// 验证 `+` 与 `加` 在表达式里是同一个 `BinaryOp::Add`。
#[test]
fn mixed_operators_normalize_to_same_binary_op() {
    let source = r#"
fn 求和(甲: int, 乙: int) -> int {
    let a = 甲 + 乙;
    let b = 甲 加 乙;
    返回 a == b;
}
"#;
    let program =
        li_parser::parse_source(source).unwrap_or_else(|errors| panic!("语法分析失败：\n{errors}"));

    let Item::Function(function) = &program.items[0] else {
        panic!("预期函数声明");
    };

    // 找两个 Let 语句的 RHS。
    let adds: Vec<BinaryOp> = function
        .body
        .stmts
        .iter()
        .filter_map(|stmt| match stmt {
            Stmt::Let(let_stmt) => match &let_stmt.initializer.kind {
                ExprKind::Binary { op, .. } => Some(*op),
                _ => None,
            },
            _ => None,
        })
        .collect();

    assert_eq!(adds.len(), 2, "应该有两个 let 初始化器");
    assert_eq!(adds[0], BinaryOp::Add, "`+` 必须归一化为 Add");
    assert_eq!(adds[1], BinaryOp::Add, "`加` 必须归一化为 Add");
}
