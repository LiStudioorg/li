//! `.lic` 文本字节码的序列化与解析。
//!
//! 文本格式以行为单位，便于肉眼调试与手工检查：
//! - 常量池使用 `下标 类型 字面量` 的形式；
//! - 函数使用 `.function 函数名 参数个数`、`.slots` 与 `.code` 段；
//! - 指令行以四位指令地址开头，末尾可附 `@行:列` 的源码定位。
//!
//! 解析器会检查指令地址是否连续、参数类型是否匹配，并在解析完成后
//! 执行完整的结构校验，确保 VM 不会读到越界的常量或跳转目标。

use std::error::Error;
use std::fmt;

use li_errors::{Position, Span};

use crate::model::{Constant, FunctionCode, Instruction, InstructionArg, Opcode, Program};

/// `.lic` 文件解析错误。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError {
    line: usize,
    column: usize,
    message: String,
}

impl ParseError {
    /// 创建一条带行列定位的解析错误。
    pub fn new(line: usize, column: usize, message: impl Into<String>) -> Self {
        Self {
            line,
            column,
            message: message.into(),
        }
    }

    /// 返回错误所在行号。
    pub const fn line(&self) -> usize {
        self.line
    }

    /// 返回错误所在列号。
    pub const fn column(&self) -> usize {
        self.column
    }

    /// 返回中文错误说明。
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "第 {} 行第 {} 列：字节码错误：{}",
            self.line, self.column, self.message
        )
    }
}

impl Error for ParseError {}

/// 把字节码程序序列化为 `.lic` 文本。
pub fn to_text(program: &Program) -> String {
    let mut output = String::new();

    output.push_str(&format!("# li 字节码文件 v{}\n", program.version));
    output.push_str("# 本文件由 li 编译器自动生成，请勿手动编辑。\n");
    output.push_str("# 指令地址为函数内下标，参数、槽位与常量均从 0 开始编号。\n");
    output.push_str(&format!(".version {}\n", program.version));

    match &program.entry {
        Some(entry) => output.push_str(&format!(".entry {entry}\n")),
        None => output.push_str(".entry 无\n"),
    }

    output.push_str(&format!("\n.constants {}\n", program.constants.len()));
    for (index, constant) in program.constants.iter().enumerate() {
        output.push_str(&format!("{index} {}\n", format_constant(constant)));
    }
    output.push_str(".end\n");

    for function in &program.functions {
        output.push('\n');
        output.push_str(&format!(".function {} {}\n", function.name, function.arity));
        output.push_str(".slots");
        for slot in &function.slots {
            output.push(' ');
            output.push_str(slot);
        }
        output.push('\n');
        output.push_str(".code\n");
        for (address, instruction) in function.code.iter().enumerate() {
            output.push_str(&format_instruction(address, instruction));
            output.push('\n');
        }
        output.push_str(".end\n");
    }

    output.push_str("\n.entry_code\n");
    for (address, instruction) in program.entry_code.iter().enumerate() {
        output.push_str(&format_instruction(address, instruction));
        output.push('\n');
    }
    output.push_str(".end\n");

    output
}

fn format_instruction(address: usize, instruction: &Instruction) -> String {
    let mut text = format!("{address:04} {}", instruction.opcode);
    for argument in &instruction.args {
        text.push(' ');
        text.push_str(&argument.to_string());
    }
    text.push_str(&format!(
        " @{}:{}",
        instruction.span.start.line, instruction.span.start.column
    ));
    text
}

fn format_constant(constant: &Constant) -> String {
    match constant {
        Constant::Integer(value) => format!("整数 {value}"),
        Constant::Float(value) => format!("小数 {value:?}"),
        Constant::Bool(true) => "布尔 真".to_owned(),
        Constant::Bool(false) => "布尔 假".to_owned(),
        Constant::String(value) => format!("字符串 「{}」", escape_string(value)),
        Constant::Null => "空".to_owned(),
    }
}

fn escape_string(value: &str) -> String {
    let mut escaped = String::new();

    for character in value.chars() {
        match character {
            '\\' => escaped.push_str("\\\\"),
            '「' => escaped.push_str("\\「"),
            '」' => escaped.push_str("\\」"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            other if other.is_control() => {
                escaped.push_str(&format!("\\u{{{:X}}}", other as u32));
            }
            other => escaped.push(other),
        }
    }

    escaped
}

/// 解析 `.lic` 文本。
pub fn from_text(source: &str) -> Result<Program, ParseError> {
    let mut parser = Parser::new(source);
    let program = parser.parse()?;

    program
        .validate()
        .map_err(|message| ParseError::new(1, 1, format!("字节码校验失败：{message}")))?;

    Ok(program)
}

struct Parser<'a> {
    lines: Vec<&'a str>,
    index: usize,
}

