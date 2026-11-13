//! `li-sema` 的集成测试。
//!
//! 每个用例先用 `li-parser` 解析源码，再交给语义分析器检查。
//! 覆盖符号表作用域、类型推导、函数与结构体检查、不可变变量、
//! 可选类型判空、结果类型强制匹配以及禁止隐式转换等规则。

use li_errors::DiagnosticBag;

/// 解析并执行语义分析。
fn check(source: &str) -> Result<(), DiagnosticBag> {
    let program =
        li_parser::parse_source(source).unwrap_or_else(|errors| panic!("语法分析失败：\n{errors}"));
    li_sema::check(&program)
}

/// 断言源码能通过语义分析。
fn assert_ok(source: &str) {
    match check(source) {
        Ok(()) => {}
        Err(errors) => panic!("语义分析不应报错：\n{errors}"),
    }
}

/// 断言源码无法通过语义分析，并返回诊断文本。
fn diagnostics(source: &str) -> String {
    match check(source) {
        Ok(()) => panic!("预期语义错误，但分析通过了"),
        Err(errors) => errors.to_string(),
    }
}

/// 断言诊断文本包含全部片段。
fn assert_contains(source: &str, fragments: &[&str]) {
    let message = diagnostics(source);

    for fragment in fragments {
        assert!(
            message.contains(fragment),
            "缺少诊断片段 `{fragment}`：\n{message}"
        );
    }
}

#[test]
fn fibonacci_fixture_passes_without_diagnostics() {
    let source = include_str!("../../li-parser/tests/fixtures/fibonacci.li");
    assert_ok(source);
}

#[test]
fn accepts_a_well_typed_program() {
    let source = "\
类型 用户 {
    名字: 字符串,
    年龄: 整数,
}

函数 取名字(用户值: 用户) -> 字符串 {
    返回 用户值.名字;
}

函数 加一(值: 整数) -> 整数 {
    返回 值 加 1;
}

函数 主() -> 整数 {
    让 可变 计数: 整数 = 0;
    让 文本: 字符串 = 「你好」;
    让 比例: 小数 = 1.5;
    让 数字们: 列表<整数> = [1, 2, 3];
    计数 = 加一(计数);
    如果 计数 大于 0 且 真 则 {
        打印(文本);
        打印(数字们[0]);
    } 否则 {
        打印(比例);
    }
    当 计数 小于 10 {
        计数 = 计数 加 1;
    }
    返回 计数;
}";

    assert_ok(source);
}

#[test]
fn rejects_reassignment_of_immutable_variable() {
    let source = "\
函数 主() -> 整数 {
    让 计数 = 0;
    计数 = 1;
    返回 计数;
}";

    assert_contains(
        source,
        &["「计数」不可变，不能赋值；如需可变请用「让 可变」声明"],
    );
}

#[test]
fn allows_reassignment_of_mutable_variable() {
    let source = "\
函数 主() -> 整数 {
    让 可变 计数 = 0;
    计数 = 1;
    返回 计数;
}";

    assert_ok(source);
}

#[test]
fn rejects_reassignment_of_immutable_parameter() {
    let source = "\
函数 主(值: 整数) -> 整数 {
    值 = 2;
    返回 值;
}";

    assert_contains(
        source,
        &["「值」不可变，不能赋值；如需可变请用「让 可变」声明"],
    );
}

#[test]
fn rejects_immutable_field_assignment() {
    let source = "\
类型 用户 {
    计数: 整数,
}

函数 主(用户值: 用户) -> 整数 {
    用户值.计数 = 1;
    返回 0;
}";

    assert_contains(
        source,
        &["「用户值」不可变，不能赋值；如需可变请用「让 可变」声明"],
    );
}

#[test]
fn rejects_implicit_numeric_conversion() {
    let source = "\
函数 主() -> 小数 {
    返回 1 加 2.5;
}";

    assert_contains(
        source,
        &[
            "类型不匹配",
            "期望 整数",
            "实际 小数",
            "li 不支持隐式数值转换",
        ],
    );
}

#[test]
fn rejects_arithmetic_with_string() {
    let source = "\
函数 主() -> 整数 {
    返回 1 加 「二」;
}";

    assert_contains(source, &["期望 数字类型（整数或小数）", "实际 字符串"]);
}

#[test]
fn rejects_non_boolean_logical_operands() {
    let source = "\
函数 主() -> 布尔 {
    返回 真 且 1;
}";

    assert_contains(
        source,
        &[
            "类型不匹配",
            "期望 布尔",
            "实际 整数",
            "逻辑运算的操作数必须是布尔表达式",
        ],
    );
}

#[test]
fn checks_function_argument_count() {
    let source = "\
函数 求和(甲: 整数, 乙: 整数) -> 整数 {
    返回 甲 加 乙;
}

函数 主() -> 整数 {
    返回 求和(1);
}";

    assert_contains(
        source,
        &["函数参数数量不匹配", "期望 2 个参数", "实际 1 个参数"],
    );
}

