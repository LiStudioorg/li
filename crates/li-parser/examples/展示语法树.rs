//! 打印 li 源文件的抽象语法树，便于检查新语法的解析结果。
//!
//! 用法：`cargo run -p li-parser --example 展示语法树 -- <文件.li>`

use std::env;
use std::fs;
use std::process::ExitCode;

fn main() -> ExitCode {
    let Some(path) = env::args().nth(1) else {
        eprintln!("用法：cargo run -p li-parser --example 展示语法树 -- <文件.li>");
        return ExitCode::FAILURE;
    };

    let source = match fs::read_to_string(&path) {
        Ok(source) => source,
        Err(error) => {
            eprintln!("错误：无法读取文件「{path}」：{error}");
            return ExitCode::FAILURE;
        }
    };

    match li_parser::parse_source(&source) {
        Ok(program) => {
            println!("{program:#?}");
            ExitCode::SUCCESS
        }
        Err(errors) => {
            eprintln!("{errors}");
            ExitCode::FAILURE
        }
    }
}