impl<'a> Parser<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            lines: source.lines().collect(),
            index: 0,
        }
    }

    fn next_line(&mut self) -> Option<(usize, &'a str)> {
        while self.index < self.lines.len() {
            let number = self.index + 1;
            let raw = self.lines[self.index];
            self.index += 1;
            let trimmed = raw.trim();

            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            return Some((number, trimmed));
        }

        None
    }

    fn parse(&mut self) -> Result<Program, ParseError> {
        let mut version = None;
        let mut entry = None;
        let mut constants = None;
        let mut functions = Vec::new();
        let mut entry_code = None;

        while let Some((line_number, line)) = self.next_line() {
            if let Some(rest) = directive_rest(line, ".version") {
                if version.is_some() {
                    return Err(ParseError::new(line_number, 1, "重复的版本声明"));
                }
                let value = rest
                    .trim()
                    .parse::<u32>()
                    .map_err(|_| ParseError::new(line_number, 1, "版本号必须是非负整数"))?;
                version = Some(value);
            } else if line == ".entry_code" {
                if entry_code.is_some() {
                    return Err(ParseError::new(line_number, 1, "重复的入口代码段"));
                }
                entry_code = Some(self.parse_code_section(line_number)?);
            } else if let Some(rest) = directive_rest(line, ".entry") {
                if entry.is_some() {
                    return Err(ParseError::new(line_number, 1, "重复的程序入口声明"));
                }
                let name = rest.trim();
                if name.is_empty() {
                    return Err(ParseError::new(
                        line_number,
                        1,
                        "程序入口声明格式应为 .entry 函数名",
                    ));
                }
                entry = Some(if name == "无" {
                    None
                } else {
                    Some(name.to_owned())
                });
            } else if let Some(rest) = directive_rest(line, ".constants") {
                if constants.is_some() {
                    return Err(ParseError::new(line_number, 1, "重复的常量池段"));
                }
                let count = rest
                    .trim()
                    .parse::<usize>()
                    .map_err(|_| ParseError::new(line_number, 1, "常量池长度必须是非负整数"))?;
                let mut pool = Vec::with_capacity(count);

                for expected in 0..count {
                    let (constant_line, text) = self.next_line().ok_or_else(|| {
                        ParseError::new(line_number, 1, "常量池段在结束前提前到达文件末尾")
                    })?;
                    if text == ".end" {
                        return Err(ParseError::new(
                            constant_line,
                            1,
                            format!("常量池声明了 {count} 个常量，实际只有 {expected} 个"),
                        ));
                    }
                    pool.push(parse_constant_line(constant_line, text, expected)?);
                }

                let (end_line, end_text) = self
                    .next_line()
                    .ok_or_else(|| ParseError::new(line_number, 1, "常量池段缺少 .end"))?;
                if end_text != ".end" {
                    return Err(ParseError::new(
                        end_line,
                        1,
                        "常量池段声明结束后仍有多余内容",
                    ));
                }
                constants = Some(pool);
            } else if let Some(rest) = directive_rest(line, ".function") {
                let (name, arity) = parse_function_header(line_number, rest)?;
                functions.push(self.parse_function_body(line_number, name, arity)?);
            } else {
                return Err(ParseError::new(
                    line_number,
                    1,
                    format!("无法识别的顶层内容「{line}」"),
                ));
            }
        }

        let Some(version) = version else {
            return Err(ParseError::new(1, 1, "缺少 .version 版本声明"));
        };
        let Some(entry_code) = entry_code else {
            return Err(ParseError::new(1, 1, "缺少 .entry_code 入口代码段"));
        };

        Ok(Program {
            version,
            entry: entry.unwrap_or(None),
            constants: constants.unwrap_or_default(),
            functions,
            entry_code,
        })
    }

    fn parse_function_body(
        &mut self,
        header_line: usize,
        name: String,
        arity: usize,
    ) -> Result<FunctionCode, ParseError> {
        let mut slots = None;
        let mut code = None;

        loop {
            let (line_number, line) = self.next_line().ok_or_else(|| {
                ParseError::new(header_line, 1, format!("函数「{name}」缺少 .end"))
            })?;

            if line == ".end" {
                break;
            }

            if let Some(rest) = directive_rest(line, ".slots") {
                if slots.is_some() {
                    return Err(ParseError::new(line_number, 1, "重复的 .slots 段"));
                }
                slots = Some(
                    rest.split_whitespace()
                        .map(str::to_owned)
                        .collect::<Vec<_>>(),
                );
            } else if line == ".code" {
                if code.is_some() {
                    return Err(ParseError::new(line_number, 1, "重复的 .code 段"));
                }
                let instructions = self.parse_code_section(line_number)?;
                code = Some(instructions);
                break;
            } else {
                return Err(ParseError::new(
                    line_number,
                    1,
                    "函数体内只能出现 .slots 或 .code 段",
                ));
            }
        }

        let Some(code) = code else {
            return Err(ParseError::new(
                header_line,
                1,
                format!("函数「{name}」缺少 .code 段"),
            ));
        };

        Ok(FunctionCode {
            name,
            arity,
            slots: slots.unwrap_or_default(),
            code,
            is_extern: false,
        })
    }

    fn parse_code_section(&mut self, section_line: usize) -> Result<Vec<Instruction>, ParseError> {
        let mut code = Vec::new();

        loop {
            let (line_number, line) = self
                .next_line()
                .ok_or_else(|| ParseError::new(section_line, 1, "代码段缺少 .end"))?;

            if line == ".end" {
                break;
            }

            code.push(parse_instruction_line(line_number, line, code.len())?);
        }

        Ok(code)
    }
}

