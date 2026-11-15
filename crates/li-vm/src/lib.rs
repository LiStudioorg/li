//! li 语言字节码栈式虚拟机。
//!
//! 虚拟机维护操作数栈、局部变量表与显式调用栈，用
//! Fetch-Decode-Execute 循环逐条执行 `.lic` 字节码：
//! - 每个函数拥有独立的操作数栈与局部槽位；
//! - `CALL` 创建新调用帧，`RETURN` 把结果送回调用者；
//! - `PRINT` 输出到控制台，`INPUT` 读取标准输入；
//! - 运行时错误附带 li 源码的行列号、函数名与指令地址。

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::rc::Rc;

use li_codegen_bytecode::{BuiltinFunction, BuiltinMethod, Constant, Instruction, Opcode, Program};
use li_errors::Span;

/// 最大调用深度，避免无限递归耗尽内存。
const MAX_CALL_DEPTH: usize = 10_000;

/// 结构体运行期值。
#[derive(Clone, Debug, PartialEq)]
pub struct StructValue {
    /// 类型名称。
    pub type_name: String,
    /// 字段表。
    pub fields: HashMap<String, Value>,
}

/// 字典键。第一版字典只允许可哈希的标量键。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum DictionaryKey {
    /// 整数键。
    Integer(i64),
    /// 小数键；使用位模式保证哈希与相等一致。
    Float(u64),
    /// 布尔键。
    Bool(bool),
    /// 字符串键。
    String(String),
}

impl DictionaryKey {
    /// 从运行期值构造字典键；非标量值返回 `None`。
    fn from_value(value: &Value) -> Option<Self> {
        match value {
            Value::Integer(value) => Some(Self::Integer(*value)),
            Value::Float(value) => {
                // 统一正零与负零，避免键语义出现意外差异。
                let bits = if *value == 0.0 {
                    0.0_f64.to_bits()
                } else {
                    value.to_bits()
                };
                Some(Self::Float(bits))
            }
            Value::Bool(value) => Some(Self::Bool(*value)),
            Value::String(value) => Some(Self::String(value.clone())),
            _ => None,
        }
    }

    /// 返回键的显示文本。
    fn display_text(&self) -> String {
        match self {
            Self::Integer(value) => value.to_string(),
            Self::Float(bits) => f64::from_bits(*bits).to_string(),
            Self::Bool(true) => "真".to_owned(),
            Self::Bool(false) => "假".to_owned(),
            Self::String(value) => value.clone(),
        }
    }
}

/// 字典运行期值，按插入顺序保存键值对。
#[derive(Clone, Debug, PartialEq)]
pub struct DictionaryValue {
    /// 键值表。
    entries: HashMap<DictionaryKey, Value>,
    /// 插入顺序。
    order: Vec<DictionaryKey>,
}

impl DictionaryValue {
    /// 创建空字典。
    fn new() -> Self {
        Self {
            entries: HashMap::new(),
            order: Vec::new(),
        }
    }

    /// 返回键值对数量。
    fn len(&self) -> usize {
        self.entries.len()
    }

    /// 插入键值对；已存在的键只更新值，不改变顺序。
    fn insert(&mut self, key: DictionaryKey, value: Value) {
        if !self.entries.contains_key(&key) {
            self.order.push(key.clone());
        }
        self.entries.insert(key, value);
    }

    /// 按键读取值。
    fn get(&self, key: &DictionaryKey) -> Option<&Value> {
        self.entries.get(key)
    }

    /// 按键删除键值对。
    fn remove(&mut self, key: &DictionaryKey) -> bool {
        if self.entries.remove(key).is_none() {
            return false;
        }
        self.order.retain(|existing| existing != key);
        true
    }

    /// 是否包含指定键。
    fn contains_key(&self, key: &DictionaryKey) -> bool {
        self.entries.contains_key(key)
    }
}

/// 虚拟机运行期值。
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    /// 整数。
    Integer(i64),
    /// 小数。
    Float(f64),
    /// 布尔值。
    Bool(bool),
    /// 字符串。
    String(String),
    /// 空值。
    Null,
    /// 列表。使用共享可变容器，保持与引用语义一致。
    List(Rc<RefCell<Vec<Value>>>),
    /// 字典。使用共享可变容器，保持与引用语义一致。
    Dictionary(Rc<RefCell<DictionaryValue>>),
    /// 结构体。使用共享可变容器。
    Struct(Rc<RefCell<StructValue>>),
    /// 可选值。
    Optional(Option<Box<Value>>),
    /// 结果值。
    ResultValue(Result<Box<Value>, Box<Value>>),
}

