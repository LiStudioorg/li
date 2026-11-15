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
    li_codegen_js::generate(&ir).unwrap_or_else(|message| panic!("JavaScript 生成失败：{message}"))
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
        .arg("--experimental-vm-modules")
        .arg(&path)
        .output()
        .expect("无法启动 Node.js");
    let _ = fs::remove_file(&path);

    if !output.status.success() {
        eprintln!(
            "Node.js FAILED: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let lines: Vec<&str> = source.lines().collect();
        eprintln!("SOURCE TAIL:");
        for line in lines.iter().rev().take(5) {
            eprintln!("{line}");
        }
    }
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
        javascript.contains("async function 斐波那契(项数)"),
        "缺少函数声明：\n{javascript}"
    );
    assert!(
        javascript.contains("async function 主()"),
        "缺少主函数：\n{javascript}"
    );
    assert!(
        javascript.contains("console.log(__li_显示(值));"),
        "打印应映射为 console.log：\n{javascript}"
    );
    assert!(
        javascript.contains("(async () => { await 主.call(null); })();"),
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
    assert!(
        javascript.contains("console.log(__li_显示(99));"),
        "缺少失败分支"
    );
    assert!(
        javascript.contains("console.log(__li_显示(值));"),
        "缺少成功分支"
    );

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
        javascript.contains("console.log(__li_显示(\"你好，li\"));"),
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
        javascript.contains("console.log(__li_显示(_li_console));"),
        "打印必须仍然调用全局 console：\n{javascript}"
    );

    if let Some(output) = run_node(&javascript) {
        assert_eq!(output, "1\n");
    }
}

#[test]
fn executes_standard_library_methods_in_node() {
    let source = "\
函数 主() -> 整数 {
    让 可变 数字们: 列表<整数> = [1, 2];
    数字们 = 数字们.添加(3);
    打印(数字们.长度());
    打印(数字们);

    让 文本: 字符串 = 「你好，li」;
    打印(文本.长度());
    打印(文本.切片(0, 2));
    打印(文本.替换(「li」, 「世界」));
    打印(「甲,乙,丙」.分割(「,」));
    返回 0;
}";
    let javascript = compile(source);

    if let Some(output) = run_node(&javascript) {
        assert_eq!(output, "3\n[1, 2, 3]\n5\n你好\n你好，世界\n[甲, 乙, 丙]\n");
    }
}

#[test]
fn executes_dictionary_methods_in_node() {
    let source = "\
函数 主() -> 整数 {
    让 可变 电话簿: 字典<字符串, 字符串> = 新建字典();
    电话簿 = 电话簿.插入(「小明」, 「123」);
    电话簿 = 电话簿.插入(「小红」, 「456」);
    打印(电话簿.长度());
    匹配 电话簿.获取(「小明」) {
        成功(号码) => { 打印(号码); },
        失败 => { 打印(「未找到」); },
    }
    打印(电话簿.包含键(「小红」));
    打印(电话簿.删除(「小红」));
    打印(电话簿.长度());
    返回 0;
}";
    let javascript = compile(source);

    if let Some(output) = run_node(&javascript) {
        assert_eq!(output, "2\n123\n真\n真\n1\n");
    }
}

#[test]
fn rejects_unsafe_blocks_for_javascript_target() {
    let program = li_parser::parse_source(include_str!(
        "../../li-parser/tests/fixtures/unsafe_raw_pointer.li"
    ))
    .expect("语法分析失败");
    li_sema::check(&program).expect("语义分析失败");
    let ir = li_ir::lower(&program).expect("IR 降级失败");

    let message =
        li_codegen_js::generate(&ir).expect_err("JavaScript 后端必须拒绝不安全块与原生指针");
    assert!(
        message.contains("当前目标平台不支持底层内存操作"),
        "错误信息不符合预期：{message}"
    );
}

/// 在本地启动一次性 HTTP 服务，返回访问地址；只响应一个请求。
fn spawn_once_http_server(body: &'static str) -> (String, std::thread::JoinHandle<()>) {
    use std::io::{Read, Write};
    use std::net::TcpListener;

    let listener =
        TcpListener::bind("127.0.0.1:0").unwrap_or_else(|error| panic!("无法绑定端口：{error}"));
    let address = listener
        .local_addr()
        .unwrap_or_else(|error| panic!("无法读取本地地址：{error}"));
    let handle = std::thread::spawn(move || {
        let (mut stream, _) = listener
            .accept()
            .unwrap_or_else(|error| panic!("无法接受连接：{error}"));
        let mut buffer = [0_u8; 1024];
        let _ = stream.read(&mut buffer);
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        let _ = stream.write_all(response.as_bytes());
        let _ = stream.flush();
    });

    (format!("http://{address}/数据"), handle)
}

#[test]
fn generates_regex_json_and_crypto_builtins() {
    let source = "\
函数 主() -> 整数 {
    打印(正则.匹配(「li 编译器 v2」, 「li.*v[0-9]」));
    打印(正则.替换(「abc123def456」, 「[0-9]+」, 「[数字]」));
    打印(加密.Base64编码(「li」));
    打印(加密.SHA256(「li」));

    匹配 JSON.解析(「{\"名字\":\"li\",\"版本\":\"2.5\"}」) {
        成功(数据) => {
            匹配 数据.获取(「名字」) {
                成功(名字) => { 打印(名字); },
                失败 => { 打印(「缺名字」); },
            }
        },
        失败(错误) => { 打印(错误); },
    }

    让 可变 数据表: 字典<字符串, 字符串> = 新建字典();
    数据表 = 数据表.插入(「键」, 「值」);
    打印(JSON.生成(数据表));
    返回 0;
}";
    let javascript = compile(source);

    if let Some(output) = run_node(&javascript) {
        assert_eq!(
            output,
            "真\nabc[数字]def[数字]\nbGk=\n00a9e4255a5b63067b76cbfb9fd67f26bdb91be802d5ffcb177ec1b7a8d4c623\nli\n{\"键\":\"值\"}\n"
        );
    }
}

#[test]
fn generates_http_fetch_json_pipeline() {
    let body = "{\"名字\":\"li\",\"版本\":\"2.5\"}";
    let (url, handle) = spawn_once_http_server(body);
    let source = format!(
        "\
函数 主() -> 整数 {{
    匹配 网络.获取(「{url}」) {{
        成功(正文) => {{
            打印(正文);
            打印(正则.匹配(正文, 「l.」));
            打印(加密.Base64编码(正文));
            打印(加密.SHA256(正文));
            匹配 JSON.解析(正文) {{
                成功(数据) => {{
                    匹配 数据.获取(「版本」) {{
                        成功(版本) => {{ 打印(版本); }},
                        失败 => {{ 打印(「缺版本」); }},
                    }}
                }},
                失败(错误) => {{ 打印(错误); }},
            }}
        }},
        失败(错误) => {{
            打印(错误);
            返回 2;
        }},
    }}
    返回 0;
}}"
    );
    let javascript = compile(&source);

    if let Some(output) = run_node(&javascript) {
        handle.join().expect("HTTP 服务线程异常");
        assert_eq!(
            output,
            format!(
                "{body}\n真\neyLlkI3lrZciOiJsaSIsIueJiOacrCI6IjIuNSJ9\nb09aded8798b2c4bc15562e4ffd79ada85dc888d6de922b186141d1d3d016390\n2.5\n"
            )
        );
    }
}