#[test]
fn checks_function_argument_types() {
    let source = "\
函数 求和(甲: 整数, 乙: 整数) -> 整数 {
    返回 甲 加 乙;
}

函数 主() -> 整数 {
    返回 求和(1, 「二」);
}";

    assert_contains(
        source,
        &["类型不匹配", "期望 整数", "实际 字符串", "第 2 个参数"],
    );
}

#[test]
fn rejects_unknown_function() {
    let source = "\
函数 主() -> 整数 {
    返回 不存在(1);
}";

    assert_contains(source, &["未定义的函数「不存在」"]);
}

#[test]
fn checks_return_type() {
    let source = "\
函数 主() -> 整数 {
    返回 「文本」;
}";

    assert_contains(
        source,
        &["类型不匹配", "期望 整数", "实际 字符串", "函数返回类型"],
    );
}

#[test]
fn rejects_missing_return_value() {
    let source = "\
函数 主() -> 整数 {
    返回;
}";

    assert_contains(source, &["缺少返回值", "期望 整数"]);
}

#[test]
fn checks_struct_fields() {
    let valid = "\
类型 用户 {
    名字: 字符串,
    年龄: 整数,
}

函数 取名字(用户值: 用户) -> 字符串 {
    返回 用户值.名字;
}

函数 取年龄(用户值: 用户) -> 整数 {
    返回 用户值.年龄;
}";
    assert_ok(valid);

    let missing_field = "\
类型 用户 {
    名字: 字符串,
}

函数 主(用户值: 用户) -> 字符串 {
    返回 用户值.年龄;
}";
    assert_contains(missing_field, &["结构体「用户」没有字段「年龄」"]);

    let non_struct = "\
函数 主() -> 整数 {
    让 值 = 1;
    返回 值.字段;
}";
    assert_contains(
        non_struct,
        &["不能访问字段", "期望 结构体类型", "实际 整数"],
    );
}

#[test]
fn rejects_unknown_type_and_duplicate_field() {
    let unknown_type = "\
函数 主(值: 不存在) -> 整数 {
    返回 0;
}";
    assert_contains(unknown_type, &["未知类型「不存在」"]);

    let duplicate_field = "\
类型 用户 {
    名字: 字符串,
    名字: 整数,
}";
    assert_contains(duplicate_field, &["重复声明字段「名字」"]);
}

#[test]
fn checks_conditions_and_equality() {
    let if_condition = "\
函数 主() -> 整数 {
    如果 1 则 {
        打印(1);
    }
    返回 0;
}";
    assert_contains(
        if_condition,
        &["期望 布尔", "实际 整数", "如果条件必须是布尔表达式"],
    );

    let equality = "\
函数 主() -> 布尔 {
    返回 1 等于 「1」;
}";
    assert_contains(
        equality,
        &["相等比较两侧类型必须一致", "期望 整数", "实际 字符串"],
    );
}

#[test]
fn checks_lists_and_indexing() {
    let valid = "\
函数 主() -> 整数 {
    让 数字们: 列表<整数> = [1, 2, 3];
    返回 数字们[0];
}";
    assert_ok(valid);

    let mixed_elements = "\
函数 主() -> 整数 {
    让 数字们: 列表<整数> = [1, 2.5];
    返回 0;
}";
    assert_contains(mixed_elements, &["列表元素类型必须一致"]);

    let invalid_index = "\
函数 主() -> 整数 {
    让 数字们 = [1, 2, 3];
    返回 数字们[「零」];
}";
    assert_contains(invalid_index, &["列表下标必须是整数", "实际 字符串"]);

    let not_a_list = "\
函数 主() -> 整数 {
    让 值 = 1;
    返回 值[0];
}";
    assert_contains(
        not_a_list,
        &["不能使用下标访问", "期望 列表类型", "实际 整数"],
    );
}

#[test]
fn optional_values_must_be_checked_before_use() {
    let direct_print = "\
函数 主() -> 整数 {
    让 可选值: 可选<整数> = 1;
    打印(可选值);
    返回 0;
}";
    assert_contains(direct_print, &["可选值未判空", "「匹配」"]);

    let direct_field = "\
类型 用户 {
    名字: 字符串,
}

函数 主(用户值: 可选<用户>) -> 字符串 {
    返回 用户值.名字;
}";
    assert_contains(direct_field, &["可选值未判空", "可选<用户>"]);
}

#[test]
fn optional_values_can_be_checked_with_match() {
    let source = "\
函数 主() -> 整数 {
    让 可选值: 可选<整数> = 1;
    匹配 可选值 {
        成功(值) => { 打印(值); },
        失败 => { 打印(0); },
    }
    返回 0;
}";

    assert_ok(source);
}

#[test]
fn optional_match_requires_both_branches() {
    let source = "\
函数 主() -> 整数 {
    让 可选值: 可选<整数> = 1;
    匹配 可选值 {
        成功(值) => { 打印(值); },
    }
    返回 0;
}";

    assert_contains(source, &["匹配缺少「失败」分支"]);
}

