//! li 编译器命令行入口。
//!
//! 提供六条主要命令：
//! - `li 编译 <文件.li>`：编译为同名 `.lic` 字节码文件；
//! - `li 执行 <文件.lic>`：在栈式虚拟机中加载并执行字节码；
//! - `li 编译-js <文件.li>`：编译为同名 JavaScript 文件；
//! - `li 编译-llvm <文件.li>`：生成同名文本 LLVM IR（`.ll`）；
//! - `li 编译-html <文件.lui>`：把视图文件编译为同名单体 HTML 文件；
//! - `li 服务 <文件.lui> [端口]`：编译视图并在内存中直接提供 HTTP 服务。
//!
//! 前端流程完全复用：语法分析 → 语义检查 → 中立 IR 降级 → 后端代码生成；
//! HTML 后端直接消费视图 AST，不经过 IR。

use std::env;
use std::fs;
use std::io::ErrorKind;
use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();

    let Some(command) = args.first() else {
        print_usage();
        return ExitCode::SUCCESS;
    };

    match command.as_str() {
        "编译" => {
            if args.len() != 2 {
                eprintln!("错误：命令「编译」需要一个 .li 文件参数");
                print_usage();
                return ExitCode::FAILURE;
            }
            compile_to_bytecode(Path::new(&args[1]))
        }
        "执行" => {
            if args.len() != 2 {
                eprintln!("错误：命令「执行」需要一个 .lic 文件参数");
                print_usage();
                return ExitCode::FAILURE;
            }
            execute_bytecode(Path::new(&args[1]))
        }
        "编译-js" => {
            if args.len() != 2 {
                eprintln!("错误：命令「编译-js」需要一个 .li 文件参数");
                print_usage();
                return ExitCode::FAILURE;
            }
            compile_to_javascript(Path::new(&args[1]))
        }
        "编译-llvm" => {
            if args.len() != 2 {
                eprintln!("错误：命令「编译-llvm」需要一个 .li 文件参数");
                print_usage();
                return ExitCode::FAILURE;
            }
            compile_to_llvm(Path::new(&args[1]))
        }
        "编译-html" => {
            if args.len() != 2 {
                eprintln!("错误：命令「编译-html」需要一个 .lui 文件参数");
                print_usage();
                return ExitCode::FAILURE;
            }
            compile_to_html(Path::new(&args[1]))
        }
        "服务" => {
            if args.len() < 2 || args.len() > 3 {
                eprintln!("错误：命令「服务」需要一个 .lui 文件参数与可选端口");
                print_usage();
                return ExitCode::FAILURE;
            }

            let port = match args.get(2) {
                Some(text) => match text.parse::<u16>() {
                    Ok(port) => port,
                    Err(_) => {
                        eprintln!("错误：端口必须是 0 到 65535 之间的整数，实际为「{text}」");
                        return ExitCode::FAILURE;
                    }
                },
                None => 8080,
            };

            serve_view(Path::new(&args[1]), port)
        }
        "版本" | "--version" | "-V" => {
            println!("li 编译器 v{}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        "帮助" | "--help" | "-h" => {
            print_usage();
            ExitCode::SUCCESS
        }
        other => {
            eprintln!("错误：未知命令「{other}」");
            print_usage();
            ExitCode::FAILURE
        }
    }
}

fn print_usage() {
    println!("li 编译器 v{}", env!("CARGO_PKG_VERSION"));
    println!("用法：");
    println!("  li 编译 <文件.li>      将 li 源码编译为同名 .lic 字节码文件");
    println!("  li 执行 <文件.lic>     在虚拟机中加载并执行字节码");
    println!("  li 编译-js <文件.li>   将 li 源码编译为同名 .js 文件");
    println!("  li 编译-llvm <文件.li> 生成同名文本 LLVM IR（.ll）");
    println!("  li 编译-html <文件.lui> 将 .lui 视图文件编译为同名 .html 文件");
    println!("  li 服务 <文件.lui> [端口] 编译单体视图并启动 HTTP 服务（默认 8080）");
    println!("  li 版本                显示版本信息");
    println!("  li 帮助                显示帮助信息");
}

