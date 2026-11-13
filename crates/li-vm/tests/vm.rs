//! `li-vm` 的栈式虚拟机执行测试。

use li_codegen_bytecode::{from_text, generate, to_text, Program};
use li_vm::Vm;

/// 解析、检查、降级、生成并校验字节码。
fn compile(source: &str) -> Program {
    let program =
        li_parser::parse_source(source).unwrap_or_else(|errors| panic!("语法分析失败：\n{errors}"));
    li_sema::check(&program).unwrap_or_else(|errors| panic!("语义分析失败：\n{errors}"));
    let ir = li_ir::lower(&program).unwrap_or_else(|errors| panic!("IR 降级失败：\n{errors}"));
    let bytecode = generate(&ir);
    bytecode
        .validate()
        .unwrap_or_else(|message| panic!("字节码校验失败：{message}"));
    bytecode
}

/// 执行程序并返回捕获的标准输出。
fn run(program: &Program) -> String {
    let mut vm = Vm::with_input(program, std::iter::empty());
    vm.run()
        .unwrap_or_else(|error| panic!("虚拟机执行失败：{error}"));
    vm.output().to_owned()
}

#[test]
fn executes_recursive_fibonacci() {
    let program = compile(include_str!("../../li-parser/tests/fixtures/fibonacci.li"));
    assert_eq!(run(&program), "55\n");
}

#[test]
fn executes_bytecode_loaded_from_lic_text() {
    let program = compile(include_str!("../../li-parser/tests/fixtures/fibonacci.li"));
    let text = to_text(&program);
    let loaded = from_text(&text).unwrap_or_else(|error| panic!("解析 .lic 失败：{error}"));

    assert_eq!(run(&loaded), "55\n");
}

#[test]
fn executes_while_break_and_continue() {
    let program = compile(
        "\
函数 主() -> 整数 {
    让 可变 计数 = 0;
    当 计数 小于 10 {
        计数 = 计数 加 1;
        如果 计数 等于 5 则 { 跳出; }
        继续;
    }
    打印(计数);
    返回 0;
}",
    );

    assert_eq!(run(&program), "5\n");
}

#[test]
fn executes_optional_and_result_matching() {
    let program = compile(
        "\
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
}",
    );

    assert_eq!(run(&program), "1\n1\n");
}

#[test]
fn executes_lists_and_index_assignment() {
    let program = compile(
        "\
函数 主() -> 整数 {
    让 可变 数据 = [1, 2, 3];
    数据[1] = 9;
    打印(数据[1]);
    打印(数据[0] 加 数据[2]);
    返回 0;
}",
    );

    assert_eq!(run(&program), "9\n4\n");
}

#[test]
fn reads_simulated_input() {
    let program = compile(
        "\
函数 主() -> 整数 {
    让 名字 = 输入;
    打印(名字);
    返回 0;
}",
    );
    let mut vm = Vm::with_input(&program, ["小明".to_owned()]);
    vm.run()
        .unwrap_or_else(|error| panic!("虚拟机执行失败：{error}"));

    assert_eq!(vm.output(), "小明\n");
}

#[test]
fn runtime_errors_include_source_position() {
    let program = compile(
        "\
函数 主() -> 整数 {
    让 值 = 1 除 0;
    打印(值);
    返回 0;
}",
    );
    let mut vm = Vm::with_input(&program, std::iter::empty());
    let error = vm.run().expect_err("除以零必须产生运行时错误");

    assert!(
        error.message().contains("整数除数不能为零"),
        "错误信息不符合预期：{error}"
    );
    assert_eq!(error.span().start.line, 2, "错误行号不正确：{error}");
    assert!(
        error.to_string().starts_with("第 2 行第 "),
        "错误缺少源码定位：{error}"
    );
    assert!(
        error.to_string().contains("函数「主」"),
        "错误缺少函数信息：{error}"
    );
}

#[test]
fn stops_runaway_recursion_at_the_call_depth_limit() {
    let program = compile(
        "\
函数 递归(值: 整数) -> 整数 {
    返回 递归(值);
}

函数 主() -> 整数 {
    返回 递归(0);
}",
    );
    let mut vm = Vm::with_input(&program, std::iter::empty());
    let error = vm.run().expect_err("无限递归必须被调用深度限制阻止");

    assert!(
        error.message().contains("调用深度超过上限"),
        "错误信息不符合预期：{error}"
    );
}