#[test]
fn optional_failure_branch_cannot_bind_a_value() {
    let source = "\
函数 主() -> 整数 {
    让 可选值: 可选<整数> = 1;
    匹配 可选值 {
        成功(值) => { 打印(值); },
        失败(错误) => { 打印(0); },
    }
    返回 0;
}";

    assert_contains(source, &["可选类型的「失败」分支不携带值"]);
}

#[test]
fn treats_null_as_an_optional_value() {
    let annotated = "\
函数 主() -> 整数 {
    让 可选值: 可选<整数> = 空;
    返回 0;
}";
    assert_ok(annotated);

    let unannotated = "\
函数 主() -> 整数 {
    让 值 = 空;
    返回 0;
}";
    assert_contains(unannotated, &["无法推导「空」的类型"]);
}

#[test]
fn result_values_must_be_matched_before_use() {
    let direct_print = "\
函数 获取() -> 结果<整数, 字符串> {
    返回 1;
}

函数 主() -> 整数 {
    打印(获取());
    返回 0;
}";
    assert_contains(direct_print, &["结果值未处理", "「匹配」"]);

    let direct_arithmetic = "\
函数 获取() -> 结果<整数, 字符串> {
    返回 1;
}

函数 主() -> 整数 {
    返回 获取() 加 1;
}";
    assert_contains(direct_arithmetic, &["结果值未处理"]);
}

#[test]
fn result_values_can_be_checked_with_match() {
    let source = "\
函数 获取() -> 结果<整数, 字符串> {
    返回 1;
}

函数 主() -> 整数 {
    匹配 获取() {
        成功(值) => { 打印(值); },
        失败(错误) => { 打印(错误); },
    }
    返回 0;
}";

    assert_ok(source);
}

#[test]
fn result_match_requires_failure_branch() {
    let source = "\
函数 获取() -> 结果<整数, 字符串> {
    返回 1;
}

函数 主() -> 整数 {
    匹配 获取() {
        成功(值) => { 打印(值); },
    }
    返回 0;
}";

    assert_contains(source, &["匹配缺少「失败」分支"]);
}

#[test]
fn rejects_result_passed_to_non_result_parameter() {
    let source = "\
函数 获取() -> 结果<整数, 字符串> {
    返回 1;
}

函数 需要整数(值: 整数) -> 整数 {
    返回 值;
}

函数 主() -> 整数 {
    返回 需要整数(获取());
}";

    assert_contains(
        source,
        &[
            "类型不匹配",
            "期望 整数",
            "实际 结果<整数, 字符串>",
            "请先使用「匹配」处理成功与失败",
        ],
    );
}

#[test]
fn supports_block_scope_and_shadowing() {
    let source = "\
函数 主() -> 整数 {
    让 值 = 1;
    如果 真 则 {
        让 值 = 「内部」;
        打印(值);
    }
    返回 值;
}";
    assert_ok(source);

    let out_of_scope = "\
函数 主() -> 整数 {
    如果 真 则 {
        让 内部 = 1;
        打印(内部);
    }
    返回 内部;
}";
    assert_contains(out_of_scope, &["未定义的变量「内部」"]);
}

#[test]
fn rejects_duplicate_declarations() {
    let duplicate_variable = "\
函数 主() -> 整数 {
    让 值 = 1;
    让 值 = 2;
    返回 值;
}";
    assert_contains(duplicate_variable, &["同一作用域内重复声明变量「值」"]);

    let duplicate_function = "\
函数 重复() -> 整数 {
    返回 0;
}

函数 重复() -> 整数 {
    返回 1;
}";
    assert_contains(duplicate_function, &["重复声明函数「重复」"]);

    let duplicate_parameter = "\
函数 主(值: 整数, 值: 整数) -> 整数 {
    返回 0;
}";
    assert_contains(duplicate_parameter, &["重复声明参数「值」"]);
}

#[test]
fn checks_break_and_continue_inside_loops() {
    let outside_loop = "\
函数 主() -> 整数 {
    跳出;
    继续;
    返回 0;
}";
    assert_contains(
        outside_loop,
        &["「跳出」只能用于循环内部", "「继续」只能用于循环内部"],
    );

    let inside_loop = "\
函数 主() -> 整数 {
    让 可变 计数 = 0;
    当 真 {
        如果 计数 大于 0 则 { 跳出; }
        计数 = 计数 加 1;
        继续;
    }
    返回 计数;
}";
    assert_ok(inside_loop);
}

#[test]
fn analyzes_functions_inside_modules() {
    let source = "\
模块 工具 {
    类型 用户 {
        名字: 字符串,
    }

    函数 取名字(用户值: 用户) -> 字符串 {
        返回 用户值.名字;
    }

    函数 加一(值: 整数) -> 整数 {
        返回 值 加 1;
    }
}

函数 主() -> 整数 {
    返回 加一(1);
}";

    assert_ok(source);
}