/// 依次执行读取、语法分析、语义检查与 IR 降级。
fn compile_to_ir(input: &Path) -> Result<li_ir::Program, ExitCode> {
    if input.extension().and_then(|extension| extension.to_str()) == Some("lui") {
        eprintln!(
            "错误：命令「编译」与「编译-js」不处理 .lui 视图文件，请改用「li 编译-html {}」",
            input.display()
        );
        return Err(ExitCode::FAILURE);
    }

    let source = match fs::read_to_string(input) {
        Ok(source) => source,
        Err(error) => {
            eprintln!(
                "错误：无法读取文件「{}」：{}",
                input.display(),
                io_error_text(&error)
            );
            return Err(ExitCode::FAILURE);
        }
    };

    let program = match li_parser::parse_source(&source) {
        Ok(program) => program,
        Err(errors) => {
            eprintln!("{errors}");
            return Err(ExitCode::FAILURE);
        }
    };

    if contains_view(&program.items) {
        eprintln!("错误：可执行代码编译命令不处理视图声明，请改用「li 编译-html <文件.lui>」");
        return Err(ExitCode::FAILURE);
    }

    if let Err(errors) = li_sema::check(&program) {
        eprintln!("{errors}");
        return Err(ExitCode::FAILURE);
    }

    match li_ir::lower(&program) {
        Ok(ir) => Ok(ir),
        Err(errors) => {
            eprintln!("{errors}");
            Err(ExitCode::FAILURE)
        }
    }
}

fn compile_to_javascript(input: &Path) -> ExitCode {
    let ir = match compile_to_ir(input) {
        Ok(ir) => ir,
        Err(code) => return code,
    };

    let javascript = match li_codegen_js::generate(&ir) {
        Ok(javascript) => javascript,
        Err(message) => {
            eprintln!("错误：{message}");
            return ExitCode::FAILURE;
        }
    };
    let output = input.with_extension("js");

    if let Err(error) = fs::write(&output, javascript) {
        eprintln!(
            "错误：无法写入文件「{}」：{}",
            output.display(),
            io_error_text(&error)
        );
        return ExitCode::FAILURE;
    }

    println!("已生成 JavaScript：{}", output.display());
    ExitCode::SUCCESS
}

/// 把 li 源码生成为同名的文本 LLVM IR（`.ll`）。
///
/// 该命令只依赖编译器本身，不需要本机安装 LLVM；生成的 `.ll`
/// 可以后续交给 `clang` 或 `llc` 生成原生可执行文件。
fn compile_to_llvm(input: &Path) -> ExitCode {
    let ir = match compile_to_ir(input) {
        Ok(ir) => ir,
        Err(code) => return code,
    };

    let llvm_ir = match li_codegen_llvm::generate(&ir) {
        Ok(llvm_ir) => llvm_ir,
        Err(errors) => {
            eprintln!("{errors}");
            return ExitCode::FAILURE;
        }
    };
    let output = input.with_extension("ll");

    if let Err(error) = fs::write(&output, llvm_ir) {
        eprintln!(
            "错误：无法写入文件「{}」：{}",
            output.display(),
            io_error_text(&error)
        );
        return ExitCode::FAILURE;
    }

    println!("已生成 LLVM IR：{}", output.display());
    ExitCode::SUCCESS
}

/// 解析 `.lui` 视图文件并生成单体 HTML 文本。
fn compile_view_to_html(input: &Path) -> Result<String, ExitCode> {
    if input.extension().and_then(|extension| extension.to_str()) != Some("lui") {
        eprintln!("错误：需要 .lui 视图文件，实际为「{}」", input.display());
        return Err(ExitCode::FAILURE);
    }

    let source = match fs::read_to_string(input) {
        Ok(source) => source,
        Err(error) => {
            eprintln!(
                "错误：无法读取文件「{}」：{}",
                input.display(),
                io_error_text(&error)
            );
            return Err(ExitCode::FAILURE);
        }
    };

    let program = match li_parser::parse_source(&source) {
        Ok(program) => program,
        Err(errors) => {
            eprintln!("{errors}");
            return Err(ExitCode::FAILURE);
        }
    };

    match li_codegen_html::generate(&program) {
        Ok(html) => Ok(html),
        Err(errors) => {
            eprintln!("{errors}");
            Err(ExitCode::FAILURE)
        }
    }
}

