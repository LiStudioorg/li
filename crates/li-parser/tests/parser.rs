//! `li-parser` 的集成测试。
//!
//! 覆盖技术方案中的斐波那契样例、Pratt 优先级与结合性、
//! 变量/函数/结构体/模块/控制流/匹配语法，以及恐慌模式的多错误恢复。

use li_ast::{
    BinaryOp, Expr, ExprKind, FuncDecl, Item, Literal, PatternKind, Program, Stmt, TypeKind,
    UnaryOp, ViewContent, ViewTextPartKind, ViewValueKind,
};
use li_errors::DiagnosticBag;
use li_parser::{parse, parse_source};

/// 解析源码并断言成功。
fn parse_ok(source: &str) -> Program {
    parse_source(source).unwrap_or_else(|errors| panic!("解析失败：\n{errors}"))
}

/// 解析源码并断言失败，返回全部诊断。
fn parse_err(source: &str) -> DiagnosticBag {
    parse_source(source).expect_err("预期解析失败，但解析器接受了源码")
}

/// 按名称查找顶层函数。
fn function<'a>(program: &'a Program, name: &str) -> &'a FuncDecl {
    program
        .items
        .iter()
        .find_map(|item| match item {
            Item::Function(function) if function.name.as_str() == name => Some(function),
            _ => None,
        })
        .unwrap_or_else(|| panic!("未找到函数 `{name}`"))
}

/// 断言表达式是二元运算，返回左右操作数。
fn assert_binary(expr: &Expr, expected: BinaryOp) -> (&Expr, &Expr) {
    match &expr.kind {
        ExprKind::Binary { left, op, right } => {
            assert_eq!(*op, expected, "二元运算符不匹配");
            (left, right)
        }
        other => panic!("预期二元表达式，实际 {other:?}"),
    }
}

/// 断言表达式是整数字面量。
fn assert_integer(expr: &Expr, expected: i64) {
    match &expr.kind {
        ExprKind::Literal(Literal::Integer(value)) => assert_eq!(*value, expected),
        other => panic!("预期整数字面量 `{expected}`，实际 {other:?}"),
    }
}

/// 断言表达式是标识符。
fn assert_identifier(expr: &Expr, expected: &str) {
    match &expr.kind {
        ExprKind::Identifier(ident) => assert_eq!(ident.name, expected),
        other => panic!("预期标识符 `{expected}`，实际 {other:?}"),
    }
}

/// 解析只有一个返回语句的函数，并返回返回表达式。
fn single_return_expr(expression: &str) -> Expr {
    let source = format!("函数 主() -> 整数 {{ 返回 {expression}; }}");
    let program = parse_ok(&source);
    let function = function(&program, "主");

    match &function.body.stmts[0] {
        Stmt::Return(return_stmt) => return_stmt.value.clone().expect("返回语句应当带有返回值"),
        other => panic!("预期返回语句，实际 {other:?}"),
    }
}

