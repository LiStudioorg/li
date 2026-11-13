//! `li-codegen-bytecode` 的代码生成与文本格式测试。

use li_codegen_bytecode::{
    from_text, generate, to_text, Constant, FunctionCode, Instruction, InstructionArg, Opcode,
    Program, BYTECODE_VERSION,
};
use li_errors::Span;

/// 解析、检查、降级并生成字节码。
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

#[test]
fn generates_fibonacci_with_constants_and_jumps() {
    let source = include_str!("../../li-parser/tests/fixtures/fibonacci.li");
    let program = compile(source);

    assert_eq!(program.version, BYTECODE_VERSION);
    assert_eq!(program.entry.as_deref(), Some("主"));
    assert_eq!(program.functions.len(), 2);
    assert!(program.constants.contains(&Constant::Integer(10)));

    let fibonacci = program.function("斐波那契").expect("缺少斐波那契函数");
    assert_eq!(fibonacci.arity, 1);
    assert!(fibonacci
        .code
        .iter()
        .any(|instruction| instruction.opcode == Opcode::Call));
    assert!(fibonacci
        .code
        .iter()
        .any(|instruction| instruction.opcode == Opcode::JumpIfFalse));

    for instruction in fibonacci.code.iter().chain(&program.entry_code) {
        if matches!(instruction.opcode, Opcode::Jump | Opcode::JumpIfFalse) {
            let target = instruction.integer_arg(0).expect("跳转指令缺少目标地址");
            assert!(
                target < fibonacci.code.len(),
                "跳转目标 {target} 越界：\n{}",
                to_text(&program)
            );
        }
    }

    let main = program.function("主").expect("缺少主函数");
    assert!(main
        .code
        .iter()
        .any(|instruction| instruction.opcode == Opcode::Print));
    assert!(main
        .code
        .iter()
        .any(|instruction| instruction.opcode == Opcode::Return));
}

#[test]
fn text_format_roundtrips_fibonacci() {
    let program = compile(include_str!("../../li-parser/tests/fixtures/fibonacci.li"));
    let text = to_text(&program);

    assert!(text.contains(".entry 主"), "缺少入口声明：\n{text}");
    assert!(
        text.contains(".function 斐波那契 1"),
        "缺少函数头：\n{text}"
    );
    assert!(text.contains(".slots 项数"), "缺少槽位表：\n{text}");
    assert!(text.contains("CALL 斐波那契 1"), "缺少调用指令：\n{text}");
    assert!(text.contains("JUMP_IF_FALSE"), "缺少条件跳转：\n{text}");
    assert!(text.contains(".entry_code"), "缺少入口代码：\n{text}");

    let parsed = from_text(&text).unwrap_or_else(|error| panic!("解析失败：{error}"));
    assert_eq!(to_text(&parsed), text, "文本往返后字节码文本发生变化");
    assert_eq!(parsed.functions.len(), program.functions.len());
    assert_eq!(parsed.constants, program.constants);
    assert_eq!(parsed.entry, program.entry);
}

#[test]
fn preserves_escaped_string_and_float_constants() {
    let program = Program {
        version: BYTECODE_VERSION,
        entry: None,
        constants: vec![
            Constant::String("第一行\n第二行「引号」\\反斜杠\t制表".to_owned()),
            Constant::Float(3.5),
            Constant::Bool(true),
            Constant::Null,
        ],
        functions: vec![FunctionCode {
            name: "主".to_owned(),
            arity: 0,
            slots: Vec::new(),
            code: vec![
                Instruction::new(
                    Opcode::PushConst,
                    vec![InstructionArg::Integer(0)],
                    Span::default(),
                ),
                Instruction::new(Opcode::Print, Vec::new(), Span::default()),
                Instruction::new(Opcode::ReturnVoid, Vec::new(), Span::default()),
            ],
        }],
        entry_code: vec![Instruction::new(Opcode::Halt, Vec::new(), Span::default())],
    };

    let text = to_text(&program);
    let parsed = from_text(&text).unwrap_or_else(|error| panic!("解析失败：{error}"));
    assert_eq!(parsed, program);
}

#[test]
fn rejects_out_of_range_jump_targets() {
    let program = Program {
        version: BYTECODE_VERSION,
        entry: None,
        constants: Vec::new(),
        functions: vec![FunctionCode {
            name: "主".to_owned(),
            arity: 0,
            slots: Vec::new(),
            code: vec![Instruction::new(
                Opcode::Jump,
                vec![InstructionArg::Integer(9)],
                Span::default(),
            )],
        }],
        entry_code: vec![Instruction::new(Opcode::Halt, Vec::new(), Span::default())],
    };

    let message = program.validate().expect_err("越界跳转必须被拒绝");
    assert!(
        message.contains("跳转目标 9 越界"),
        "错误信息不符合预期：{message}"
    );
}

#[test]
fn reports_text_address_gaps_with_line_and_column() {
    let text = "\
.version 1
.entry 无
.constants 0
.end
.entry_code
0001 HALT
.end
";
    let error = from_text(text).expect_err("不连续的指令地址必须被拒绝");

    assert_eq!(error.line(), 6);
    assert!(
        error.message().contains("指令地址应为 0"),
        "错误信息不符合预期：{error}"
    );
    assert!(
        error
            .to_string()
            .starts_with("第 6 行第 1 列：字节码错误："),
        "错误格式不符合预期：{error}"
    );
}