impl Value {
    /// 返回值的中文类型名称。
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Integer(_) => "整数",
            Self::Float(_) => "小数",
            Self::Bool(_) => "布尔",
            Self::String(_) => "字符串",
            Self::Null => "空",
            Self::List(_) => "列表",
            Self::Dictionary(_) => "字典",
            Self::Struct(_) => "结构体",
            Self::Optional(_) => "可选值",
            Self::ResultValue(_) => "结果值",
        }
    }

    /// 结构化相等比较，供 `EQ` 指令使用。
    pub fn equals(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Integer(left), Self::Integer(right)) => left == right,
            (Self::Float(left), Self::Float(right)) => left == right,
            (Self::Bool(left), Self::Bool(right)) => left == right,
            (Self::String(left), Self::String(right)) => left == right,
            (Self::Null, Self::Null) => true,
            (Self::List(left), Self::List(right)) => {
                if Rc::ptr_eq(left, right) {
                    return true;
                }
                let left = left.borrow();
                let right = right.borrow();
                left.len() == right.len()
                    && left
                        .iter()
                        .zip(right.iter())
                        .all(|(left, right)| left.equals(right))
            }
            (Self::Dictionary(left), Self::Dictionary(right)) => {
                if Rc::ptr_eq(left, right) {
                    return true;
                }
                let left = left.borrow();
                let right = right.borrow();
                left.len() == right.len()
                    && left.entries.iter().all(|(key, value)| {
                        right
                            .entries
                            .get(key)
                            .is_some_and(|other| value.equals(other))
                    })
            }
            (Self::Struct(left), Self::Struct(right)) => {
                if Rc::ptr_eq(left, right) {
                    return true;
                }
                let left = left.borrow();
                let right = right.borrow();
                left.type_name == right.type_name
                    && left.fields.len() == right.fields.len()
                    && left.fields.iter().all(|(name, value)| {
                        right
                            .fields
                            .get(name)
                            .is_some_and(|other| value.equals(other))
                    })
            }
            (Self::Optional(left), Self::Optional(right)) => match (left, right) {
                (Some(left), Some(right)) => left.equals(right),
                (None, None) => true,
                _ => false,
            },
            (Self::ResultValue(left), Self::ResultValue(right)) => match (left, right) {
                (Ok(left), Ok(right)) => left.equals(right),
                (Err(left), Err(right)) => left.equals(right),
                _ => false,
            },
            _ => false,
        }
    }

    fn to_display_text(&self) -> String {
        match self {
            Self::Integer(value) => value.to_string(),
            Self::Float(value) => value.to_string(),
            Self::Bool(true) => "真".to_owned(),
            Self::Bool(false) => "假".to_owned(),
            Self::String(value) => value.clone(),
            Self::Null => "空".to_owned(),
            Self::List(values) => {
                let values = values.borrow();
                let items = values
                    .iter()
                    .map(Self::to_display_text)
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("[{items}]")
            }
            Self::Dictionary(value) => {
                let value = value.borrow();
                let items = value
                    .order
                    .iter()
                    .filter_map(|key| {
                        value.entries.get(key).map(|item| {
                            format!("{}: {}", key.display_text(), item.to_display_text())
                        })
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{{{items}}}")
            }
            Self::Struct(value) => {
                let value = value.borrow();
                let mut fields = value.fields.iter().collect::<Vec<_>>();
                fields.sort_by(|left, right| left.0.cmp(right.0));
                let fields = fields
                    .into_iter()
                    .map(|(name, value)| format!("{name}: {}", value.to_display_text()))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{} {{ {fields} }}", value.type_name)
            }
            Self::Optional(Some(value)) => format!("成功({})", value.to_display_text()),
            Self::Optional(None) => "空".to_owned(),
            Self::ResultValue(Ok(value)) => format!("成功({})", value.to_display_text()),
            Self::ResultValue(Err(value)) => format!("失败({})", value.to_display_text()),
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_display_text())
    }
}

/// 运行时错误。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeError {
    message: String,
    span: Span,
    function: String,
    instruction: usize,
}

impl RuntimeError {
    /// 创建一条运行时错误。
    pub fn new(
        message: impl Into<String>,
        span: Span,
        function: impl Into<String>,
        instruction: usize,
    ) -> Self {
        Self {
            message: message.into(),
            span,
            function: function.into(),
            instruction,
        }
    }

    /// 返回中文错误说明。
    pub fn message(&self) -> &str {
        &self.message
    }

    /// 返回对应的 li 源码区间。
    pub const fn span(&self) -> Span {
        self.span
    }

    /// 返回发生错误的函数名。
    pub fn function(&self) -> &str {
        &self.function
    }

    /// 返回发生错误的指令地址。
    pub const fn instruction(&self) -> usize {
        self.instruction
    }
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}：运行时错误：{}；函数「{}」，指令 {}",
            self.span.start, self.message, self.function, self.instruction
        )
    }
}

impl std::error::Error for RuntimeError {}

/// 调用帧。
struct Frame {
    code: CodeRef,
    ip: usize,
    slots: Vec<Value>,
    stack: Vec<Value>,
}

/// 当前帧执行的是入口代码还是某个函数。
#[derive(Clone, Copy)]
enum CodeRef {
    Entry,
    Function(usize),
}

/// 栈式虚拟机。
pub struct Vm<'a> {
    program: &'a Program,
    frames: Vec<Frame>,
    input: VecDeque<String>,
    output: String,
    use_stdio: bool,
    current_span: Span,
    current_ip: usize,
}

impl<'a> Vm<'a> {
    /// 创建使用标准输入输出的虚拟机。
    pub fn new(program: &'a Program) -> Self {
        Self {
            program,
            frames: Vec::new(),
            input: VecDeque::new(),
            output: String::new(),
            use_stdio: true,
            current_span: Span::default(),
            current_ip: 0,
        }
    }

    /// 创建使用预置输入、并捕获输出到内存的虚拟机，便于测试。
    pub fn with_input(program: &'a Program, input: impl IntoIterator<Item = String>) -> Self {
        Self {
            program,
            frames: Vec::new(),
            input: input.into_iter().collect(),
            output: String::new(),
            use_stdio: false,
            current_span: Span::default(),
            current_ip: 0,
        }
    }

    /// 返回已经捕获的全部输出。
    pub fn output(&self) -> &str {
        &self.output
    }