#[test]
fn parses_the_fibonacci_fixture() {
    let program = parse_ok(include_str!("fixtures/fibonacci.li"));
    assert_eq!(program.items.len(), 2);

    let fibonacci = function(&program, "斐波那契");
    assert_eq!(fibonacci.name.span.start.line, 2);
    assert_eq!(fibonacci.params.len(), 1);
    assert_eq!(fibonacci.params[0].name.as_str(), "项数");
    assert_eq!(fibonacci.params[0].ty.kind, TypeKind::Integer);
    assert_eq!(fibonacci.return_type.kind, TypeKind::Integer);
    assert_eq!(fibonacci.body.stmts.len(), 2);

    // 如果 项数 小于 2 则 { 返回 项数; }
    let Stmt::If(if_stmt) = &fibonacci.body.stmts[0] else {
        panic!("首个语句应为条件语句，实际 {:?}", fibonacci.body.stmts[0]);
    };
    let (condition_left, condition_right) = assert_binary(&if_stmt.condition, BinaryOp::Less);
    assert_identifier(condition_left, "项数");
    assert_integer(condition_right, 2);
    assert!(if_stmt.else_branch.is_none());
    assert_eq!(if_stmt.then_branch.stmts.len(), 1);

    let Stmt::Return(base_return) = &if_stmt.then_branch.stmts[0] else {
        panic!("条件分支应包含返回语句");
    };
    let base_value = base_return.value.as_ref().expect("返回语句应带值");
    assert_identifier(base_value, "项数");

    // 返回 斐波那契(项数 减 1) 加 斐波那契(项数 减 2);
    let Stmt::Return(recursive_return) = &fibonacci.body.stmts[1] else {
        panic!("第二个语句应为返回语句");
    };
    let recursive_value = recursive_return.value.as_ref().expect("返回语句应带值");
    let (left_call, right_call) = assert_binary(recursive_value, BinaryOp::Add);
    assert_fibonacci_call(left_call, 1);
    assert_fibonacci_call(right_call, 2);

    // 主函数：让 值 = 斐波那契(10); 打印(值); 返回 0;
    let main = function(&program, "主");
    assert_eq!(main.name.span.start.line, 9);
    assert_eq!(main.body.stmts.len(), 3);

    let Stmt::Let(let_stmt) = &main.body.stmts[0] else {
        panic!("主函数首个语句应为变量声明");
    };
    assert_eq!(let_stmt.name.as_str(), "值");
    assert!(!let_stmt.mutable);
    let ExprKind::Call { callee, args } = &let_stmt.initializer.kind else {
        panic!("变量初始化应为函数调用，实际 {:?}", let_stmt.initializer);
    };
    assert_identifier(callee, "斐波那契");
    assert_eq!(args.len(), 1);
    assert_integer(&args[0], 10);

    let Stmt::Print(print_stmt) = &main.body.stmts[1] else {
        panic!("主函数第二个语句应为打印语句");
    };
    assert_identifier(&print_stmt.value, "值");

    let Stmt::Return(main_return) = &main.body.stmts[2] else {
        panic!("主函数第三个语句应为返回语句");
    };
    assert_integer(main_return.value.as_ref().expect("返回语句应带值"), 0);
}

/// 断言表达式是 `斐波那契(项数 减 offset)` 形式的递归调用。
fn assert_fibonacci_call(expr: &Expr, offset: i64) {
    let ExprKind::Call { callee, args } = &expr.kind else {
        panic!("预期函数调用，实际 {:?}", expr.kind);
    };
    assert_identifier(callee, "斐波那契");
    assert_eq!(args.len(), 1);

    let (left, right) = assert_binary(&args[0], BinaryOp::Subtract);
    assert_identifier(left, "项数");
    assert_integer(right, offset);
}

#[test]
fn honors_c_style_precedence_and_associativity() {
    // 乘法优先于加法。
    let expression = single_return_expr("1 加 2 乘 3");
    let (left, right) = assert_binary(&expression, BinaryOp::Add);
    assert_integer(left, 1);
    let (nested_left, nested_right) = assert_binary(right, BinaryOp::Multiply);
    assert_integer(nested_left, 2);
    assert_integer(nested_right, 3);

    // 同级运算符左结合。
    let expression = single_return_expr("1 减 2 加 3");
    let (left, right) = assert_binary(&expression, BinaryOp::Add);
    let (nested_left, nested_right) = assert_binary(left, BinaryOp::Subtract);
    assert_integer(nested_left, 1);
    assert_integer(nested_right, 2);
    assert_integer(right, 3);

    let expression = single_return_expr("8 除 4 乘 2");
    let (left, right) = assert_binary(&expression, BinaryOp::Multiply);
    let (nested_left, nested_right) = assert_binary(left, BinaryOp::Divide);
    assert_integer(nested_left, 8);
    assert_integer(nested_right, 4);
    assert_integer(right, 2);

    // 比较优先于且，且优先于或。
    let expression = single_return_expr("1 小于 2 且 2 小于 3 或 假");
    let (left, right) = assert_binary(&expression, BinaryOp::Or);
    assert!(matches!(
        right.kind,
        ExprKind::Literal(Literal::Bool(false))
    ));

    let (and_left, and_right) = assert_binary(left, BinaryOp::And);
    let (compare_left, compare_right) = assert_binary(and_left, BinaryOp::Less);
    assert_integer(compare_left, 1);
    assert_integer(compare_right, 2);

    let (compare_left, compare_right) = assert_binary(and_right, BinaryOp::Less);
    assert_integer(compare_left, 2);
    assert_integer(compare_right, 3);

    // 或的右侧是完整的且表达式。
    let expression = single_return_expr("假 或 真 且 假");
    let (left, right) = assert_binary(&expression, BinaryOp::Or);
    assert!(matches!(left.kind, ExprKind::Literal(Literal::Bool(false))));
    let (and_left, and_right) = assert_binary(right, BinaryOp::And);
    assert!(matches!(
        and_left.kind,
        ExprKind::Literal(Literal::Bool(true))
    ));
    assert!(matches!(
        and_right.kind,
        ExprKind::Literal(Literal::Bool(false))
    ));
}

