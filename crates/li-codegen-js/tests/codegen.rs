//! `li-codegen-js` 的代码生成测试。
//!
//! 除字符串断言外，部分测试会直接调用 Node.js 执行生成的 JavaScript，
//! 以验证控制流、可选值与结果值的运行时行为。若环境没有 Node.js，
//! 相关测试会输出提示并跳过执行部分。

use std::fs;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

/// 解析、检查、降级并生成 JavaScript。
fn compile(source: &str) -> String {
    let program =
        li_parser::parse_source(source).unwrap_or_else(|errors| panic!("语法分析失败：\n{errors}"));
    li_sema::check(&program).unwrap_or_else(|errors| panic!("语义分析失败：\n{errors}"));
    let ir = li_ir::lower(&program).unwrap_or_else(|errors| panic!("IR 降级失败：\n{errors}"));
    li_codegen_js::generate(&ir)
}

/// 用 Node.js 执行 JavaScript，返回标准输出；没有 Node.js 时返回 `None`。
fn run_node(source: &str) -> Option<String> {
    if Command::new("node").arg("--version").output().is_err() {
        eprintln!("未找到 Node.js，跳过运行时验证。");
        return None;
    }

    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("系统时间早于 UNIX 纪元")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("li-codegen-{}-{nanos}.js", std::process::id()));

    fs::write(&path, source).expect("无法写入临时 JavaScript 文件");
    let output = Command::new("node")
        .arg(&path)
        .output()
        .expect("无法启动 Node.js");
    let _ = fs::remove_file(&path);

    assert!(
        output.status.success(),
        "Node.js 执行失败：\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    Some(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[test]
fn generates_runnable_fibonacci() {
    let source = include_str!("../../li-parser/tests/fixtures/fibonacci.li");
    let javascript = compile(source);

    assert!(
        javascript.contains("function 斐波那契(项数)"),
        "缺少函数声明：\n{javascript}"
    );
    assert!(
        javascript.contains("function 主()"),
        "缺少主函数：\n{javascript}"
    );
    assert!(
        javascript.contains("console.log(值);"),
        "打印应映射为 console.log：\n{javascript}"
    );
    assert!(
        javascript.contains("主();"),
        "缺少程序入口调用：\n{javascript}"
    );
    assert!(
        javascript.contains("while (true)") && javascript.contains("switch (__li_状态)"),
        "控制流应生成状态机：\n{javascript}"
    );

    if let Some(output) = run_node(&javascript) {
        assert_eq!(output, "55\n", "斐波那契(10) 应输出 55");
    }
}

#[test]
fn generates_while_and_break_control_flow() {
    let source = "\
函数 主() -> 整数 {
    让 可变 计数 = 0;
    当 计数 小于 10 {
        计数 = 计数 加 1;
        如果 计数 等于 5 则 { 跳出; }
    }
    打印(计数);
    返回 0;
}";
    let javascript = compile(source);

    assert!(javascript.contains("__li_状态 ="), "缺少状态跳转");
    assert!(javascript.contains("continue;"), "缺少循环继续语句");

    if let Some(output) = run_node(&javascript) {
        assert_eq!(output, "5\n");
    }
}

#[test]
fn generates_optional_and_result_matching() {
    let source = "\
函数 获取() -> 结果<整数, 字符串> {
    返回 7;
}

函数 主() -> 整数 {
    让 可选值: 可选<整数> = 空;
    匹配 可选值 {
        成功(值) => { 打印(值); },
        失败 => { 打印(99); },
    }
    匹配 获取() {
        成功(值) => { 打印(值); },
        失败(错误) => { 打印(错误); },
    }
    返回 0;
}";
    let javascript = compile(source);

    assert!(javascript.contains("__li_成功"), "缺少可选值包装");
    assert!(javascript.contains("__li_值"), "缺少结果值包装");
    assert!(javascript.contains("console.log(99);"), "缺少失败分支");
    assert!(javascript.contains("console.log(值);"), "缺少成功分支");

    if let Some(output) = run_node(&javascript) {
        assert_eq!(output, "99\n7\n");
    }
}

#[test]
fn escapes_string_literals() {
    let source = "\
函数 主() -> 整数 {
    打印(「你好，li」);
    返回 0;
}";
    let javascript = compile(source);

    assert!(
        javascript.contains("console.log(\"你好，li\");"),
        "字符串未正确生成：\n{javascript}"
    );
}

#[test]
fn avoids_shadowing_runtime_globals() {
    let source = "\
函数 主() -> 整数 {
    让 console = 1;
    打印(console);
    返回 0;
}";
    let javascript = compile(source);

    assert!(
        javascript.contains("let _li_console;"),
        "局部变量应避开 console：\n{javascript}"
    );
    assert!(
        javascript.contains("console.log(_li_console);"),
        "打印必须仍然调用全局 console：\n{javascript}"
    );

    if let Some(output) = run_node(&javascript) {
        assert_eq!(output, "1\n");
    }
}