    /// 执行程序，直到 `HALT` 或入口代码结束。
    pub fn run(&mut self) -> Result<(), RuntimeError> {
        if let Err(message) = self.program.validate() {
            return Err(RuntimeError::new(
                format!("字节码校验失败：{message}"),
                Span::default(),
                "程序",
                0,
            ));
        }

        if self.frames.is_empty() {
            self.frames.push(Frame {
                code: CodeRef::Entry,
                ip: 0,
                slots: Vec::new(),
                stack: Vec::new(),
            });
        }

        loop {
            let Some(frame) = self.frames.last() else {
                return Ok(());
            };
            let code = frame.code;
            let ip = frame.ip;
            self.current_ip = ip;

            let instruction = self.instruction(code, ip).cloned();
            let Some(instruction) = instruction else {
                let message = format!("程序计数器越界：指令地址 {ip} 不存在");
                return Err(self.error(message));
            };
            self.current_span = instruction.span;

            match instruction.opcode {
                Opcode::PushConst => {
                    let index = instruction.integer_arg(0).unwrap_or(usize::MAX);
                    let Some(constant) = self.program.constants.get(index) else {
                        return Err(self.error(format!("常量下标 {index} 越界")));
                    };
                    let value = value_from_constant(constant);
                    self.push(value);
                    self.advance(ip + 1);
                }
                Opcode::Load => {
                    let index = instruction.integer_arg(0).unwrap_or(usize::MAX);
                    let value = self.load_slot(index)?;
                    self.push(value);
                    self.advance(ip + 1);
                }
                Opcode::Store => {
                    let index = instruction.integer_arg(0).unwrap_or(usize::MAX);
                    let value = self.pop()?;
                    self.store_slot(index, value)?;
                    self.advance(ip + 1);
                }
                Opcode::Pop => {
                    self.pop()?;
                    self.advance(ip + 1);
                }
                Opcode::Add | Opcode::Subtract | Opcode::Multiply | Opcode::Divide => {
                    let right = self.pop_number()?;
                    let left = self.pop_number()?;
                    let value = self.numeric_operation(instruction.opcode, left, right)?;
                    self.push(value);
                    self.advance(ip + 1);
                }
                Opcode::Negate => {
                    let value = match self.pop_number()? {
                        Number::Integer(value) => Value::Integer(
                            value
                                .checked_neg()
                                .ok_or_else(|| self.error("整数取负溢出".to_owned()))?,
                        ),
                        Number::Float(value) => Value::Float(-value),
                    };
                    self.push(value);
                    self.advance(ip + 1);
                }
                Opcode::Not => {
                    let value = self.pop_bool()?;
                    self.push(Value::Bool(!value));
                    self.advance(ip + 1);
                }
                Opcode::Less | Opcode::Greater => {
                    let right = self.pop_number()?;
                    let left = self.pop_number()?;
                    let value = match (left, right) {
                        (Number::Integer(left), Number::Integer(right)) => {
                            if instruction.opcode == Opcode::Less {
                                left < right
                            } else {
                                left > right
                            }
                        }
                        (Number::Float(left), Number::Float(right)) => {
                            if instruction.opcode == Opcode::Less {
                                left < right
                            } else {
                                left > right
                            }
                        }
                        (left, right) => {
                            return Err(self.error(format!(
                                "比较运算两侧类型必须一致，实际为「{}」与「{}」",
                                left.type_name(),
                                right.type_name()
                            )));
                        }
                    };
                    self.push(Value::Bool(value));
                    self.advance(ip + 1);
                }
                Opcode::Equal => {
                    let right = self.pop()?;
                    let left = self.pop()?;
                    self.push(Value::Bool(left.equals(&right)));
                    self.advance(ip + 1);
                }
                Opcode::And | Opcode::Or => {
                    let right = self.pop_bool()?;
                    let left = self.pop_bool()?;
                    let value = if instruction.opcode == Opcode::And {
                        left && right
                    } else {
                        left || right
                    };
                    self.push(Value::Bool(value));
                    self.advance(ip + 1);
                }
                Opcode::Jump => {
                    let target = instruction.integer_arg(0).unwrap_or(usize::MAX);
                    self.jump(code, target)?;
                }
                Opcode::JumpIfFalse => {
                    let target = instruction.integer_arg(0).unwrap_or(usize::MAX);
                    let condition = self.pop_bool()?;
                    if condition {
                        self.advance(ip + 1);
                    } else {
                        self.jump(code, target)?;
                    }
                }
                Opcode::Call => {
                    let name = instruction.name_arg(0).unwrap_or("").to_owned();
                    let argc = instruction.integer_arg(1).unwrap_or(usize::MAX);
                    let arguments = self.pop_arguments(argc)?;
                    self.advance(ip + 1);
                    self.push_function_frame(&name, arguments)?;
                }
                Opcode::InvokeBuiltin => {
                    let name = instruction.name_arg(0).unwrap_or("").to_owned();
                    let argc = instruction.integer_arg(1).unwrap_or(usize::MAX);
                    let value = self.invoke_builtin(&name, argc)?;
                    self.push(value);
                    self.advance(ip + 1);
                }
                Opcode::Return => {
                    let value = self.pop()?;
                    if !self.return_from_frame(value) {
                        return Ok(());
                    }
                }
                Opcode::ReturnVoid => {
                    if !self.return_from_frame(Value::Null) {
                        return Ok(());
                    }
                }
                Opcode::Halt => return Ok(()),
                Opcode::Print => {
                    let value = self.pop()?;
                    let text = value.to_display_text();
                    self.output.push_str(&text);
                    self.output.push('\n');
                    if self.use_stdio {
                        println!("{text}");
                    }
                    self.advance(ip + 1);
                }
                Opcode::Input => {
                    let value = self.read_input()?;
                    self.push(value);
                    self.advance(ip + 1);
                }
                Opcode::MakeList => {
                    let count = instruction.integer_arg(0).unwrap_or(usize::MAX);
                    let mut values = self.pop_arguments(count)?;
                    self.push(Value::List(Rc::new(RefCell::new(std::mem::take(
                        &mut values,
                    )))));
                    self.advance(ip + 1);
                }
                Opcode::GetField => {
                    let field = instruction.name_arg(0).unwrap_or("").to_owned();
                    let object = self.pop()?;
                    let value = self.get_field(&object, &field)?;
                    self.push(value);
                    self.advance(ip + 1);
                }
                Opcode::SetField => {
                    let field = instruction.name_arg(0).unwrap_or("").to_owned();
                    let value = self.pop()?;
                    let object = self.pop()?;
                    self.set_field(&object, &field, value)?;
                    self.advance(ip + 1);
                }
                Opcode::GetIndex => {
                    let index = self.pop_integer()?;
                    let object = self.pop()?;
                    let value = self.get_index(&object, index)?;
                    self.push(value);
                    self.advance(ip + 1);
                }
                Opcode::SetIndex => {
                    let value = self.pop()?;
                    let index = self.pop_integer()?;
                    let object = self.pop()?;
                    self.set_index(&object, index, value)?;
                    self.advance(ip + 1);
                }
                Opcode::WrapOptional => {
                    let has_value = instruction.integer_arg(0).unwrap_or(usize::MAX);
                    let value = if has_value == 1 {
                        Value::Optional(Some(Box::new(self.pop()?)))
                    } else {
                        Value::Optional(None)
                    };
                    self.push(value);
                    self.advance(ip + 1);
                }
                Opcode::WrapResult => {
                    let value = self.pop()?;
                    self.push(Value::ResultValue(Ok(Box::new(value))));
                    self.advance(ip + 1);
                }
                Opcode::IsSuccess => {
                    let value = self.pop()?;
                    let success = match value {
                        Value::Optional(value) => value.is_some(),
                        Value::ResultValue(value) => value.is_ok(),
                        other => {
                            return Err(self.error(format!(
                                "只能判断可选值或结果值是否成功，实际为「{}」",
                                other.type_name()
                            )));
                        }
                    };
                    self.push(Value::Bool(success));
                    self.advance(ip + 1);
                }
                Opcode::Unwrap => {
                    let value = self.pop()?;
                    let unwrapped = match value {
                        Value::Optional(Some(value)) => *value,
                        Value::Optional(None) => {
                            return Err(self.error("尝试解包空的可选值".to_owned()));
                        }
                        Value::ResultValue(Ok(value)) => *value,
                        Value::ResultValue(Err(_)) => {
                            return Err(self.error("尝试解包失败的结果值".to_owned()));
                        }
                        other => {
                            return Err(self.error(format!(
                                "只能解包可选值或结果值，实际为「{}」",
                                other.type_name()
                            )));
                        }
                    };
                    self.push(unwrapped);
                    self.advance(ip + 1);
                }
                Opcode::UnwrapError => {
                    let value = self.pop()?;
                    let error = match value {
                        Value::ResultValue(Err(error)) => *error,
                        Value::ResultValue(Ok(_)) => {
                            return Err(self.error("尝试读取成功结果的失败值".to_owned()));
                        }
                        other => {
                            return Err(self.error(format!(
                                "只能从结果值读取失败值，实际为「{}」",
                                other.type_name()
                            )));
                        }
                    };
                    self.push(error);
                    self.advance(ip + 1);
                }
                Opcode::Nop => self.advance(ip + 1),
            }
        }
    }