#[test]
fn parses_parentheses_and_unary_operators() {
    // 括号覆盖默认优先级，全角括号由词法层归一化。
    let expression = single_return_expr("（1 加 2）乘 3");
    let (left, right) = assert_binary(&expression, BinaryOp::Multiply);
    assert_integer(right, 3);
    let ExprKind::Parenthesized(inner) = &left.kind else {
        panic!("预期括号表达式，实际 {:?}", left.kind);
    };
    let (nested_left, nested_right) = assert_binary(inner, BinaryOp::Add);
    assert_integer(nested_left, 1);
    assert_integer(nested_right, 2);

    // 一元负号绑定力高于乘法。
    let expression = single_return_expr("-1 乘 2");
    let (left, right) = assert_binary(&expression, BinaryOp::Multiply);
    assert_integer(right, 2);
    let ExprKind::Unary { op, operand } = &left.kind else {
        panic!("预期一元表达式，实际 {:?}", left.kind);
    };
    assert_eq!(*op, UnaryOp::Negate);
    assert_integer(operand, 1);

    // 逻辑非绑定力高于且。
    let expression = single_return_expr("非 真 且 假");
    let (left, right) = assert_binary(&expression, BinaryOp::And);
    assert!(matches!(
        right.kind,
        ExprKind::Literal(Literal::Bool(false))
    ));
    let ExprKind::Unary { op, operand } = &left.kind else {
        panic!("预期一元表达式，实际 {:?}", left.kind);
    };
    assert_eq!(*op, UnaryOp::Not);
    assert!(matches!(
        operand.kind,
        ExprKind::Literal(Literal::Bool(true))
    ));
}

#[test]
fn parses_assignment_with_lowest_precedence() {
    let source = "函数 主() -> 整数 { 累计 = 1 加 2 乘 3; 返回 0; }";
    let program = parse_ok(source);
    let main = function(&program, "主");

    let Stmt::Assign(assign_stmt) = &main.body.stmts[0] else {
        panic!("首个语句应为赋值语句");
    };
    assert_identifier(&assign_stmt.target, "累计");

    let (left, right) = assert_binary(&assign_stmt.value, BinaryOp::Add);
    assert_integer(left, 1);
    let (nested_left, nested_right) = assert_binary(right, BinaryOp::Multiply);
    assert_integer(nested_left, 2);
    assert_integer(nested_right, 3);
}

#[test]
fn parses_variable_declarations_with_optional_types() {
    let source = "\
函数 主() -> 整数 {
    让 甲 = 1;
    让 可变 乙: 小数 = 2.5;
    让 丙: 列表<整数> = [1, 2, 3,];
    返回 0;
}";
    let program = parse_ok(source);
    let main = function(&program, "主");
    assert_eq!(main.body.stmts.len(), 4);

    let Stmt::Let(first) = &main.body.stmts[0] else {
        panic!("首个语句应为变量声明");
    };
    assert_eq!(first.name.as_str(), "甲");
    assert!(!first.mutable);
    assert!(first.ty.is_none());
    assert_integer(&first.initializer, 1);

    let Stmt::Let(second) = &main.body.stmts[1] else {
        panic!("第二个语句应为变量声明");
    };
    assert_eq!(second.name.as_str(), "乙");
    assert!(second.mutable);
    assert_eq!(
        second.ty.as_ref().map(|ty| &ty.kind),
        Some(&TypeKind::Float)
    );
    assert!(matches!(
        second.initializer.kind,
        ExprKind::Literal(Literal::Float(_))
    ));

    let Stmt::Let(third) = &main.body.stmts[2] else {
        panic!("第三个语句应为变量声明");
    };
    assert_eq!(third.name.as_str(), "丙");
    let Some(declared_type) = &third.ty else {
        panic!("丙应带有类型标注");
    };
    let TypeKind::List(element) = &declared_type.kind else {
        panic!("丙应标注为列表类型");
    };
    assert_eq!(element.kind, TypeKind::Integer);

    let ExprKind::ListLiteral(items) = &third.initializer.kind else {
        panic!("第三个变量应以列表字面量初始化");
    };
    assert_eq!(items.len(), 3);
    assert_integer(&items[0], 1);
    assert_integer(&items[1], 2);
    assert_integer(&items[2], 3);
}