/// 编译 `.lui` 视图并在内存中提供单体 HTML 服务。
///
/// 不写入任何 `.html` 文件，也不依赖外部静态资源目录：产物完全内嵌在
/// 进程内，HTTP 响应直接来自内存。
fn serve_view(input: &Path, port: u16) -> ExitCode {
    use std::io::{Read, Write};
    use std::net::TcpListener;

    let html = match compile_view_to_html(input) {
        Ok(html) => html,
        Err(code) => return code,
    };

    let listener = match TcpListener::bind(("127.0.0.1", port)) {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("错误：无法监听端口 {port}：{error}");
            return ExitCode::FAILURE;
        }
    };

    let address = match listener.local_addr() {
        Ok(address) => address,
        Err(error) => {
            eprintln!("错误：无法读取本地地址：{error}");
            return ExitCode::FAILURE;
        }
    };

    println!("服务已启动：http://{address}/");
    println!("按 Ctrl+C 停止服务。");

    for stream in listener.incoming() {
        let Ok(mut stream) = stream else {
            continue;
        };

        let html = html.clone();
        std::thread::spawn(move || {
            let mut buffer = [0_u8; 4096];
            let Ok(read) = stream.read(&mut buffer) else {
                return;
            };
            let request = String::from_utf8_lossy(&buffer[..read]);
            let path = request
                .lines()
                .next()
                .and_then(|line| line.split_whitespace().nth(1))
                .unwrap_or("/");

            let (status, content_type, body) = match path {
                "/" | "/index.html" => ("200 OK", "text/html; charset=utf-8", html),
                "/favicon.ico" => ("204 No Content", "text/plain; charset=utf-8", String::new()),
                _ => (
                    "404 Not Found",
                    "text/plain; charset=utf-8",
                    "未找到请求的资源".to_owned(),
                ),
            };

            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
        });
    }

    ExitCode::SUCCESS
}

/// 解析 `.lui` 视图文件并生成同名的静态 HTML 文件。
fn compile_to_html(input: &Path) -> ExitCode {
    if input.extension().and_then(|extension| extension.to_str()) != Some("lui") {
        eprintln!(
            "错误：命令「编译-html」需要 .lui 视图文件，实际为「{}」",
            input.display()
        );
        return ExitCode::FAILURE;
    }

    let html = match compile_view_to_html(input) {
        Ok(html) => html,
        Err(code) => return code,
    };

    let output = input.with_extension("html");

    if let Err(error) = fs::write(&output, html) {
        eprintln!(
            "错误：无法写入文件「{}」：{}",
            output.display(),
            io_error_text(&error)
        );
        return ExitCode::FAILURE;
    }

    println!("已生成 HTML：{}", output.display());
    ExitCode::SUCCESS
}

fn compile_to_bytecode(input: &Path) -> ExitCode {
    let ir = match compile_to_ir(input) {
        Ok(ir) => ir,
        Err(code) => return code,
    };

    let bytecode = li_codegen_bytecode::generate(&ir);

    if let Err(message) = bytecode.validate() {
        eprintln!("错误：生成的字节码校验失败：{message}");
        return ExitCode::FAILURE;
    }

    let text = li_codegen_bytecode::to_text(&bytecode);
    let output = input.with_extension("lic");

    if let Err(error) = fs::write(&output, text) {
        eprintln!(
            "错误：无法写入文件「{}」：{}",
            output.display(),
            io_error_text(&error)
        );
        return ExitCode::FAILURE;
    }

    println!("已生成字节码：{}", output.display());
    ExitCode::SUCCESS
}

fn execute_bytecode(input: &Path) -> ExitCode {
    match input.extension().and_then(|extension| extension.to_str()) {
        Some("lui") => {
            eprintln!(
                "错误：执行命令不能处理 .lui 视图文件，请使用「li 编译-html {}」",
                input.display()
            );
            return ExitCode::FAILURE;
        }
        Some("li") => {
            eprintln!(
                "错误：执行命令需要 .lic 文件，请先运行「li 编译 {}」",
                input.display()
            );
            return ExitCode::FAILURE;
        }
        _ => {}
    }

    let source = match fs::read_to_string(input) {
        Ok(source) => source,
        Err(error) => {
            eprintln!(
                "错误：无法读取文件「{}」：{}",
                input.display(),
                io_error_text(&error)
            );
            return ExitCode::FAILURE;
        }
    };

    let program = match li_codegen_bytecode::from_text(&source) {
        Ok(program) => program,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };

    match li_vm::execute(&program) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

/// 判断程序中是否包含视图声明；递归检查模块内部。
fn contains_view(items: &[li_ast::Item]) -> bool {
    items.iter().any(|item| match item {
        li_ast::Item::View(_) => true,
        li_ast::Item::Module(module) => contains_view(&module.items),
        li_ast::Item::Import(_) | li_ast::Item::Function(_) | li_ast::Item::Type(_) => false,
    })
}

/// 把常见的输入输出错误转换为中文说明。
fn io_error_text(error: &std::io::Error) -> &'static str {
    match error.kind() {
        ErrorKind::NotFound => "文件不存在",
        ErrorKind::PermissionDenied => "权限不足",
        ErrorKind::InvalidData => "文件内容不是有效的 UTF-8",
        ErrorKind::InvalidInput => "路径或参数无效",
        ErrorKind::AlreadyExists => "文件已存在",
        ErrorKind::WriteZero => "写入失败",
        ErrorKind::UnexpectedEof => "文件内容不完整",
        _ => "输入输出错误",
    }
}