    fn instruction(&self, code: CodeRef, ip: usize) -> Option<&Instruction> {
        match code {
            CodeRef::Entry => self.program.entry_code.get(ip),
            CodeRef::Function(index) => self
                .program
                .functions
                .get(index)
                .and_then(|function| function.code.get(ip)),
        }
    }

    fn current_code_len(&self, code: CodeRef) -> usize {
        match code {
            CodeRef::Entry => self.program.entry_code.len(),
            CodeRef::Function(index) => self
                .program
                .functions
                .get(index)
                .map_or(0, |function| function.code.len()),
        }
    }

    fn current_function_name(&self) -> String {
        match self.frames.last().map(|frame| frame.code) {
            Some(CodeRef::Entry) => "程序入口".to_owned(),
            Some(CodeRef::Function(index)) => self
                .program
                .functions
                .get(index)
                .map_or_else(|| "未知函数".to_owned(), |function| function.name.clone()),
            None => "程序".to_owned(),
        }
    }

    fn error(&self, message: String) -> RuntimeError {
        RuntimeError::new(
            message,
            self.current_span,
            self.current_function_name(),
            self.current_ip,
        )
    }

    fn advance(&mut self, ip: usize) {
        if let Some(frame) = self.frames.last_mut() {
            frame.ip = ip;
        }
    }

    fn jump(&mut self, code: CodeRef, target: usize) -> Result<(), RuntimeError> {
        if target >= self.current_code_len(code) {
            return Err(self.error(format!(
                "跳转目标 {target} 越界，指令序列长度为 {}",
                self.current_code_len(code)
            )));
        }
        self.advance(target);
        Ok(())
    }

    fn push(&mut self, value: Value) {
        if let Some(frame) = self.frames.last_mut() {
            frame.stack.push(value);
        }
    }

    fn pop(&mut self) -> Result<Value, RuntimeError> {
        let value = self.frames.last_mut().and_then(|frame| frame.stack.pop());

        value.ok_or_else(|| self.error("操作数栈不足，字节码可能已损坏".to_owned()))
    }

    fn pop_arguments(&mut self, count: usize) -> Result<Vec<Value>, RuntimeError> {
        let mut values = Vec::with_capacity(count);

        for _ in 0..count {
            values.push(self.pop()?);
        }
        values.reverse();
        Ok(values)
    }

    fn pop_integer(&mut self) -> Result<i64, RuntimeError> {
        match self.pop()? {
            Value::Integer(value) => Ok(value),
            other => Err(self.error(format!("期望整数，实际为「{}」", other.type_name()))),
        }
    }

    fn pop_bool(&mut self) -> Result<bool, RuntimeError> {
        match self.pop()? {
            Value::Bool(value) => Ok(value),
            other => Err(self.error(format!("期望布尔值，实际为「{}」", other.type_name()))),
        }
    }

    fn pop_number(&mut self) -> Result<Number, RuntimeError> {
        match self.pop()? {
            Value::Integer(value) => Ok(Number::Integer(value)),
            Value::Float(value) => Ok(Number::Float(value)),
            other => Err(self.error(format!("期望数字，实际为「{}」", other.type_name()))),
        }
    }

    fn numeric_operation(
        &self,
        opcode: Opcode,
        left: Number,
        right: Number,
    ) -> Result<Value, RuntimeError> {
        match (left, right) {
            (Number::Integer(left), Number::Integer(right)) => {
                let value = match opcode {
                    Opcode::Add => left
                        .checked_add(right)
                        .ok_or_else(|| self.error("整数加法溢出".to_owned()))?,
                    Opcode::Subtract => left
                        .checked_sub(right)
                        .ok_or_else(|| self.error("整数减法溢出".to_owned()))?,
                    Opcode::Multiply => left
                        .checked_mul(right)
                        .ok_or_else(|| self.error("整数乘法溢出".to_owned()))?,
                    Opcode::Divide => {
                        if right == 0 {
                            return Err(self.error("整数除数不能为零".to_owned()));
                        }
                        left.checked_div(right)
                            .ok_or_else(|| self.error("整数除法溢出".to_owned()))?
                    }
                    _ => return Err(self.error("内部错误：非算术指令进入了算术运算".to_owned())),
                };
                Ok(Value::Integer(value))
            }
            (Number::Float(left), Number::Float(right)) => {
                let value = match opcode {
                    Opcode::Add => left + right,
                    Opcode::Subtract => left - right,
                    Opcode::Multiply => left * right,
                    Opcode::Divide => left / right,
                    _ => return Err(self.error("内部错误：非算术指令进入了算术运算".to_owned())),
                };
                Ok(Value::Float(value))
            }
            (left, right) => Err(self.error(format!(
                "算术运算两侧类型必须一致，实际为「{}」与「{}」",
                left.type_name(),
                right.type_name()
            ))),
        }
    }

