//! li 语言的中立中间表示。
//!
//! 本 crate 负责把 `li-ast` 降级为与后端无关的 IR：
//! - 数据流使用三地址码表示；
//! - 控制流使用基本块与 CFG 终止指令表示；
//! - 每个值都保留静态类型，供 JS、字节码等后端消费。
//!
//! 入口函数是 [`lower`]：传入语法分析与语义分析通过的 AST，
//! 返回 [`Program`]。若降级阶段发现未知类型等问题，则返回
//! 包含中文诊断的 [`li_errors::DiagnosticBag`]。

pub mod ir;
mod lower;

pub use ir::{
    BasicBlock, BinaryOp, BuiltinFunction, BuiltinMethod, Constant, Function, Instruction, IrType,
    Label, Local, Operand, Parameter, Place, Program, StructDecl, StructField, TempId, Terminator,
    UnaryOp,
};
pub use lower::lower;