fn parse_function_header(line_number: usize, rest: &str) -> Result<(String, usize), ParseError> {
    let mut tokens = rest.split_whitespace();
    let name = tokens
        .next()
        .ok_or_else(|| ParseError::new(line_number, 1, "函数定义缺少函数名"))?;
    let arity = tokens
        .next()
        .ok_or_else(|| ParseError::new(line_number, 1, "函数定义缺少参数个数"))?;

    if tokens.next().is_some() {
        return Err(ParseError::new(
            line_number,
            1,
            "函数定义格式应为 .function 函数名 参数个数",
        ));
    }

    let arity = arity
        .parse::<usize>()
        .map_err(|_| ParseError::new(line_number, 1, "函数参数个数必须是非负整数"))?;

    Ok((name.to_owned(), arity))
}

fn parse_constant_line(
    line_number: usize,
    line: &str,
    expected_index: usize,
) -> Result<Constant, ParseError> {
    let (index_text, rest) = take_token(line);
    let index = index_text
        .parse::<usize>()
        .map_err(|_| ParseError::new(line_number, 1, "常量行必须以常量下标开头"))?;

    if index != expected_index {
        return Err(ParseError::new(
            line_number,
            1,
            format!("常量下标应为 {expected_index}，实际为 {index}"),
        ));
    }

    let (kind, rest) = take_token(rest);
    if kind.is_empty() {
        return Err(ParseError::new(line_number, 1, "常量行缺少常量类型"));
    }
    let payload = rest.trim();

    match kind {
        "整数" => payload
            .parse::<i64>()
            .map(Constant::Integer)
            .map_err(|_| ParseError::new(line_number, 1, format!("无效的整数常量「{payload}」"))),
        "小数" => payload
            .parse::<f64>()
            .map(Constant::Float)
            .map_err(|_| ParseError::new(line_number, 1, format!("无效的小数常量「{payload}」"))),
        "布尔" => match payload {
            "真" => Ok(Constant::Bool(true)),
            "假" => Ok(Constant::Bool(false)),
            _ => Err(ParseError::new(
                line_number,
                1,
                format!("布尔常量只能是「真」或「假」，实际为「{payload}」"),
            )),
        },
        "字符串" => parse_string_literal(line_number, payload).map(Constant::String),
        "空" => {
            if payload.is_empty() {
                Ok(Constant::Null)
            } else {
                Err(ParseError::new(line_number, 1, "空常量后面不能有额外内容"))
            }
        }
        other => Err(ParseError::new(
            line_number,
            1,
            format!("未知的常量类型「{other}」"),
        )),
    }
}

fn take_token(text: &str) -> (&str, &str) {
    let text = text.trim_start();

    match text.find(char::is_whitespace) {
        Some(index) => (&text[..index], &text[index..]),
        None => (text, ""),
    }
}

/// 匹配 `.directive` 或 `.directive 参数`，避免把 `.entry_code` 误认成 `.entry`。
fn directive_rest<'a>(line: &'a str, directive: &str) -> Option<&'a str> {
    let rest = line.strip_prefix(directive)?;
    if rest.is_empty() || rest.chars().next().is_some_and(char::is_whitespace) {
        Some(rest)
    } else {
        None
    }
}