#[test]
fn parses_function_parameters_and_calls() {
    let source = "\
函数 求和(甲: 整数, 乙: 整数,) -> 整数 {
    返回 甲 加 乙;
}
函数 主() -> 整数 {
    求和(1, 2,);
    返回 0;
}";
    let program = parse_ok(source);
    assert_eq!(program.items.len(), 2);

    let sum = function(&program, "求和");
    assert_eq!(sum.params.len(), 2);
    assert_eq!(sum.params[0].name.as_str(), "甲");
    assert_eq!(sum.params[0].ty.kind, TypeKind::Integer);
    assert_eq!(sum.params[1].name.as_str(), "乙");
    assert_eq!(sum.params[1].ty.kind, TypeKind::Integer);
    assert_eq!(sum.return_type.kind, TypeKind::Integer);

    let Stmt::Return(return_stmt) = &sum.body.stmts[0] else {
        panic!("函数体应包含返回语句");
    };
    let (left, right) = assert_binary(
        return_stmt.value.as_ref().expect("返回语句应带值"),
        BinaryOp::Add,
    );
    assert_identifier(left, "甲");
    assert_identifier(right, "乙");

    let main = function(&program, "主");
    let Stmt::Expr(expr_stmt) = &main.body.stmts[0] else {
        panic!("首个语句应为表达式语句");
    };
    let ExprKind::Call { callee, args } = &expr_stmt.expr.kind else {
        panic!("预期函数调用，实际 {:?}", expr_stmt.expr.kind);
    };
    assert_identifier(callee, "求和");
    assert_eq!(args.len(), 2);
    assert_integer(&args[0], 1);
    assert_integer(&args[1], 2);
}

#[test]
fn parses_struct_declarations_and_nested_generic_types() {
    let source = "\
类型 用户 {
    名字: 字符串,
    年龄: 整数,
    标签: 列表<可选<字符串>>,
    结果值: 结果<整数, 字符串>,
}";
    let program = parse_ok(source);
    assert_eq!(program.items.len(), 1);

    let Item::Type(type_decl) = &program.items[0] else {
        panic!("期望类型声明");
    };
    assert_eq!(type_decl.name.as_str(), "用户");
    assert_eq!(type_decl.fields.len(), 4);

    assert_eq!(type_decl.fields[0].name.as_str(), "名字");
    assert_eq!(type_decl.fields[0].ty.kind, TypeKind::String);
    assert_eq!(type_decl.fields[1].name.as_str(), "年龄");
    assert_eq!(type_decl.fields[1].ty.kind, TypeKind::Integer);

    assert_eq!(type_decl.fields[2].name.as_str(), "标签");
    let TypeKind::List(element) = &type_decl.fields[2].ty.kind else {
        panic!("标签字段应为列表类型");
    };
    let TypeKind::Optional(inner) = &element.kind else {
        panic!("列表元素应为可选类型");
    };
    assert_eq!(inner.kind, TypeKind::String);

    assert_eq!(type_decl.fields[3].name.as_str(), "结果值");
    let TypeKind::Result { value, error } = &type_decl.fields[3].ty.kind else {
        panic!("结果值字段应为结果类型");
    };
    assert_eq!(value.kind, TypeKind::Integer);
    assert_eq!(error.kind, TypeKind::String);
}

