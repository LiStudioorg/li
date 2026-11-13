//! `li-ir` 的降级测试。
//!
//! 测试会先执行语法分析与语义分析，再把 AST 降级为 IR，
//! 重点验证类型保留、三地址码以及 `LABEL` / `JUMP` / `BRANCH` / `RETURN`。

use li_ir::{Instruction, IrType, Program, Terminator};

/// 解析、检查并降级一段源码。
fn lower_source(source: &str) -> Program {
    let program =
        li_parser::parse_source(source).unwrap_or_else(|errors| panic!("语法分析失败：\n{errors}"));
    li_sema::check(&program).unwrap_or_else(|errors| panic!("语义分析失败：\n{errors}"));
    li_ir::lower(&program).unwrap_or_else(|errors| panic!("IR 降级失败：\n{errors}"))
}

#[test]
fn lowers_fibonacci_with_types_and_control_flow() {
    let source = include_str!("../../li-parser/tests/fixtures/fibonacci.li");
    let program = lower_source(source);

    assert_eq!(program.functions.len(), 2);

    let fibonacci = program.function("斐波那契").expect("缺少斐波那契函数");
    assert_eq!(fibonacci.params.len(), 1);
    assert_eq!(fibonacci.params[0].name, "项数");
    assert_eq!(fibonacci.params[0].ty, IrType::Integer);
    assert_eq!(fibonacci.return_type, IrType::Integer);

    let main = program.function("主").expect("缺少主函数");
    assert!(main
        .locals
        .iter()
        .any(|local| local.name == "值" && local.ty == IrType::Integer));

    let text = program.to_text();
    assert!(text.contains("LABEL L0"), "缺少 LABEL：\n{text}");
    assert!(text.contains("BRANCH"), "缺少 BRANCH：\n{text}");
    assert!(text.contains("RETURN"), "缺少 RETURN：\n{text}");
    assert!(text.contains("打印 值"), "缺少打印指令：\n{text}");
}

#[test]
fn lowers_while_break_and_continue_to_jumps() {
    let source = "\
函数 主() -> 整数 {
    让 可变 计数 = 0;
    当 计数 小于 10 {
        计数 = 计数 加 1;
        如果 计数 等于 5 则 { 跳出; }
        继续;
    }
    返回 计数;
}";
    let program = lower_source(source);
    let main = program.function("主").expect("缺少主函数");

    let mut jump_count = 0;
    let mut branch_count = 0;
    for block in &main.blocks {
        match &block.terminator {
            Terminator::Jump(_) => jump_count += 1,
            Terminator::Branch { .. } => branch_count += 1,
            Terminator::Return(_) => {}
        }
    }

    assert!(jump_count >= 3, "跳出与继续应降级为 JUMP");
    assert!(branch_count >= 2, "如果与当应降级为 BRANCH");

    let text = program.to_text();
    assert!(text.contains("JUMP"), "缺少 JUMP：\n{text}");
    assert!(text.contains("BRANCH"), "缺少 BRANCH：\n{text}");

    let counter = main
        .locals
        .iter()
        .find(|local| local.name == "计数")
        .expect("缺少计数变量");
    assert_eq!(counter.ty, IrType::Integer);
    assert!(counter.mutable);
}

#[test]
fn wraps_optional_and_result_values() {
    let source = "\
函数 获取() -> 结果<整数, 字符串> {
    返回 1;
}

函数 主() -> 整数 {
    让 可选值: 可选<整数> = 1;
    匹配 可选值 {
        成功(值) => { 打印(值); },
        失败 => { 打印(0); },
    }
    匹配 获取() {
        成功(值) => { 打印(值); },
        失败(错误) => { 打印(错误); },
    }
    返回 0;
}";
    let program = lower_source(source);

    let mut has_wrap_optional = false;
    let mut has_wrap_result = false;
    let mut has_is_success = false;
    let mut unwrap_count = 0;

    for block in program
        .functions
        .iter()
        .flat_map(|function| &function.blocks)
    {
        for instruction in &block.instructions {
            match instruction {
                Instruction::WrapOptional { .. } => has_wrap_optional = true,
                Instruction::WrapResult { .. } => has_wrap_result = true,
                Instruction::IsSuccess { .. } => has_is_success = true,
                Instruction::Unwrap { .. } => unwrap_count += 1,
                _ => {}
            }
        }
    }

    assert!(has_wrap_optional, "可选值应生成 WrapOptional");
    assert!(has_wrap_result, "结果值应生成 WrapResult");
    assert!(has_is_success, "匹配应生成 IsSuccess");
    assert!(unwrap_count >= 2, "匹配分支应解包成功值");
}

#[test]
fn flattens_functions_declared_inside_modules() {
    let source = "\
模块 工具 {
    函数 加一(值: 整数) -> 整数 {
        返回 值 加 1;
    }
}

函数 主() -> 整数 {
    返回 加一(1);
}";
    let program = lower_source(source);

    assert!(program.function("加一").is_some());
    assert!(program.function("主").is_some());

    let call = program
        .function("主")
        .expect("缺少主函数")
        .blocks
        .iter()
        .flat_map(|block| &block.instructions)
        .find_map(|instruction| match instruction {
            Instruction::Call { function, ty, .. } => Some((function, ty)),
            _ => None,
        })
        .expect("缺少调用指令");

    assert_eq!(call.0, "加一");
    assert_eq!(call.1, &IrType::Integer);
}
