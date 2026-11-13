//! li 编译器命令行入口。
//!
//! 提供三条主要命令：
//! - `li 编译 <文件.li>`：编译为同名 `.lic` 字节码文件；
//! - `li 执行 <文件.lic>`：在栈式虚拟机中加载并执行字节码；
//! - `li 编译-js <文件.li>`：编译为同名 JavaScript 文件。
//!
//! 前端流程完全复用：语法分析 → 语义检查 → 中立 IR 降级 → 后端代码生成。

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
    println!("  li 版本                显示版本信息");
    println!("  li 帮助                显示帮助信息");
}

/// 依次执行读取、语法分析、语义检查与 IR 降级。
fn compile_to_ir(input: &Path) -> Result<li_ir::Program, ExitCode> {
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
    if input.extension().and_then(|extension| extension.to_str()) == Some("li") {
        eprintln!(
            "错误：执行命令需要 .lic 文件，请先运行「li 编译 {}」",
            input.display()
        );
        return ExitCode::FAILURE;
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