#[test]
fn parses_modules_imports_and_postfix_expressions() {
    let source = "\
模块 工具 {
    导入 标准::输入输出;
    类型 用户 { 名字: 字符串, }
    函数 取名字(用户值: 用户) -> 字符串 { 返回 用户值.名字; }
}
函数 主() -> 整数 {
    让 集合 = [1, 2, 3];
    打印(集合[0]);
    返回 0;
}";
    let program = parse_ok(source);
    assert_eq!(program.items.len(), 2);

    let Item::Module(module) = &program.items[0] else {
        panic!("期望模块声明");
    };
    assert_eq!(module.name.as_str(), "工具");
    assert_eq!(module.items.len(), 3);

    let Item::Import(import) = &module.items[0] else {
        panic!("模块首项应为导入声明");
    };
    let segments: Vec<&str> = import
        .path
        .segments
        .iter()
        .map(|segment| segment.as_str())
        .collect();
    assert_eq!(segments, vec!["标准", "输入输出"]);

    let Item::Type(user_type) = &module.items[1] else {
        panic!("模块第二项应为类型声明");
    };
    assert_eq!(user_type.name.as_str(), "用户");
    assert_eq!(user_type.fields.len(), 1);

    let Item::Function(get_name) = &module.items[2] else {
        panic!("模块第三项应为函数声明");
    };
    assert_eq!(get_name.name.as_str(), "取名字");
    let Stmt::Return(return_stmt) = &get_name.body.stmts[0] else {
        panic!("取名字函数应返回字段访问结果");
    };
    let returned = return_stmt.value.as_ref().expect("返回语句应带值");
    let ExprKind::FieldAccess { object, field } = &returned.kind else {
        panic!("预期字段访问，实际 {:?}", returned.kind);
    };
    assert_identifier(object, "用户值");
    assert_eq!(field.as_str(), "名字");

    let main = function(&program, "主");
    let Stmt::Let(let_stmt) = &main.body.stmts[0] else {
        panic!("主函数首个语句应为变量声明");
    };
    assert!(matches!(
        let_stmt.initializer.kind,
        ExprKind::ListLiteral(_)
    ));

    let Stmt::Print(print_stmt) = &main.body.stmts[1] else {
        panic!("主函数第二个语句应为打印语句");
    };
    let ExprKind::Index { object, index } = &print_stmt.value.kind else {
        panic!("预期下标访问，实际 {:?}", print_stmt.value.kind);
    };
    assert_identifier(object, "集合");
    assert_integer(index, 0);
}

#[test]
fn parses_control_flow_statements() {
    let source = "\
函数 主() -> 整数 {
    如果 真 则 {
        打印(1);
    } 否则 {
        打印(0);
    }
    让 可变 计数 = 0;
    当 计数 小于 3 {
        计数 = 计数 加 1;
        如果 计数 等于 2 则 { 继续; }
        如果 计数 等于 3 则 { 跳出; }
    }
    返回 0;
}";
    let program = parse_ok(source);
    let main = function(&program, "主");
    assert_eq!(main.body.stmts.len(), 4);

    let Stmt::If(if_stmt) = &main.body.stmts[0] else {
        panic!("首个语句应为条件语句");
    };
    assert!(matches!(
        if_stmt.condition.kind,
        ExprKind::Literal(Literal::Bool(true))
    ));
    let else_branch = if_stmt.else_branch.as_ref().expect("应包含否则分支");
    assert_eq!(if_stmt.then_branch.stmts.len(), 1);
    assert_eq!(else_branch.stmts.len(), 1);

    let Stmt::Let(counter) = &main.body.stmts[1] else {
        panic!("第二个语句应为变量声明");
    };
    assert!(counter.mutable);
    assert_integer(&counter.initializer, 0);

    let Stmt::While(while_stmt) = &main.body.stmts[2] else {
        panic!("第三个语句应为循环语句");
    };
    let (condition_left, condition_right) = assert_binary(&while_stmt.condition, BinaryOp::Less);
    assert_identifier(condition_left, "计数");
    assert_integer(condition_right, 3);
    assert_eq!(while_stmt.body.stmts.len(), 3);

    let Stmt::Assign(assign_stmt) = &while_stmt.body.stmts[0] else {
        panic!("循环体首个语句应为赋值语句");
    };
    assert_identifier(&assign_stmt.target, "计数");
    let (left, right) = assert_binary(&assign_stmt.value, BinaryOp::Add);
    assert_identifier(left, "计数");
    assert_integer(right, 1);

    let Stmt::If(continue_if) = &while_stmt.body.stmts[1] else {
        panic!("循环体第二个语句应为条件语句");
    };
    assert!(matches!(
        continue_if.then_branch.stmts[0],
        Stmt::Continue(_)
    ));

    let Stmt::If(break_if) = &while_stmt.body.stmts[2] else {
        panic!("循环体第三个语句应为条件语句");
    };
    assert!(matches!(break_if.then_branch.stmts[0], Stmt::Break(_)));
}

