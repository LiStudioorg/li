//! li 语言中立 IR 的字节码后端。
//!
//! 本 crate 只消费 [`li_ir::Program`]，不依赖前端 AST。它负责：
//! - 把三地址码与 CFG 翻译为精简的栈式字节码；
//! - 维护常量池与函数内局部槽位；
//! - 把基本块标签回填为函数内的指令地址；
//! - 将字节码序列化为便于调试的 `.lic` 文本，并支持重新解析与校验。
//!
//! 虚拟机 crate `li-vm` 直接消费本 crate 的 [`Program`] 类型。

mod codegen;
mod model;
mod text;

pub use model::{
    Constant, FunctionCode, Instruction, InstructionArg, Opcode, Program, BYTECODE_VERSION,
};
pub use text::{from_text, to_text, ParseError};

/// 把中立 IR 生成为字节码程序。
pub fn generate(program: &li_ir::Program) -> Program {
    codegen::generate(program)
}
