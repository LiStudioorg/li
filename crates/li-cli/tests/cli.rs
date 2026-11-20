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
    assert!(javascript.contains("console.log(__li_显示(42));"));
    assert!(javascript.contains("(async () => { await 主.call(null); })();"));

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
fn compiles_a_li_file_to_llvm_ir() {
    let directory = temp_directory("li-cli-llvm");
    let input = directory.join("斐波那契.li");
    fs::write(
        &input,
        include_str!("../../li-parser/tests/fixtures/fibonacci.li"),
    )
    .expect("无法写入 li 源文件");

    let output = Command::new(env!("CARGO_BIN_EXE_li"))
        .arg("编译-llvm")
        .arg(&input)
        .output()
        .expect("无法启动 li 编译器");

    assert!(
        output.status.success(),
        "编译失败：\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("已生成 LLVM IR"), "缺少成功提示：{stdout}");

    let llvm_path = directory.join("斐波那契.ll");
    let llvm = fs::read_to_string(&llvm_path).expect("未生成 .ll 文件");
    assert!(llvm.contains("define i32 @main()"), "LLVM IR：\n{llvm}");
    assert!(llvm.contains("@\"斐波那契\""), "LLVM IR：\n{llvm}");
    assert!(llvm.contains("br i1"), "LLVM IR：\n{llvm}");

    // 环境提供 clang 时，顺带验证生成的 IR 可以被 LLVM 接受并执行。
    if Command::new("clang").arg("--version").output().is_ok() {
        let executable = directory.join("斐波那契");
        let clang = Command::new("clang")
            .arg(&llvm_path)
            .arg("-o")
            .arg(&executable)
            .output()
            .expect("无法启动 clang");
        assert!(
            clang.status.success(),
            "clang 编译失败：\n{}",
            String::from_utf8_lossy(&clang.stderr)
        );

        let run = Command::new(&executable)
            .output()
            .expect("无法运行原生可执行文件");
        assert_eq!(String::from_utf8_lossy(&run.stdout), "55\n");
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
fn compiles_a_lui_view_to_html() {
    let directory = temp_directory("li-cli-视图");
    let input = directory.join("你好.lui");
    fs::write(
        &input,
        include_str!("../../li-parser/tests/fixtures/hello.lui"),
    )
    .expect("无法写入 .lui 视图文件");

    let output = Command::new(env!("CARGO_BIN_EXE_li"))
        .arg("编译-html")
        .arg(&input)
        .output()
        .expect("无法启动 li 编译器");

    assert!(
        output.status.success(),
        "编译失败：\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("已生成 HTML"), "缺少成功提示：{stdout}");

    let html_path = directory.join("你好.html");
    let html = fs::read_to_string(&html_path).expect("未生成 .html 文件");
    assert!(html.contains("<!DOCTYPE html>"), "HTML：\n{html}");
    assert!(
        html.contains(r#"<html lang="zh-CN">"#),
        "缺少语言声明：\n{html}"
    );
    assert!(
        html.contains("<h1>你好，li！</h1>"),
        "缺少中文标题：\n{html}"
    );
    assert!(
        html.contains(r#"<button class="主按钮" type="button">点我</button>"#),
        "缺少按钮结构：\n{html}"
    );
    assert!(
        html.contains("<li>声明式视图</li>"),
        "缺少列表结构：\n{html}"
    );
    assert!(html.contains("</html>"), "HTML 文档未闭合：\n{html}");

    let _ = fs::remove_dir_all(&directory);
}

#[test]
fn rejects_non_lui_input_for_html_compilation() {
    let directory = temp_directory("li-cli-视图后缀");
    let input = directory.join("不是视图.li");
    fs::write(&input, "视图 { 容器 { } }\n").expect("无法写入测试文件");

    let output = Command::new(env!("CARGO_BIN_EXE_li"))
        .arg("编译-html")
        .arg(&input)
        .output()
        .expect("无法启动 li 编译器");

    assert!(!output.status.success(), "非 .lui 文件不应编译成功");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains(".lui"), "错误信息缺少后缀说明：{stderr}");
    assert!(
        !directory.join("不是视图.html").exists(),
        "失败时不应生成 HTML 文件"
    );

    let _ = fs::remove_dir_all(&directory);
}

#[test]
fn rejects_views_when_compiling_executable_code() {
    let directory = temp_directory("li-cli-视图不可执行");
    let input = directory.join("视图混入.li");
    fs::write(&input, "视图 { 容器 { } }\n").expect("无法写入测试文件");

    let output = Command::new(env!("CARGO_BIN_EXE_li"))
        .arg("编译-js")
        .arg(&input)
        .output()
        .expect("无法启动 li 编译器");

    assert!(!output.status.success(), "视图声明不应被当作可执行代码");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("视图声明"), "错误信息缺少原因：{stderr}");
    assert!(
        stderr.contains("编译-html"),
        "错误信息缺少 HTML 编译提示：{stderr}"
    );
    assert!(
        !directory.join("视图混入.js").exists(),
        "失败时不应生成 JavaScript 文件"
    );

    let _ = fs::remove_dir_all(&directory);
}

#[test]
fn reports_fatal_semantic_errors_with_positions() {
    // 真正的致命错误（类型不匹配）才允许拦截编译；
    // 不可变性违规只产生黄色警告，编译放行。
    let directory = temp_directory("li-cli-致命");
    let input = directory.join("致命.li");
    fs::write(&input, "函数 主() -> 整数 {\n    返回 「文本」;\n}\n").expect("无法写入 li 源文件");

    let output = Command::new(env!("CARGO_BIN_EXE_li"))
        .arg("编译-js")
        .arg(&input)
        .output()
        .expect("无法启动 li 编译器");

    assert!(!output.status.success(), "致命类型错误不应编译成功");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("第 2 行"), "错误缺少行号：{stderr}");
    assert!(stderr.contains("类型不匹配"), "缺少类型不匹配：{stderr}");

    let _ = fs::remove_dir_all(&directory);
}

/// 强制警告机制：不可变性违规产生黄色警告，编译放行，产物生成。
#[test]
fn immutability_violation_is_mandatory_warning() {
    let directory = temp_directory("li-cli-警告");
    let input = directory.join("默认.li");
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

    // 强制警告机制：编译必须放行，黄色警告出现在 stderr。
    assert!(
        output.status.success(),
        "不可变性违规不应拦截编译：\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("不可变，不能赋值"),
        "缺少黄色警告：{stderr}"
    );
    // 产物（.js）必须已生成，证明「满屏黄字但能编译」成立。
    assert!(input.with_extension("js").exists(), "编译产物（.js）未生成");

    let _ = fs::remove_dir_all(&directory);
}

/// 强制警告机制：`li.toml` 里写 `[检查]` 表已无效；用户不再能
/// 通过配置把警告提升为错误，也不能消除警告。
#[test]
fn li_toml_check_table_no_longer_affects_compilation() {
    let directory = temp_directory("li-cli-无效配置");
    let input = directory.join("无效.li");
    fs::write(
        &input,
        "函数 主() -> 整数 {\n    让 值 = 1;\n    值 = 2;\n    返回 值;\n}\n",
    )
    .expect("无法写入 li 源文件");
    // 故意写一份 `li.toml` 试图把不可变性提升为「报错」；
    // 强制警告机制下，配置无效，编译仍然放行。
    fs::write(
        directory.join("li.toml"),
        "[项目]\n名称 = \"无效\"\n\n[检查]\n不可变性 = \"报错\"\n",
    )
    .expect("无法写入 li.toml");

    let output = Command::new(env!("CARGO_BIN_EXE_li"))
        .arg("编译-js")
        .arg(&input)
        .output()
        .expect("无法启动 li 编译器");

    assert!(
        output.status.success(),
        "li.toml 的 [检查] 表已废弃，不可变性违规不应拦截编译"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("不可变，不能赋值"),
        "黄色警告仍然必须出现：{stderr}"
    );

    let _ = fs::remove_dir_all(&directory);
}

/// 强制警告机制：`@检查(...)` 注解已被移除，源码里写 `@检查` 会
/// 触发词法错误（`@` 不再是合法 token），编译被拦截。
#[test]
fn at_check_annotation_is_no_longer_supported() {
    let directory = temp_directory("li-cli-注解废弃");
    let input = directory.join("注解.li");
    fs::write(
        &input,
        "@检查(不可变性: 忽略)\n函数 主() -> 整数 {\n    让 值 = 1;\n    值 = 2;\n    返回 值;\n}\n",
    )
    .expect("无法写入 li 源文件");

    let output = Command::new(env!("CARGO_BIN_EXE_li"))
        .arg("编译-js")
        .arg(&input)
        .output()
        .expect("无法启动 li 编译器");

    // `@` 不再是合法 token；词法层面就会报错。
    assert!(!output.status.success(), "废弃的 @检查 注解应被词法拦截");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("忽略等级下不应"),
        "不应再出现「忽略」等级相关的提示：{stderr}"
    );

    let _ = fs::remove_dir_all(&directory);
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
fn runs_standard_library_pipeline_through_real_cli() {
    let body = "{\"名字\":\"li\",\"版本\":\"2.5\"}";
    let (url, handle) = spawn_once_http_server(body);
    let expected = format!(
        "{body}\n真\neyLlkI3lrZciOiJsaSIsIueJiOacrCI6IjIuNSJ9\nb09aded8798b2c4bc15562e4ffd79ada85dc888d6de922b186141d1d3d016390\n2.5\n"
    );

    let directory = temp_directory("li-cli-标准库");
    let input = directory.join("标准库.li");
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
    fs::write(&input, &source).expect("无法写入 li 源文件");

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

    let execute = Command::new(env!("CARGO_BIN_EXE_li"))
        .arg("执行")
        .arg(directory.join("标准库.lic"))
        .output()
        .expect("无法启动 li 虚拟机");
    assert!(
        execute.status.success(),
        "执行失败：\n{}",
        String::from_utf8_lossy(&execute.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&execute.stdout), expected);
    handle.join().expect("HTTP 服务线程异常");

    let js_body = "{\"名字\":\"li\",\"版本\":\"2.5\"}";
    let (js_url, js_handle) = spawn_once_http_server(js_body);
    let js_input = directory.join("标准库-js.li");
    fs::write(&js_input, source.replace(&url, &js_url)).expect("无法写入 li 源文件");

    let js_compile = Command::new(env!("CARGO_BIN_EXE_li"))
        .arg("编译-js")
        .arg(&js_input)
        .output()
        .expect("无法启动 li 编译器");
    assert!(
        js_compile.status.success(),
        "JavaScript 编译失败：\n{}",
        String::from_utf8_lossy(&js_compile.stderr)
    );

    if Command::new("node").arg("--version").output().is_ok() {
        let node_run = Command::new("node")
            .arg(directory.join("标准库-js.js"))
            .output()
            .expect("无法启动 Node.js");
        assert!(
            node_run.status.success(),
            "Node.js 执行失败：\n{}",
            String::from_utf8_lossy(&node_run.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&node_run.stdout), expected);
        js_handle.join().expect("HTTP 服务线程异常");
    }

    let _ = fs::remove_dir_all(&directory);
}

#[test]
fn compiles_single_file_interactive_html_through_cli() {
    let directory = temp_directory("li-cli-交互视图");
    let input = directory.join("计数器.lui");
    fs::write(
        &input,
        include_str!("../../li-parser/tests/fixtures/counter.lui"),
    )
    .expect("无法写入视图文件");

    let output = Command::new(env!("CARGO_BIN_EXE_li"))
        .arg("编译-html")
        .arg(&input)
        .output()
        .expect("无法启动 li 编译器");
    assert!(
        output.status.success(),
        "编译失败：\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let html = fs::read_to_string(directory.join("计数器.html")).expect("未生成 HTML");
    assert!(html.contains("<script>"), "产物缺少内联脚本：\n{html}");
    assert!(
        html.contains("addEventListener(\"click\""),
        "产物缺少点击绑定：\n{html}"
    );
    assert!(
        !html.contains("src=") && !html.contains("<link"),
        "产物不得引用外部资源：\n{html}"
    );

    let _ = fs::remove_dir_all(&directory);
}

#[test]
fn serves_single_file_view_over_http() {
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpStream;

    let directory = temp_directory("li-cli-服务");
    let input = directory.join("计数器.lui");
    fs::write(
        &input,
        include_str!("../../li-parser/tests/fixtures/counter.lui"),
    )
    .expect("无法写入视图文件");

    let mut child = Command::new(env!("CARGO_BIN_EXE_li"))
        .arg("服务")
        .arg(&input)
        .arg("0")
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("无法启动 li 服务");

    let stdout = child.stdout.take().expect("无法读取服务输出");
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    reader.read_line(&mut line).expect("无法读取启动信息");
    let address = line
        .trim()
        .strip_prefix("服务已启动：http://")
        .and_then(|rest| rest.strip_suffix('/'))
        .unwrap_or_else(|| panic!("启动信息格式不正确：{line}"));

    let mut stream = TcpStream::connect(address).expect("无法连接 li 服务");
    stream
        .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .expect("无法发送请求");
    let mut response = String::new();
    stream.read_to_string(&mut response).expect("无法读取响应");

    assert!(
        response.starts_with("HTTP/1.1 200 OK"),
        "响应：\n{response}"
    );
    assert!(
        response.contains("当前计数："),
        "响应缺少视图内容：\n{response}"
    );
    assert!(
        response.contains("addEventListener(\"click\""),
        "响应缺少内联交互脚本：\n{response}"
    );

    let mut missing = TcpStream::connect(address).expect("无法连接 li 服务");
    missing
        .write_all(b"GET /missing HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .expect("无法发送请求");
    let mut missing_response = String::new();
    missing
        .read_to_string(&mut missing_response)
        .expect("无法读取响应");
    assert!(
        missing_response.starts_with("HTTP/1.1 404 Not Found"),
        "响应：\n{missing_response}"
    );

    let _ = child.kill();
    let _ = child.wait();
    let _ = fs::remove_dir_all(&directory);
}

/// 创建一个带本地依赖的完整示例项目，返回项目目录。
fn create_dependency_project(directory: &std::path::Path) -> std::path::PathBuf {
    let library = directory.join("数学库");
    fs::create_dir_all(&library).expect("无法创建依赖目录");
    fs::write(
        library.join("主.li"),
        "函数 加法(甲: 整数, 乙: 整数) -> 整数 {\n    返回 甲 加 乙;\n}\n",
    )
    .expect("无法写入依赖源码");

    let application = directory.join("应用");
    fs::create_dir_all(&application).expect("无法创建项目目录");
    fs::write(
        application.join("li.toml"),
        "[项目]\n名称 = \"应用\"\n版本 = \"0.1.0\"\n入口 = \"主.li\"\n\n[依赖]\n数学库 = { 路径 = \"../数学库\" }\n",
    )
    .expect("无法写入项目配置");
    fs::write(
        application.join("主.li"),
        "导入 数学库;\n\n函数 主() -> 整数 {\n    打印(数学库.加法(1, 2));\n    返回 0;\n}\n",
    )
    .expect("无法写入项目源码");

    application
}

#[test]
fn fetches_dependency_then_runs_project() {
    let directory = temp_directory("li-cli-依赖项目");
    let application = create_dependency_project(&directory);

    let fetched = Command::new(env!("CARGO_BIN_EXE_li"))
        .arg("获取")
        .arg(&application)
        .output()
        .expect("无法启动 li 编译器");
    assert!(
        fetched.status.success(),
        "获取依赖失败：\n{}",
        String::from_utf8_lossy(&fetched.stderr)
    );
    let stdout = String::from_utf8_lossy(&fetched.stdout);
    assert!(stdout.contains("已获取 1 个依赖"), "输出：{stdout}");
    assert!(stdout.contains("依赖锁文件"), "输出：{stdout}");
    assert!(application.join("li.lock").is_file(), "未生成 li.lock");
    assert!(
        application.join(".li/依赖/数学库/主.li").is_file(),
        "未安装依赖源码"
    );

    let run = Command::new(env!("CARGO_BIN_EXE_li"))
        .arg("运行")
        .current_dir(&application)
        .output()
        .expect("无法启动 li 编译器");
    assert!(
        run.status.success(),
        "运行项目失败：\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "3\n");

    let _ = fs::remove_dir_all(&directory);
}

#[test]
fn run_reports_missing_dependency_lock() {
    let directory = temp_directory("li-cli-缺少锁文件");
    let application = create_dependency_project(&directory);

    let run = Command::new(env!("CARGO_BIN_EXE_li"))
        .arg("运行")
        .current_dir(&application)
        .output()
        .expect("无法启动 li 编译器");

    assert!(!run.status.success(), "缺少锁文件时不应运行成功");
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(stderr.contains("缺少依赖锁文件"), "错误输出：{stderr}");
    assert!(stderr.contains("li 获取"), "错误输出缺少建议：{stderr}");

    let _ = fs::remove_dir_all(&directory);
}

#[test]
fn desktop_command_falls_back_in_headless_environment() {
    // M15：无头 CI 环境没有 WebKit2GTK，`li 桌面` 必须输出降级日志并成功退出。
    let directory = temp_directory("li-cli-桌面");
    let input = directory.join("桌面.li");
    fs::write(&input, "函数 主() -> 整数 { 返回 0; }\n").expect("无法写入 li 源文件");
    // 复用 fixture 中的 .lui 文件，避免额外维护。
    let lui = copy_fixture_to(&directory, "hello.lui");

    let output = Command::new(env!("CARGO_BIN_EXE_li"))
        .arg("桌面")
        .arg(&lui)
        .output()
        .expect("无法启动 li 编译器");

    assert!(
        output.status.success(),
        "桌面命令在无头环境应正常退出：\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("桌面 GUI 模式初始化成功"),
        "缺少降级日志：{stderr}"
    );

    let _ = fs::remove_dir_all(&directory);
}

/// 把 fixture 的 `.lui` 文件复制到测试目录，返回目标路径。
fn copy_fixture_to(directory: &std::path::Path, name: &str) -> std::path::PathBuf {
    let source = std::env::var("CARGO_MANIFEST_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::env::current_dir().unwrap())
        .join("..")
        .join("li-parser")
        .join("tests")
        .join("fixtures")
        .join(name);
    let target = directory.join(name);
    std::fs::copy(&source, &target).unwrap_or_else(|_| {
        panic!(
            "无法复制 fixture「{name}」（源：{}）到测试目录",
            source.display()
        );
    });
    target
}