#[test]
fn parses_success_and_failure_match_patterns() {
    let source = "\
函数 主() -> 整数 {
    让 结果值 = 获取();
    匹配 结果值 {
        成功(值) => { 打印(值); },
        失败(错误) => { 打印(错误); },
    }
    返回 0;
}";
    let program = parse_ok(source);
    let main = function(&program, "主");

    let Stmt::Match(match_stmt) = &main.body.stmts[1] else {
        panic!("第二个语句应为匹配语句");
    };
    assert_identifier(&match_stmt.value, "结果值");
    assert_eq!(match_stmt.arms.len(), 2);

    let PatternKind::Success(binding) = &match_stmt.arms[0].pattern.kind else {
        panic!("首个分支应为成功模式");
    };
    assert_eq!(binding.as_ref().expect("成功模式应绑定名称").as_str(), "值");
    let Stmt::Print(print_stmt) = &match_stmt.arms[0].body.stmts[0] else {
        panic!("成功分支应包含打印语句");
    };
    assert_identifier(&print_stmt.value, "值");

    let PatternKind::Failure(binding) = &match_stmt.arms[1].pattern.kind else {
        panic!("第二个分支应为失败模式");
    };
    assert_eq!(
        binding.as_ref().expect("失败模式应绑定名称").as_str(),
        "错误"
    );
    let Stmt::Print(print_stmt) = &match_stmt.arms[1].body.stmts[0] else {
        panic!("失败分支应包含打印语句");
    };
    assert_identifier(&print_stmt.value, "错误");
}

#[test]
fn parses_match_patterns_without_bindings() {
    let source = "\
函数 主() -> 整数 {
    匹配 获取() {
        成功 => { 打印(1); },
        失败 => { 打印(0); },
    }
    返回 0;
}";
    let program = parse_ok(source);
    let main = function(&program, "主");

    let Stmt::Match(match_stmt) = &main.body.stmts[0] else {
        panic!("首个语句应为匹配语句");
    };
    assert!(matches!(
        match_stmt.arms[0].pattern.kind,
        PatternKind::Success(None)
    ));
    assert!(matches!(
        match_stmt.arms[1].pattern.kind,
        PatternKind::Failure(None)
    ));
}

#[test]
fn parses_return_without_value() {
    let source = "函数 提前退出() -> 整数 { 返回; }";
    let program = parse_ok(source);
    let early_exit = function(&program, "提前退出");

    let Stmt::Return(return_stmt) = &early_exit.body.stmts[0] else {
        panic!("函数体应包含返回语句");
    };
    assert!(return_stmt.value.is_none());
}

#[test]
fn parses_empty_source() {
    let program = parse_ok("");
    assert!(program.items.is_empty());
    assert_eq!(program.span.start.line, 1);
    assert_eq!(program.span.start.column, 1);
}

#[test]
fn parses_fullwidth_syntax_normalized_by_the_lexer() {
    let program = parse_ok("函数 主（）－＞整数｛返回（1＋2＊3）；｝");
    let main = function(&program, "主");

    let Stmt::Return(return_stmt) = &main.body.stmts[0] else {
        panic!("函数体应包含返回语句");
    };
    let value = return_stmt.value.as_ref().expect("返回语句应带值");
    let ExprKind::Parenthesized(value) = &value.kind else {
        panic!("预期括号表达式，实际 {:?}", value.kind);
    };
    let (left, right) = assert_binary(value, BinaryOp::Add);
    assert_integer(left, 1);
    let (nested_left, nested_right) = assert_binary(right, BinaryOp::Multiply);
    assert_integer(nested_left, 2);
    assert_integer(nested_right, 3);
}

#[test]
fn parse_accepts_token_streams_directly() {
    let tokens = li_lexer::lex("函数 恒等(甲: 整数) -> 整数 { 返回 甲; }").expect("词法分析应成功");
    let program = parse(tokens).expect("语法分析应成功");

    let identity = function(&program, "恒等");
    assert_eq!(identity.params.len(), 1);
    assert_eq!(identity.params[0].name.as_str(), "甲");
}

