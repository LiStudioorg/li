//! li 编译器命令行入口。
//!
//! 当前仅输出版本信息；`li 解析 <file.li>` 与 `li 检查 <file.li>`
//! 将在里程碑 5 中接入。

fn main() {
    println!("li 编译器前端 v{}（开发中）", env!("CARGO_PKG_VERSION"));
}
