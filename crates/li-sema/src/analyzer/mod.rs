//! 语义分析器核心：声明收集、类型解析、作用域与通用诊断。
//!
//! 表达式检查见 `expr` 子模块，语句与控制流检查见 `stmt` 子模块。

mod expr;
mod stmt;

use std::collections::{HashMap, HashSet};

use li_ast::{Block, CheckAnnotation, FuncDecl, Item, Literal, Program, Type, TypeDecl, TypeKind};
use li_errors::{Diagnostic, DiagnosticBag, Rule, RuleConfig, RuleLevel, Span};

use crate::symbols::{FunctionSignature, StructInfo, SymbolTable, Variable};
use crate::ty::Ty;

/// 对整份程序执行语义分析；使用默认规则等级（`警告`）。
///
/// 所有声明会先统一登记，因此函数可以递归调用，也可以引用后面才声明的
/// 函数与结构体。分析过程中收集全部诊断，最后一次性返回。
pub fn check(program: &Program) -> Result<(), DiagnosticBag> {
    match check_with(program, &RuleConfig::new()) {
        Ok(Some(_warnings)) => Ok(()),
        Ok(None) => Ok(()),
        Err(errors) => Err(errors),
    }
}

/// 按 [`RuleConfig`] 对整份程序执行语义分析；`@检查(...)` 注解会局部覆盖
/// 项目级配置，覆盖优先级为「注解 > 项目配置 > 默认」。
///
/// 即使没有错误，只要有警告，也会把包含警告的 [`DiagnosticBag`] 通过
/// 第二个返回值交给调用方；调用方可以把它打印到 stderr。
pub fn check_with(
    program: &Program,
    config: &RuleConfig,
) -> Result<Option<DiagnosticBag>, DiagnosticBag> {
    let mut analyzer = Analyzer::new(config.clone());
    analyzer.run(program);

    if analyzer.diagnostics.has_errors() {
        Err(analyzer.diagnostics)
    } else if analyzer.diagnostics.has_warnings() {
        // 编译继续，但把警告交给 CLI 打印。
        Ok(Some(analyzer.diagnostics))
    } else {
        Ok(None)
    }
}

/// 语义分析器的内部状态。
struct Analyzer {
    /// 全程序函数签名。
    functions: HashMap<String, FunctionSignature>,
    /// 全程序结构体定义。
    structs: HashMap<String, StructInfo>,
    /// 当前变量作用域栈。
    symbols: SymbolTable,
    /// 收集到的语义诊断。
    diagnostics: DiagnosticBag,
    /// 当前函数的返回类型。
    current_return: Option<Ty>,
    /// 当前循环嵌套深度，用于检查 `跳出` / `继续`。
    loop_depth: usize,
    /// 当前 `不安全` 块嵌套深度。
    unsafe_depth: usize,
    /// 当前是否处于异步函数体内；用于 `等待` 与 `网络.获取` 的类型推导。
    current_async: bool,
    /// 项目级检查规则配置；未声明的规则使用默认等级。
    checks: RuleConfig,
    /// 当前函数/模块内由 `@检查(...)` 注解覆盖的规则等级；
    /// 嵌套作用域按 LIFO 弹出。
    annotation_stack: Vec<HashMap<Rule, RuleLevel>>,
}

impl Analyzer {
    fn new(checks: RuleConfig) -> Self {
        Self {
            functions: HashMap::new(),
            structs: HashMap::new(),
            symbols: SymbolTable::default(),
            diagnostics: DiagnosticBag::new(),
            current_return: None,
            loop_depth: 0,
            unsafe_depth: 0,
            current_async: false,
            checks,
            annotation_stack: Vec::new(),
        }
    }

    /// 先登记声明，再检查所有函数体。
    fn run(&mut self, program: &Program) {
        let mut raw = RawDeclarations::default();
        collect_raw_declarations(&program.items, &mut raw);

        self.register_structs(&raw.structs);
        self.register_functions(&raw.functions);
        self.analyze_items(&program.items);
    }

    /// 登记结构体名称，并解析字段类型。
    fn register_structs(&mut self, declarations: &[&TypeDecl]) {
        let mut unique: HashMap<String, &TypeDecl> = HashMap::new();

        for declaration in declarations {
            if unique.contains_key(&declaration.name.name) {
                self.report(
                    declaration.name.span,
                    format!("重复声明类型「{}」", declaration.name.name),
                );
            } else {
                unique.insert(declaration.name.name.clone(), declaration);
            }
        }

        // 先登记全部名称，字段才能引用后面声明的结构体。
        for name in unique.keys() {
            self.structs.insert(
                name.clone(),
                StructInfo {
                    fields: HashMap::new(),
                },
            );
        }

        for (name, declaration) in &unique {
            let mut fields = HashMap::new();

            for field in &declaration.fields {
                let ty = self.resolve_type(&field.ty);
                if fields.insert(field.name.name.clone(), ty).is_some() {
                    self.report(
                        field.name.span,
                        format!("结构体「{name}」重复声明字段「{}」", field.name.name),
                    );
                }
            }

            if let Some(info) = self.structs.get_mut(name) {
                info.fields = fields;
            }
        }
    }