#[test]
fn recovers_from_multiple_syntax_errors() {
    // 第 2 行缺少变量名，第 3 行缺少初始化表达式，
    // 第 6 行在顶层出现变量声明。三处错误应被一次性报告。
    let source = "\
函数 主() -> 整数 {
    让 = 1;
    让 乙 = ;
    返回 0;
}
让 丙 = 2;
";
    let errors = parse_err(source);

    assert_eq!(errors.error_count(), 3, "诊断：\n{errors}");
    let message = errors.to_string();

    for line in [2, 3, 6] {
        assert!(
            message.contains(&format!("第 {line} 行")),
            "缺少第 {line} 行的诊断：\n{message}"
        );
    }

    assert!(message.contains("期望 变量名"), "诊断：\n{message}");
    assert!(message.contains("实际 `=`"), "诊断：\n{message}");
    assert!(message.contains("期望 表达式"), "诊断：\n{message}");
    assert!(
        message.contains("期望 `模块`、`导入`、`函数`、`异步 函数`、`类型` 或 `视图`"),
        "诊断：\n{message}"
    );
}

/// 查找唯一的顶层视图声明。
fn view(program: &Program) -> &li_ast::ViewDecl {
    program
        .items
        .iter()
        .find_map(|item| match item {
            Item::View(view) => Some(view),
            _ => None,
        })
        .expect("未找到视图声明")
}

#[test]
fn parses_view_fixture() {
    let program = parse_ok(include_str!("fixtures/hello.lui"));
    assert_eq!(program.items.len(), 1);

    let view = view(&program);
    assert_eq!(view.attributes.len(), 2);
    assert_eq!(view.attributes[0].name.as_str(), "标题");
    assert_eq!(
        view.attributes[0].value.kind,
        ViewValueKind::Text("你好，li".to_owned())
    );
    assert_eq!(view.attributes[1].name.as_str(), "语言");

    let ViewContent::Children(children) = &view.content else {
        panic!("视图根节点应包含子节点，实际 {:?}", view.content);
    };
    assert_eq!(children.len(), 1);

    let container = &children[0];
    assert_eq!(container.name.as_str(), "容器");
    let ViewContent::Children(children) = &container.content else {
        panic!("容器应包含子节点，实际 {:?}", container.content);
    };
    let names = children
        .iter()
        .map(|child| child.name.as_str())
        .collect::<Vec<_>>();
    assert_eq!(names, ["标题", "段落", "列表", "按钮"]);

    let ViewContent::Children(title_children) = &children[0].content else {
        panic!("标题应包含子节点");
    };
    assert_eq!(title_children.len(), 1);
    assert_eq!(title_children[0].name.as_str(), "文本");
    let ViewContent::Text(title_text) = &title_children[0].content else {
        panic!("文本节点应包含字符串");
    };
    assert_eq!(title_text, "你好，li！");

    let list = &children[2];
    let ViewContent::Children(list_items) = &list.content else {
        panic!("列表应包含列表项");
    };
    assert_eq!(list_items.len(), 2);
    assert_eq!(list_items[0].name.as_str(), "列表项");

    let button = &children[3];
    assert_eq!(button.attributes.len(), 2);
    assert_eq!(button.attributes[1].name.as_str(), "类型");
    assert_eq!(
        button.attributes[1].value.kind,
        ViewValueKind::Text("button".to_owned())
    );
    assert_eq!(button.name.span.start.line, 12);
}

#[test]
fn parses_view_keywords_as_names() {
    // `列表` 是类型关键字，`类型` 是顶层关键字；在视图上下文中都应作为名称使用。
    let program =
        parse_ok("视图 { 列表 { 列表项 { 类: 条目, 文本 { 「第一项」 } } } 按钮 { 类型: 按钮 } }");
    let view = view(&program);

    let ViewContent::Children(children) = &view.content else {
        panic!("视图根节点应包含子节点");
    };
    assert_eq!(children[0].name.as_str(), "列表");

    let ViewContent::Children(items) = &children[0].content else {
        panic!("列表应包含子节点");
    };
    assert_eq!(
        items[0].attributes[0].value.kind,
        ViewValueKind::Identifier("条目".to_owned())
    );
    assert_eq!(children[1].attributes[0].name.as_str(), "类型");
}

#[test]
fn view_strings_require_chinese_quotes() {
    let errors = parse_err("视图 { 文本 { \"你好\" } }");
    let message = errors.to_string();

    assert!(
        message.contains("字符串必须使用中文书名号「」作为定界符"),
        "诊断：\n{message}"
    );
    assert!(message.contains("第 1 行第 11 列"), "诊断：\n{message}");
}

#[test]
fn recovers_from_a_missing_view_value() {
    let errors = parse_err("视图 { 容器 { 类: } }");
    let message = errors.to_string();

    assert!(message.contains("期望 属性值"), "诊断：\n{message}");
    assert!(message.contains("第 1 行"), "诊断：\n{message}");
}