    fn load_slot(&self, index: usize) -> Result<Value, RuntimeError> {
        let value = self
            .frames
            .last()
            .and_then(|frame| frame.slots.get(index))
            .cloned();

        value.ok_or_else(|| self.error(format!("局部槽位 {index} 越界")))
    }

    fn store_slot(&mut self, index: usize, value: Value) -> Result<(), RuntimeError> {
        let slot_count = self.frames.last().map_or(0, |frame| frame.slots.len());
        if index >= slot_count {
            return Err(self.error(format!("局部槽位 {index} 越界")));
        }

        if let Some(frame) = self.frames.last_mut() {
            frame.slots[index] = value;
        }
        Ok(())
    }

    fn push_function_frame(
        &mut self,
        name: &str,
        arguments: Vec<Value>,
    ) -> Result<(), RuntimeError> {
        let function_index = self
            .program
            .functions
            .iter()
            .position(|function| function.name == name);
        let Some(function_index) = function_index else {
            return Err(self.error(format!("调用了未定义的函数「{name}」")));
        };

        let function = &self.program.functions[function_index];
        if arguments.len() != function.arity {
            return Err(self.error(format!(
                "调用函数「{name}」需要 {} 个实参，实际传入 {} 个",
                function.arity,
                arguments.len()
            )));
        }

        if self.frames.len() >= MAX_CALL_DEPTH {
            return Err(self.error(format!(
                "调用深度超过上限 {MAX_CALL_DEPTH}，可能存在无限递归"
            )));
        }

        let mut slots = vec![Value::Null; function.slots.len()];
        for (index, argument) in arguments.into_iter().enumerate() {
            if let Some(slot) = slots.get_mut(index) {
                *slot = argument;
            }
        }

        self.frames.push(Frame {
            code: CodeRef::Function(function_index),
            ip: 0,
            slots,
            stack: Vec::new(),
        });

        Ok(())
    }

    /// 弹出当前帧，并把返回值交给调用者。返回 `false` 表示调用栈已空。
    fn return_from_frame(&mut self, value: Value) -> bool {
        self.frames.pop();

        match self.frames.last_mut() {
            Some(caller) => {
                caller.stack.push(value);
                true
            }
            None => false,
        }
    }

    fn get_field(&self, object: &Value, field: &str) -> Result<Value, RuntimeError> {
        match object {
            Value::Struct(value) => value
                .borrow()
                .fields
                .get(field)
                .cloned()
                .ok_or_else(|| self.error(format!("结构体没有字段「{field}」"))),
            other => Err(self.error(format!(
                "不能访问字段：类型「{}」不是结构体",
                other.type_name()
            ))),
        }
    }

    fn set_field(&self, object: &Value, field: &str, value: Value) -> Result<(), RuntimeError> {
        match object {
            Value::Struct(structure) => {
                structure
                    .borrow_mut()
                    .fields
                    .insert(field.to_owned(), value);
                Ok(())
            }
            other => Err(self.error(format!(
                "不能写入字段：类型「{}」不是结构体",
                other.type_name()
            ))),
        }
    }

    fn get_index(&self, object: &Value, index: i64) -> Result<Value, RuntimeError> {
        let Value::List(values) = object else {
            return Err(self.error(format!(
                "不能读取下标：类型「{}」不是列表",
                object.type_name()
            )));
        };

        if index < 0 {
            return Err(self.error("列表下标不能为负数".to_owned()));
        }
        let index =
            usize::try_from(index).map_err(|_| self.error("列表下标超出平台范围".to_owned()))?;

        values
            .borrow()
            .get(index)
            .cloned()
            .ok_or_else(|| self.error(format!("列表下标 {index} 越界")))
    }

    fn set_index(&self, object: &Value, index: i64, value: Value) -> Result<(), RuntimeError> {
        let Value::List(values) = object else {
            return Err(self.error(format!(
                "不能写入下标：类型「{}」不是列表",
                object.type_name()
            )));
        };

        if index < 0 {
            return Err(self.error("列表下标不能为负数".to_owned()));
        }
        let index =
            usize::try_from(index).map_err(|_| self.error("列表下标超出平台范围".to_owned()))?;

        let mut values = values.borrow_mut();
        if index >= values.len() {
            return Err(self.error(format!(
                "列表下标 {index} 越界，列表长度为 {}",
                values.len()
            )));
        }
        values[index] = value;
        Ok(())
    }

    /// 执行 `INVOKE_BUILTIN` 指令。
    ///
    /// 带点号的名称表示内置方法，操作数栈上依次是接收者与实参；
    /// 不带点号的名称表示全局内置函数，只有实参。
    fn invoke_builtin(&mut self, name: &str, argc: usize) -> Result<Value, RuntimeError> {
        let arguments = self.pop_arguments(argc)?;

        if BuiltinMethod::from_qualified_name(name).is_some() {
            let receiver = self.pop()?;
            self.invoke_method(name, receiver, arguments)
        } else {
            self.invoke_function(name, arguments)
        }
    }

