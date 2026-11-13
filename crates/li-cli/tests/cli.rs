//! CLI 端到端测试。

use std::fs;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_directory(prefix: &str) -> std::path::PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("系统时间早于 UNIX 纪元")
        .as_nanos();
    let directory = std::env::temp_dir().join(format!("{prefix}-{}-{nanos}", std::process::id()));
    fs::create_dir_all(&directory).expect("无法创建临时目录");
    directory
}

#[test]
fn compiles_a_li_file_to_javascript() {
    let directory = temp_directory("li-cli-成功");
    let input = directory.join("示例.li");
    fs::write(
        &input,
        "函数 主() -> 整数 {\n    打印(42);\n    返回 0;\n}\n",
    )
    .expect("无法写入 li 源文件");

    let output = Command::new(env!("CARGO_BIN_EXE_li"))
        .arg("编译-js")
        .arg(&input)
        .output()
        .expect("无法启动 li 编译器");

    assert!(
        output.status.success(),
        "编译失败：\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("已生成 JavaScript"),
        "缺少成功提示：{stdout}"
    );

    let javascript_path = directory.join("示例.js");
    let javascript = fs::read_to_string(&javascript_path).expect("未生成 JavaScript 文件");
    assert!(javascript.contains("console.log(42);"));
    assert!(javascript.contains("主();"));

    if Command::new("node").arg("--version").output().is_ok() {
        let result = Command::new("node")
            .arg(&javascript_path)
            .output()
            .expect("无法启动 Node.js");
        assert!(
            result.status.success(),
            "Node.js 执行失败：\n{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&result.stdout), "42\n");
    }

    let _ = fs::remove_dir_all(&directory);
}

#[test]
fn compiles_and_executes_bytecode() {
    let directory = temp_directory("li-cli-字节码");
    let input = directory.join("斐波那契.li");
    fs::write(
        &input,
        include_str!("../../li-parser/tests/fixtures/fibonacci.li"),
    )
    .expect("无法写入 li 源文件");

    let compile = Command::new(env!("CARGO_BIN_EXE_li"))
        .arg("编译")
        .arg(&input)
        .output()
        .expect("无法启动 li 编译器");
    assert!(
        compile.status.success(),
        "编译失败：\n{}",
        String::from_utf8_lossy(&compile.stderr)
    );

    let stdout = String::from_utf8_lossy(&compile.stdout);
    assert!(stdout.contains("已生成字节码"), "缺少成功提示：{stdout}");

    let bytecode_path = directory.join("斐波那契.lic");
    let bytecode = fs::read_to_string(&bytecode_path).expect("未生成 .lic 文件");
    assert!(bytecode.contains(".entry 主"), "缺少入口声明：\n{bytecode}");
    assert!(
        bytecode.contains("CALL 斐波那契 1"),
        "缺少函数调用指令：\n{bytecode}"
    );

    let execute = Command::new(env!("CARGO_BIN_EXE_li"))
        .arg("执行")
        .arg(&bytecode_path)
        .output()
        .expect("无法启动 li 虚拟机");
    assert!(
        execute.status.success(),
        "执行失败：\n{}",
        String::from_utf8_lossy(&execute.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&execute.stdout), "55\n");

    let _ = fs::remove_dir_all(&directory);
}

#[test]
fn reports_chinese_semantic_errors_with_positions() {
    let directory = temp_directory("li-cli-失败");
    let input = directory.join("错误.li");
    fs::write(
        &input,
        "函数 主() -> 整数 {\n    让 值 = 1;\n    值 = 2;\n    返回 值;\n}\n",
    )
    .expect("无法写入 li 源文件");

    let output = Command::new(env!("CARGO_BIN_EXE_li"))
        .arg("编译-js")
        .arg(&input)
        .output()
        .expect("无法启动 li 编译器");

    assert!(!output.status.success(), "无效程序不应编译成功");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("第 3 行"), "错误缺少行号：{stderr}");
    assert!(stderr.contains("第 5 列"), "错误缺少列号：{stderr}");
    assert!(
        stderr.contains("不可变，不能赋值"),
        "错误信息不是预期的中文内容：{stderr}"
    );

    let _ = fs::remove_dir_all(&directory);
}