#[test]
fn parses_unsafe_blocks_and_raw_pointer_types() {
    let program = parse_ok(include_str!("fixtures/unsafe_raw_pointer.li"));

    let declaration = program
        .items
        .iter()
        .find_map(|item| match item {
            Item::Type(declaration) if declaration.name.as_str() == "缓冲区" => {
                Some(declaration)
            }
            _ => None,
        })
        .expect("缺少缓冲区类型声明");
    assert!(
        matches!(declaration.fields[0].ty.kind, TypeKind::RawPointer(_)),
        "字段类型应为原生指针：{:?}",
        declaration.fields[0].ty
    );

    let null_pointer = function(&program, "空指针");
    assert!(
        matches!(null_pointer.return_type.kind, TypeKind::RawPointer(_)),
        "返回类型应为原生指针：{:?}",
        null_pointer.return_type
    );
    let Stmt::Unsafe(unsafe_stmt) = &null_pointer.body.stmts[0] else {
        panic!("空指针函数体应以不安全块开始");
    };
    assert!(matches!(unsafe_stmt.body.stmts[0], Stmt::Return(_)));

    let main = function(&program, "主");
    let Stmt::Unsafe(unsafe_stmt) = &main.body.stmts[0] else {
        panic!("主函数体应以不安全块开始");
    };

    let Stmt::Let(pointer) = &unsafe_stmt.body.stmts[0] else {
        panic!("不安全块内应以变量声明开始");
    };
    assert_eq!(pointer.name.as_str(), "指针");
    let annotation = pointer.ty.as_ref().expect("指针变量应有类型标注");
    assert!(
        matches!(annotation.kind, TypeKind::RawPointer(_)),
        "标注应为原生指针：{:?}",
        annotation
    );
}

#[test]
fn parses_view_state_events_and_text_bindings() {
    let program = parse_ok(include_str!("fixtures/counter.lui"));
    let view = view(&program);

    assert_eq!(view.state.len(), 2, "状态块应包含两条声明");
    let Stmt::Let(first) = &view.state[0] else {
        panic!("状态应以变量声明开始");
    };
    assert!(first.mutable, "状态变量应允许修改");
    assert_eq!(first.name.as_str(), "次数");

    let ViewContent::Children(children) = &view.content else {
        panic!("视图根节点应包含子节点");
    };
    let container = &children[0];

    let ViewContent::Children(nodes) = &container.content else {
        panic!("容器应包含子节点");
    };

    let ViewContent::Parts(parts) = &nodes[0].content else {
        panic!("绑定文本应解析为混合片段，实际 {:?}", nodes[0].content);
    };
    assert_eq!(parts.len(), 2);
    assert!(matches!(parts[0].kind, ViewTextPartKind::Text(_)));
    assert!(matches!(
        parts[1].kind,
        ViewTextPartKind::Expression(Expr {
            kind: ExprKind::Identifier(_),
            ..
        })
    ));

    let button = &nodes[1];
    assert_eq!(button.events.len(), 1, "按钮应绑定一个事件");
    assert_eq!(button.events[0].name.as_str(), "点击");
    assert_eq!(button.events[0].body.stmts.len(), 2);

    let ViewContent::Parts(parts) = &nodes[2].content else {
        panic!("提示文本应解析为表达式绑定");
    };
    assert_eq!(parts.len(), 1);
}

#[test]
fn reports_state_inside_child_node() {
    let errors = parse_err("视图 { 容器 { 状态 { 让 甲 = 1; } } }");
    let message = errors.to_string();

    assert!(
        message.contains("`状态` 只能写在视图根节点"),
        "诊断：\n{message}"
    );
}

#[test]
fn parses_binding_attribute_expression() {
    let program = parse_ok(
        "视图 { 状态 { 让 可变 次数 = 0; } 段落 { 类: { 文本 }, 隐藏: { 次数 等于 0 } } }",
    );
    let view = view(&program);
    let ViewContent::Children(children) = &view.content else {
        panic!("视图根节点应包含子节点");
    };
    let paragraph = &children[0];

    assert!(matches!(
        paragraph.attributes[0].value.kind,
        ViewValueKind::Expression(_)
    ));
    assert!(matches!(
        paragraph.attributes[1].value.kind,
        ViewValueKind::Expression(_)
    ));
}