    fn invoke_method(
        &self,
        name: &str,
        receiver: Value,
        arguments: Vec<Value>,
    ) -> Result<Value, RuntimeError> {
        match name {
            "字符串.长度" => {
                let Value::String(text) = &receiver else {
                    return Err(self.builtin_receiver_error(name, &receiver));
                };
                Ok(Value::Integer(text.chars().count() as i64))
            }
            "字符串.拼接" => {
                let Value::String(text) = &receiver else {
                    return Err(self.builtin_receiver_error(name, &receiver));
                };
                let suffix = self.string_argument(name, &arguments, 0)?;
                Ok(Value::String(format!("{text}{suffix}")))
            }
            "字符串.转整数" => {
                let Value::String(text) = &receiver else {
                    return Err(self.builtin_receiver_error(name, &receiver));
                };
                let result = match parse_integer_text(text) {
                    Ok(value) => Value::ResultValue(Ok(Box::new(Value::Integer(value)))),
                    Err(message) => Value::ResultValue(Err(Box::new(Value::String(message)))),
                };
                Ok(result)
            }
            "字符串.包含" => {
                let Value::String(text) = &receiver else {
                    return Err(self.builtin_receiver_error(name, &receiver));
                };
                let needle = self.string_argument(name, &arguments, 0)?;
                Ok(Value::Bool(text.contains(&needle)))
            }
            "字符串.字符" => {
                let Value::String(text) = &receiver else {
                    return Err(self.builtin_receiver_error(name, &receiver));
                };
                let index = self.integer_argument(name, &arguments, 0)?;
                if index < 0 {
                    return Ok(Value::Optional(None));
                }
                let character = text
                    .chars()
                    .nth(index as usize)
                    .map(|ch| Box::new(Value::String(ch.to_string())));
                Ok(Value::Optional(character))
            }
            "字符串.切片" => {
                let Value::String(text) = &receiver else {
                    return Err(self.builtin_receiver_error(name, &receiver));
                };
                let start = self.integer_argument(name, &arguments, 0)?;
                let end = self.integer_argument(name, &arguments, 1)?;
                let characters = text.chars().collect::<Vec<_>>();
                let start = start.max(0).min(characters.len() as i64) as usize;
                let end = end.max(0).min(characters.len() as i64) as usize;
                let slice = if start < end {
                    characters[start..end].iter().collect()
                } else {
                    String::new()
                };
                Ok(Value::String(slice))
            }
            "字符串.查找" => {
                let Value::String(text) = &receiver else {
                    return Err(self.builtin_receiver_error(name, &receiver));
                };
                let needle = self.string_argument(name, &arguments, 0)?;
                let found = text
                    .find(&needle)
                    .map(|byte_index| text[..byte_index].chars().count() as i64);
                Ok(Value::Optional(
                    found.map(|index| Box::new(Value::Integer(index))),
                ))
            }
            "字符串.替换" => {
                let Value::String(text) = &receiver else {
                    return Err(self.builtin_receiver_error(name, &receiver));
                };
                let from = self.string_argument(name, &arguments, 0)?;
                let to = self.string_argument(name, &arguments, 1)?;
                Ok(Value::String(text.replace(&from, &to)))
            }
            "字符串.分割" => {
                let Value::String(text) = &receiver else {
                    return Err(self.builtin_receiver_error(name, &receiver));
                };
                let separator = self.string_argument(name, &arguments, 0)?;
                let parts = if separator.is_empty() {
                    text.chars()
                        .map(|ch| Value::String(ch.to_string()))
                        .collect::<Vec<_>>()
                } else {
                    text.split(&separator)
                        .map(|part| Value::String(part.to_owned()))
                        .collect::<Vec<_>>()
                };
                Ok(Value::List(Rc::new(RefCell::new(parts))))
            }
            "列表.长度" => {
                let Value::List(values) = &receiver else {
                    return Err(self.builtin_receiver_error(name, &receiver));
                };
                Ok(Value::Integer(values.borrow().len() as i64))
            }
            "列表.添加" => {
                let Value::List(values) = &receiver else {
                    return Err(self.builtin_receiver_error(name, &receiver));
                };
                let value = arguments
                    .into_iter()
                    .next()
                    .ok_or_else(|| self.error("内置方法「列表.添加」缺少实参".to_owned()))?;
                values.borrow_mut().push(value);
                Ok(receiver)
            }
            "列表.获取" => {
                let Value::List(values) = &receiver else {
                    return Err(self.builtin_receiver_error(name, &receiver));
                };
                let index = self.integer_argument(name, &arguments, 0)?;
                if index < 0 {
                    return Ok(Value::Optional(None));
                }
                let value = values.borrow().get(index as usize).cloned().map(Box::new);
                Ok(Value::Optional(value))
            }
            "列表.删除" => {
                let Value::List(values) = &receiver else {
                    return Err(self.builtin_receiver_error(name, &receiver));
                };
                let index = self.integer_argument(name, &arguments, 0)?;
                if index < 0 {
                    return Ok(Value::Bool(false));
                }
                let mut values = values.borrow_mut();
                let index = index as usize;
                if index >= values.len() {
                    return Ok(Value::Bool(false));
                }
                values.remove(index);
                Ok(Value::Bool(true))
            }
            "字典.长度" => {
                let Value::Dictionary(dictionary) = &receiver else {
                    return Err(self.builtin_receiver_error(name, &receiver));
                };
                Ok(Value::Integer(dictionary.borrow().len() as i64))
            }
            "字典.插入" => {
                let Value::Dictionary(dictionary) = &receiver else {
                    return Err(self.builtin_receiver_error(name, &receiver));
                };
                let key = arguments
                    .first()
                    .ok_or_else(|| self.error("内置方法「字典.插入」缺少键".to_owned()))?;
                let value = arguments
                    .get(1)
                    .cloned()
                    .ok_or_else(|| self.error("内置方法「字典.插入」缺少值".to_owned()))?;
                let key = self.dictionary_key(key)?;
                dictionary.borrow_mut().insert(key, value);
                Ok(receiver)
            }
            "字典.获取" => {
                let Value::Dictionary(dictionary) = &receiver else {
                    return Err(self.builtin_receiver_error(name, &receiver));
                };
                let key = arguments
                    .first()
                    .ok_or_else(|| self.error("内置方法「字典.获取」缺少键".to_owned()))?;
                let key = self.dictionary_key(key)?;
                let value = dictionary.borrow().get(&key).cloned().map(Box::new);
                Ok(Value::Optional(value))
            }
            "字典.删除" => {
                let Value::Dictionary(dictionary) = &receiver else {
                    return Err(self.builtin_receiver_error(name, &receiver));
                };
                let key = arguments
                    .first()
                    .ok_or_else(|| self.error("内置方法「字典.删除」缺少键".to_owned()))?;
                let key = self.dictionary_key(key)?;
                Ok(Value::Bool(dictionary.borrow_mut().remove(&key)))
            }
            "字典.包含键" => {
                let Value::Dictionary(dictionary) = &receiver else {
                    return Err(self.builtin_receiver_error(name, &receiver));
                };
                let key = arguments
                    .first()
                    .ok_or_else(|| self.error("内置方法「字典.包含键」缺少键".to_owned()))?;
                let key = self.dictionary_key(key)?;
                Ok(Value::Bool(dictionary.borrow().contains_key(&key)))
            }
            other => Err(self.error(format!("未知的内置方法「{other}」"))),
        }
    }

