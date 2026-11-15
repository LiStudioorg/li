//! `li-vm` 的栈式虚拟机执行测试。

use std::time::{SystemTime, UNIX_EPOCH};

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
fn executes_builtin_string_and_list_methods() {
    let program = compile(
        "\
函数 主() -> 整数 {
    让 可变 数字们: 列表<整数> = [1, 2];
    数字们 = 数字们.添加(3);
    打印(数字们.长度());
    打印(数字们);
    打印(数字们.删除(2));
    打印(数字们);

    让 文本: 字符串 = 「你好，li」;
    打印(文本.长度());
    打印(文本.拼接(「！」));
    打印(文本.包含(「li」));
    打印(文本.切片(0, 2));
    打印(文本.替换(「li」, 「世界」));
    打印(「甲,乙,丙」.分割(「,」));
    匹配 文本.字符(0) {
        成功(字符) => { 打印(字符); },
        失败 => { 打印(「空」); },
    }
    匹配 文本.查找(「li」) {
        成功(下标) => { 打印(下标); },
        失败 => { 打印(-1); },
    }
    匹配 「42」.转整数() {
        成功(数字) => { 打印(数字); },
        失败(错误) => { 打印(错误); },
    }
    返回 0;
}",
    );

    assert_eq!(
        run(&program),
        "3\n[1, 2, 3]\n真\n[1, 2]\n5\n你好，li！\n真\n你好\n你好，世界\n[甲, 乙, 丙]\n你\n3\n42\n"
    );
}

#[test]
fn executes_dictionary_insert_get_and_remove() {
    let program = compile(
        "\
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
    匹配 电话簿.获取(「小红」) {
        成功(号码) => { 打印(号码); },
        失败 => { 打印(「未找到」); },
    }
    返回 0;
}",
    );

    assert_eq!(run(&program), "2\n123\n真\n真\n1\n未找到\n");
}

#[test]
fn executes_file_read_and_write() {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("系统时间早于 UNIX 纪元")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("li-vm-{}-{nanos}.txt", std::process::id()));
    let path_text = path.to_string_lossy().replace('\\', "\\\\");

    let source = format!(
        "\
函数 主() -> 整数 {{
    匹配 写入文本(「{path_text}」, 「li\\n中文」) {{
        成功(字节数) => {{ 打印(字节数); }},
        失败(错误) => {{ 打印(错误); }},
    }}
    匹配 读取文本(「{path_text}」) {{
        成功(内容) => {{ 打印(内容); }},
        失败(错误) => {{ 打印(错误); }},
    }}
    返回 0;
}}"
    );
    let program = compile(&source);
    let output = run(&program);
    let _ = std::fs::remove_file(&path);

    assert_eq!(output, "9\nli\n中文\n");
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
fn executes_regex_json_and_crypto_builtins() {
    let program = compile(
        "\
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
    打印(系统.时间戳());
    返回 0;
}",
    );

    let output = run(&program);
    let mut lines = output.lines();
    assert_eq!(lines.next(), Some("真"), "正则匹配结果不正确");
    assert_eq!(
        lines.next(),
        Some("abc[数字]def[数字]"),
        "正则替换结果不正确"
    );
    assert_eq!(lines.next(), Some("bGk="), "Base64 编码结果不正确");
    assert_eq!(
        lines.next(),
        Some("00a9e4255a5b63067b76cbfb9fd67f26bdb91be802d5ffcb177ec1b7a8d4c623"),
        "SHA-256 摘要不正确"
    );
    assert_eq!(lines.next(), Some("li"), "JSON 解析结果不正确");
    assert_eq!(lines.next(), Some("{\"键\":\"值\"}"), "JSON 生成结果不正确");
    let timestamp = lines.next().expect("缺少时间戳输出");
    assert!(
        timestamp.parse::<i64>().is_ok_and(|value| value > 0),
        "时间戳必须为正整数，实际为「{timestamp}」"
    );
    assert_eq!(lines.next(), None, "输出行数多于预期");
}

#[test]
fn fetches_http_json_and_hashes_it() {
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
    let program = compile(&source);
    let output = run(&program);
    handle.join().expect("HTTP 服务线程异常");

    let expected = format!(
        "{body}\n真\neyLlkI3lrZciOiJsaSIsIueJiOacrCI6IjIuNSJ9\nb09aded8798b2c4bc15562e4ffd79ada85dc888d6de922b186141d1d3d016390\n2.5\n"
    );
    assert_eq!(output, expected);
}