    /// 登记函数签名，参数与返回类型都会在此完成解析。
    fn register_functions(&mut self, declarations: &[&FuncDecl]) {
        for declaration in declarations {
            if self.functions.contains_key(&declaration.name.name) {
                self.report(
                    declaration.name.span,
                    format!("重复声明函数「{}」", declaration.name.name),
                );
                continue;
            }

            let mut params = Vec::with_capacity(declaration.params.len());
            for param in &declaration.params {
                params.push(self.resolve_type(&param.ty));
            }
            let return_type = self.resolve_type(&declaration.return_type);

            self.functions.insert(
                declaration.name.name.clone(),
                FunctionSignature {
                    params,
                    return_type,
                    is_async: declaration.is_async,
                },
            );
        }
    }

    /// 把 AST 类型解析为语义类型。
    fn resolve_type(&mut self, ty: &Type) -> Ty {
        match &ty.kind {
            TypeKind::Integer => Ty::Integer,
            TypeKind::Float => Ty::Float,
            TypeKind::Bool => Ty::Bool,
            TypeKind::String => Ty::String,
            TypeKind::List(element) => Ty::List(Box::new(self.resolve_type(element))),
            TypeKind::Dictionary { key, value } => {
                let key_ty = self.resolve_type(key);
                if !key_ty.is_hashable() {
                    self.diagnostics.push(
                        Diagnostic::error(format!("字典键类型「{key_ty}」不可哈希"))
                            .with_span(key.span)
                            .with_expected("字符串、整数、小数 或 布尔")
                            .with_actual(key_ty.to_string())
                            .with_note("第一版字典使用宿主哈希表，只支持标量键"),
                    );
                }
                Ty::Dictionary {
                    key: Box::new(key_ty),
                    value: Box::new(self.resolve_type(value)),
                }
            }
            TypeKind::Optional(inner) => Ty::Optional(Box::new(self.resolve_type(inner))),
            TypeKind::RawPointer(inner) => Ty::RawPointer(Box::new(self.resolve_type(inner))),
            TypeKind::Task(inner) => Ty::Task(Box::new(self.resolve_type(inner))),
            TypeKind::Result { value, error } => Ty::Result {
                ok: Box::new(self.resolve_type(value)),
                error: Box::new(self.resolve_type(error)),
            },
            TypeKind::Named(ident) => {
                if self.structs.contains_key(&ident.name) {
                    Ty::Struct(ident.name.clone())
                } else {
                    self.diagnostics.push(
                        Diagnostic::error(format!("未知类型「{}」", ident.name))
                            .with_span(ident.span)
                            .with_expected("已声明的结构体或基础类型")
                            .with_actual(format!("`{}`", ident.name)),
                    );
                    Ty::Error
                }
            }
        }
    }

    /// 按源码顺序检查顶层项；模块拥有自己的声明作用域。
    fn analyze_items(&mut self, items: &[Item]) {
        for item in items {
            match item {
                Item::Function(function) => self.analyze_function(function),
                Item::Module(module) => {
                    self.symbols.push_scope();
                    self.push_annotations(&module.check_annotations);
                    self.analyze_items(&module.items);
                    self.pop_annotations();
                    self.symbols.pop_scope();
                }
                Item::Import(_) | Item::Type(_) | Item::View(_) => {}
            }
        }
    }

    /// 检查单个函数：参数进入函数作用域，函数体再进入块作用域。
    fn analyze_function(&mut self, function: &FuncDecl) {
        let signature = self.functions.get(&function.name.name).cloned();
        let return_type = signature
            .as_ref()
            .map(|signature| signature.return_type.clone())
            .unwrap_or(Ty::Error);

        self.symbols.push_scope();
        let mut seen_params = HashSet::new();

        for (index, param) in function.params.iter().enumerate() {
            if !seen_params.insert(param.name.name.clone()) {
                self.report(
                    param.name.span,
                    format!(
                        "函数「{}」重复声明参数「{}」",
                        function.name.name, param.name.name
                    ),
                );
                continue;
            }

            let ty = signature
                .as_ref()
                .and_then(|signature| signature.params.get(index))
                .cloned()
                .unwrap_or(Ty::Error);

            let _ = self
                .symbols
                .declare(&param.name.name, Variable { ty, mutable: false });
        }

        // 解析函数上方的 `@检查(...)` 注解；局部覆盖项目级配置。
        self.push_annotations(&function.check_annotations);

        self.current_return = Some(return_type);
        self.current_async = function.is_async;
        self.analyze_block(&function.body);
        self.pop_annotations();
        self.symbols.pop_scope();
        self.current_return = None;
        self.current_async = false;
    }