    fn invoke_function(&self, name: &str, arguments: Vec<Value>) -> Result<Value, RuntimeError> {
        match BuiltinFunction::from_name(name) {
            Some(BuiltinFunction::ReadText) => {
                let path = self.string_argument(name, &arguments, 0)?;
                let result = match fs::read_to_string(&path) {
                    Ok(text) => Value::ResultValue(Ok(Box::new(Value::String(text)))),
                    Err(error) => Value::ResultValue(Err(Box::new(Value::String(format!(
                        "无法读取文件「{path}」：{error}"
                    ))))),
                };
                Ok(result)
            }
            Some(BuiltinFunction::WriteText) => {
                let path = self.string_argument(name, &arguments, 0)?;
                let content = self.string_argument(name, &arguments, 1)?;
                let result = match fs::write(&path, content.as_bytes()) {
                    Ok(()) => {
                        Value::ResultValue(Ok(Box::new(Value::Integer(content.len() as i64))))
                    }
                    Err(error) => Value::ResultValue(Err(Box::new(Value::String(format!(
                        "无法写入文件「{path}」：{error}"
                    ))))),
                };
                Ok(result)
            }
            Some(BuiltinFunction::NewDictionary) => Ok(Value::Dictionary(Rc::new(RefCell::new(
                DictionaryValue::new(),
            )))),
            Some(BuiltinFunction::SystemTimestamp) => {
                let seconds = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|duration| duration.as_secs() as i64)
                    .unwrap_or(0);
                Ok(Value::Integer(seconds))
            }
            Some(BuiltinFunction::SystemArguments) => {
                let items = std::env::args()
                    .map(|item| Value::String(item.to_owned()))
                    .collect::<Vec<_>>();
                Ok(Value::List(Rc::new(RefCell::new(items))))
            }
            Some(BuiltinFunction::SystemEnvironment) => {
                let key = self.string_argument(name, &arguments, 0)?;
                let value = std::env::var(&key)
                    .ok()
                    .map(|text| Box::new(Value::String(text)));
                Ok(Value::Optional(value))
            }
            Some(BuiltinFunction::RegexMatch) => {
                let text = self.string_argument(name, &arguments, 0)?;
                let pattern = self.string_argument(name, &arguments, 1)?;
                let result = match regex::Regex::new(&pattern) {
                    Ok(re) => Ok(Value::Bool(re.is_match(&text))),
                    Err(error) => Err(self.error(format!("无效的正则模式「{pattern}」：{error}"))),
                };
                result
            }
            Some(BuiltinFunction::RegexReplace) => {
                let text = self.string_argument(name, &arguments, 0)?;
                let pattern = self.string_argument(name, &arguments, 1)?;
                let replacement = self.string_argument(name, &arguments, 2)?;
                let result = match regex::Regex::new(&pattern) {
                    Ok(re) => Ok(Value::String(
                        re.replace_all(&text, replacement.as_str()).into_owned(),
                    )),
                    Err(error) => Err(self.error(format!("无效的正则模式「{pattern}」：{error}"))),
                };
                result
            }
            Some(BuiltinFunction::JsonParse) => {
                let text = self.string_argument(name, &arguments, 0)?;
                let parsed = match serde_json::from_str::<serde_json::Value>(&text) {
                    Ok(value) => value,
                    Err(error) => {
                        return Ok(Value::ResultValue(Err(Box::new(Value::String(format!(
                            "JSON 解析失败：{error}"
                        ))))));
                    }
                };
                let serde_json::Value::Object(map) = parsed else {
                    return Ok(Value::ResultValue(Err(Box::new(Value::String(format!(
                        "JSON.解析 的实参必须是 JSON 对象，实际为「{text}」"
                    ))))));
                };
                let pairs = map
                    .into_iter()
                    .map(|(key, value)| {
                        (
                            key,
                            match value {
                                serde_json::Value::String(content) => content,
                                serde_json::Value::Null => "空".to_owned(),
                                serde_json::Value::Bool(flag) => {
                                    if flag {
                                        "真".to_owned()
                                    } else {
                                        "假".to_owned()
                                    }
                                }
                                other => other.to_string(),
                            },
                        )
                    })
                    .collect::<Vec<_>>();
                let dictionary = self.dictionary_from_pairs(name, &text, &pairs)?;
                Ok(Value::ResultValue(Ok(Box::new(dictionary))))
            }
            Some(BuiltinFunction::JsonGenerate) => {
                let dictionary = self.dictionary_argument(name, &arguments, 0)?;
                let mut entries = Vec::new();
                let value = dictionary.borrow();
                for key in &value.order {
                    if let Some(Value::String(content)) = value.entries.get(key) {
                        entries.push((
                            key.display_text(),
                            serde_json::Value::String(content.clone()),
                        ));
                    }
                }
                let object = serde_json::Map::from_iter(entries);
                let text = serde_json::to_string(&object)
                    .map_err(|error| self.error(format!("JSON 生成失败：{error}")))?;
                Ok(Value::String(text))
            }
            Some(BuiltinFunction::NetworkGet) => {
                let url = self.string_argument(name, &arguments, 0)?;
                let result = match ureq::get(&url).call() {
                    Ok(response) => match response.into_string() {
                        Ok(body) => Value::ResultValue(Ok(Box::new(Value::String(body)))),
                        Err(error) => Value::ResultValue(Err(Box::new(Value::String(format!(
                            "读取响应体失败：{error}"
                        ))))),
                    },
                    Err(error) => Value::ResultValue(Err(Box::new(Value::String(format!(
                        "无法访问网址「{url}」：{error}"
                    ))))),
                };
                Ok(result)
            }
            Some(BuiltinFunction::CryptoBase64Encode) => {
                let text = self.string_argument(name, &arguments, 0)?;
                let encoded = base64::Engine::encode(
                    &base64::engine::general_purpose::STANDARD,
                    text.as_bytes(),
                );
                Ok(Value::String(encoded))
            }
            Some(BuiltinFunction::CryptoSha256) => {
                let text = self.string_argument(name, &arguments, 0)?;
                use sha2::{Digest, Sha256};
                let mut hasher = Sha256::new();
                hasher.update(text.as_bytes());
                let digest = hasher.finalize();
                let hex: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
                Ok(Value::String(hex))
            }
            None => Err(self.error(format!("未知的内置函数「{name}」"))),
        }
    }

    fn builtin_receiver_error(&self, name: &str, receiver: &Value) -> RuntimeError {
        self.error(format!(
            "内置方法「{name}」的接收者类型不匹配，实际为「{}」",
            receiver.type_name()
        ))
    }

    fn string_argument(
        &self,
        name: &str,
        arguments: &[Value],
        index: usize,
    ) -> Result<String, RuntimeError> {
        match arguments.get(index) {
            Some(Value::String(value)) => Ok(value.clone()),
            Some(other) => Err(self.error(format!(
                "内置调用「{name}」的第 {} 个实参期望字符串，实际为「{}」",
                index + 1,
                other.type_name()
            ))),
            None => Err(self.error(format!("内置调用「{name}」缺少第 {} 个实参", index + 1))),
        }
    }

    fn integer_argument(
        &self,
        name: &str,
        arguments: &[Value],
        index: usize,
    ) -> Result<i64, RuntimeError> {
        match arguments.get(index) {
            Some(Value::Integer(value)) => Ok(*value),
            Some(other) => Err(self.error(format!(
                "内置调用「{name}」的第 {} 个实参期望整数，实际为「{}」",
                index + 1,
                other.type_name()
            ))),
            None => Err(self.error(format!("内置调用「{name}」缺少第 {} 个实参", index + 1))),
        }
    }

    /// 从运行期字典值构造 `JSON.生成` 需要的参数。
    fn dictionary_argument<'b>(
        &self,
        name: &str,
        arguments: &'b [Value],
        index: usize,
    ) -> Result<&'b Rc<RefCell<DictionaryValue>>, RuntimeError> {
        match arguments.get(index) {
            Some(Value::Dictionary(dictionary)) => Ok(dictionary),
            Some(other) => Err(self.error(format!(
                "内置调用「{name}」的第 {} 个实参期望字典，实际为「{}」",
                index + 1,
                other.type_name()
            ))),
            None => Err(self.error(format!("内置调用「{name}」缺少第 {} 个实参", index + 1))),
        }
    }

    /// 把字符串键值对写入新建字典；键值对无法解析时返回中文错误。
    fn dictionary_from_pairs(
        &self,
        name: &str,
        _source: &str,
        pairs: &[(String, String)],
    ) -> Result<Value, RuntimeError> {
        let mut dictionary = DictionaryValue::new();
        for (key, value) in pairs {
            let key = self
                .dictionary_key(&Value::String(key.clone()))
                .map_err(|_| {
                    self.error(format!(
                        "内置调用「{name}」的 JSON 键「{key}」不是合法的字典键"
                    ))
                })?;
            dictionary.insert(key, Value::String(value.clone()));
        }
        Ok(Value::Dictionary(Rc::new(RefCell::new(dictionary))))
    }

    fn dictionary_key(&self, value: &Value) -> Result<DictionaryKey, RuntimeError> {
        DictionaryKey::from_value(value).ok_or_else(|| {
            self.error(format!(
                "字典键必须是字符串、整数、小数或布尔，实际为「{}」",
                value.type_name()
            ))
        })
    }

    fn read_input(&mut self) -> Result<Value, RuntimeError> {
        if !self.use_stdio {
            return match self.input.pop_front() {
                Some(line) => Ok(Value::String(line)),
                None => Err(self.error("没有更多可用的模拟输入".to_owned())),
            };
        }

        print!("请输入：");
        if let Err(error) = io::stdout().flush() {
            return Err(self.error(format!("刷新标准输出失败：{error}")));
        }

        let mut line = String::new();
        match io::stdin().read_line(&mut line) {
            Ok(0) => Err(self.error("标准输入已结束".to_owned())),
            Ok(_) => {
                while line.ends_with('\n') || line.ends_with('\r') {
                    line.pop();
                }
                Ok(Value::String(line))
            }
            Err(error) => Err(self.error(format!("读取标准输入失败：{error}"))),
        }
    }
}