fn parse_string_literal(line_number: usize, text: &str) -> Result<String, ParseError> {
    let mut characters = text.chars();

    match characters.next() {
        Some('「') => {}
        _ => {
            return Err(ParseError::new(
                line_number,
                1,
                "字符串常量必须使用「」定界",
            ));
        }
    }

    let mut value = String::new();

    loop {
        match characters.next() {
            Some('」') => {
                if characters.next().is_some() {
                    return Err(ParseError::new(
                        line_number,
                        1,
                        "字符串常量结束定界符后还有多余内容",
                    ));
                }
                return Ok(value);
            }
            Some('\\') => match characters.next() {
                Some('\\') => value.push('\\'),
                Some('「') => value.push('「'),
                Some('」') => value.push('」'),
                Some('n') => value.push('\n'),
                Some('r') => value.push('\r'),
                Some('t') => value.push('\t'),
                Some('u') => {
                    if characters.next() != Some('{') {
                        return Err(ParseError::new(
                            line_number,
                            1,
                            "统一码转义应写成 \\u{十六进制数}",
                        ));
                    }
                    let mut digits = String::new();
                    loop {
                        match characters.next() {
                            Some('}') => break,
                            Some(character) => digits.push(character),
                            None => {
                                return Err(ParseError::new(
                                    line_number,
                                    1,
                                    "字符串中的 Unicode 转义缺少右花括号",
                                ));
                            }
                        }
                    }
                    let code = u32::from_str_radix(&digits, 16)
                        .map_err(|_| ParseError::new(line_number, 1, "无效的 Unicode 转义"))?;
                    let character = char::from_u32(code)
                        .ok_or_else(|| ParseError::new(line_number, 1, "统一码转义不是合法字符"))?;
                    value.push(character);
                }
                Some(other) => {
                    return Err(ParseError::new(
                        line_number,
                        1,
                        format!("未知的字符串转义「\\{other}」"),
                    ));
                }
                None => {
                    return Err(ParseError::new(line_number, 1, "字符串常量以反斜杠结尾"));
                }
            },
            Some(character) => value.push(character),
            None => {
                return Err(ParseError::new(
                    line_number,
                    1,
                    "字符串常量缺少结束定界符「」",
                ));
            }
        }
    }
}

fn parse_instruction_line(
    line_number: usize,
    line: &str,
    expected_address: usize,
) -> Result<Instruction, ParseError> {
    let mut tokens = line.split_whitespace();

    let address_text = tokens
        .next()
        .ok_or_else(|| ParseError::new(line_number, 1, "指令行缺少指令地址"))?;
    let address = address_text
        .parse::<usize>()
        .map_err(|_| ParseError::new(line_number, 1, "指令地址必须是非负整数"))?;

    if address != expected_address {
        return Err(ParseError::new(
            line_number,
            1,
            format!("指令地址应为 {expected_address}，实际为 {address}"),
        ));
    }

    let mnemonic = tokens
        .next()
        .ok_or_else(|| ParseError::new(line_number, 1, "指令行缺少操作码"))?;
    let opcode = Opcode::from_mnemonic(mnemonic)
        .ok_or_else(|| ParseError::new(line_number, 1, format!("未知的操作码「{mnemonic}」")))?;

    let mut args = Vec::new();
    let mut span = Span::default();
    let mut has_span = false;

    for token in tokens {
        if let Some(annotation) = token.strip_prefix('@') {
            if has_span {
                return Err(ParseError::new(
                    line_number,
                    1,
                    "一条指令只能有一个源码定位",
                ));
            }
            span = parse_span_annotation(line_number, annotation)?;
            has_span = true;
        } else {
            if has_span {
                return Err(ParseError::new(line_number, 1, "源码定位必须写在指令末尾"));
            }

            if let Ok(value) = token.parse::<usize>() {
                args.push(InstructionArg::Integer(value));
            } else if token.is_empty() {
                return Err(ParseError::new(line_number, 1, "指令参数不能为空"));
            } else {
                args.push(InstructionArg::Name(token.to_owned()));
            }
        }
    }

    let instruction = Instruction::new(opcode, args, span);
    instruction
        .validate()
        .map_err(|message| ParseError::new(line_number, 1, message))?;

    Ok(instruction)
}

fn parse_span_annotation(line_number: usize, text: &str) -> Result<Span, ParseError> {
    let (line, column) = text
        .split_once(':')
        .ok_or_else(|| ParseError::new(line_number, 1, "源码定位应写成 @行:列"))?;

    let line = line
        .parse::<usize>()
        .map_err(|_| ParseError::new(line_number, 1, "源码定位的行号必须是从 1 开始的整数"))?;
    let column = column
        .parse::<usize>()
        .map_err(|_| ParseError::new(line_number, 1, "源码定位的列号必须是从 1 开始的整数"))?;

    if line == 0 || column == 0 {
        return Err(ParseError::new(
            line_number,
            1,
            "源码定位的行号与列号都必须从 1 开始",
        ));
    }

    let position = Position::new(line, column, 0);
    Ok(Span::point(position))
}
