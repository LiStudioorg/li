//! li 编译器命令行入口。
//!
//! 当前提供 `li 编译-js <文件.li>`：依次执行语法分析、语义检查、
//! IR 降级与 JavaScript 代码生成，并在源文件旁边写入同名 `.js` 文件。

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
        "编译-js" => {
            if args.len() != 2 {
                eprintln!("错误：命令「编译-js」需要一个 .li 文件参数");
                print_usage();
                return ExitCode::FAILURE;
            }
            compile_to_javascript(Path::new(&args[1]))
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
    println!("  li 编译-js <文件.li>   将 li 源码编译为同名 .js 文件");
    println!("  li 版本                 显示版本信息");
    println!("  li 帮助                 显示帮助信息");
}

fn compile_to_javascript(input: &Path) -> ExitCode {
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

    let program = match li_parser::parse_source(&source) {
        Ok(program) => program,
        Err(errors) => {
            eprintln!("{errors}");
            return ExitCode::FAILURE;
        }
    };

    if let Err(errors) = li_sema::check(&program) {
        eprintln!("{errors}");
        return ExitCode::FAILURE;
    }

    let ir = match li_ir::lower(&program) {
        Ok(ir) => ir,
        Err(errors) => {
            eprintln!("{errors}");
            return ExitCode::FAILURE;
        }
    };

    let javascript = li_codegen_js::generate(&ir);
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