    /// 把函数/模块上方的 `@检查(...)` 注解压入注解栈；
    /// 同名规则在多条注解中按“最后一条生效”处理。
    fn push_annotations(&mut self, annotations: &[CheckAnnotation]) {
        if annotations.is_empty() {
            return;
        }

        let mut overrides = HashMap::new();
        for annotation in annotations {
            let rule = Rule::parse(&annotation.rule.name).unwrap_or(Rule::Unknown);
            let level = RuleLevel::parse(&annotation.level.name).unwrap_or_default();
            if level == RuleLevel::Warning {
                // 与默认值一致，不需要写入覆盖。
                continue;
            }
            overrides.insert(rule, level);
        }

        self.annotation_stack.push(overrides);
    }

    /// 弹出最近一次 `@检查(...)` 注解覆盖的作用域。
    fn pop_annotations(&mut self) {
        self.annotation_stack.pop();
    }

    /// 按「注解 > 项目配置 > 默认」优先级返回某条规则的等级。
    fn level_for(&self, rule: Rule) -> RuleLevel {
        for layer in self.annotation_stack.iter().rev() {
            if let Some(level) = layer.get(&rule) {
                return *level;
            }
        }
        self.checks.level_of(rule)
    }

    /// 进入新的块作用域检查语句。
    fn analyze_block(&mut self, block: &Block) {
        self.symbols.push_scope();
        for statement in &block.stmts {
            self.analyze_statement(statement);
        }
        self.symbols.pop_scope();
    }

    /// 判断 `actual` 能否用于期望类型 `expected`。
    ///
    /// 除完全相同外，仅允许三种“构造”场景：
    /// - 值或 `空` 包装为 `可选<T>`；
    /// - 成功值包装为 `结果<T, E>`；
    /// - 空列表的元素类型视为未知，可匹配任意列表。
    ///
    /// 数字类型之间不会隐式转换。
    fn is_compatible(expected: &Ty, actual: &Ty) -> bool {
        if expected.is_error() || actual.is_error() || expected == actual {
            return true;
        }

        match expected {
            Ty::Optional(inner) => match actual {
                Ty::Null => true,
                other => Self::is_compatible(inner, other),
            },
            Ty::List(expected_element) => match actual {
                Ty::List(actual_element) => Self::is_compatible(expected_element, actual_element),
                _ => false,
            },
            Ty::Dictionary {
                key: expected_key,
                value: expected_value,
            } => match actual {
                Ty::Dictionary {
                    key: actual_key,
                    value: actual_value,
                } => {
                    Self::is_compatible(expected_key, actual_key)
                        && Self::is_compatible(expected_value, actual_value)
                }
                _ => false,
            },
            Ty::Result { ok, .. } => Self::is_compatible(ok, actual),
            Ty::RawPointer(_) => matches!(actual, Ty::Null),
            _ => false,
        }
    }

    /// 检查类型兼容性，不兼容时生成带上下文的中文诊断。
    fn check_compatible(&mut self, span: Span, expected: &Ty, actual: &Ty, context: &str) -> bool {
        if Self::is_compatible(expected, actual) {
            return true;
        }

        let mut diagnostic =
            Diagnostic::type_mismatch(span, expected.to_string(), actual.to_string());

        if !context.is_empty() {
            diagnostic = diagnostic.with_note(context.to_owned());
        }
        if expected.is_numeric() && actual.is_numeric() {
            diagnostic = diagnostic.with_note("li 不支持隐式数值转换");
        }

        match actual {
            Ty::Optional(_) => diagnostic = diagnostic.with_note("请先使用「匹配」判空"),
            Ty::Result { .. } => {
                diagnostic = diagnostic.with_note("请先使用「匹配」处理成功与失败");
            }
            _ => {}
        }

        self.diagnostics.push(diagnostic);
        false
    }

    /// 添加一条仅含位置与消息的诊断。
    fn report(&mut self, span: Span, message: impl Into<String>) {
        self.diagnostics
            .push(Diagnostic::error(message).with_span(span));
    }

    /// 由字面量推导类型。
    fn literal_type(literal: &Literal) -> Ty {
        match literal {
            Literal::Integer(_) => Ty::Integer,
            Literal::Float(_) => Ty::Float,
            Literal::Bool(_) => Ty::Bool,
            Literal::String(_) => Ty::String,
            Literal::Null => Ty::Null,
        }
    }
}

/// 尚未解析的声明引用。
#[derive(Default)]
struct RawDeclarations<'a> {
    functions: Vec<&'a FuncDecl>,
    structs: Vec<&'a TypeDecl>,
}

/// 递归收集顶层与模块内的函数、结构体声明。
fn collect_raw_declarations<'a>(items: &'a [Item], raw: &mut RawDeclarations<'a>) {
    for item in items {
        match item {
            Item::Function(function) => raw.functions.push(function),
            Item::Type(declaration) => raw.structs.push(declaration),
            Item::Module(module) => collect_raw_declarations(&module.items, raw),
            Item::Import(_) | Item::View(_) => {}
        }
    }
}