/// 算术运算的操作数。
#[derive(Clone, Copy)]
enum Number {
    Integer(i64),
    Float(f64),
}

impl Number {
    fn type_name(self) -> &'static str {
        match self {
            Self::Integer(_) => "整数",
            Self::Float(_) => "小数",
        }
    }
}

fn value_from_constant(constant: &Constant) -> Value {
    match constant {
        Constant::Integer(value) => Value::Integer(*value),
        Constant::Float(value) => Value::Float(*value),
        Constant::Bool(value) => Value::Bool(*value),
        Constant::String(value) => Value::String(value.clone()),
        Constant::Null => Value::Null,
    }
}

/// 严格解析十进制整数，并为失败结果生成中文说明。
fn parse_integer_text(text: &str) -> Result<i64, String> {
    match text.parse::<i64>() {
        Ok(value) => Ok(value),
        Err(_) => {
            let digits = text
                .strip_prefix('+')
                .or_else(|| text.strip_prefix('-'))
                .unwrap_or(text);
            if !digits.is_empty() && digits.chars().all(|ch| ch.is_ascii_digit()) {
                Err(format!("整数超出 64 位范围：「{text}」"))
            } else {
                Err(format!("不是有效的整数：「{text}」"))
            }
        }
    }
}

/// 使用标准输入输出执行字节码程序。
pub fn execute(program: &Program) -> Result<(), RuntimeError> {
    Vm::new(program).run()
}
