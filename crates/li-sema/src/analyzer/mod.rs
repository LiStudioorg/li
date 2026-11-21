//! 语义分析器核心：声明收集、类型解析、作用域与通用诊断。
//!
//! 表达式检查见 `expr` 子模块，语句与控制流检查见 `stmt` 子模块。

mod expr;
mod stmt;

use std::collections::{HashMap, HashSet};

use li_ast::{Block, EnumDecl, FuncDecl, Item, Literal, Program, Type, TypeDecl, TypeKind};
use li_errors::{Diagnostic, DiagnosticBag, Span};

use crate::symbols::{EnumInfo, FunctionSignature, StructInfo, SymbolTable, Variable};
use crate::ty::Ty;

/// 对整份程序执行语义分析。
///
/// 所有声明会先统一登记，因此函数可以递归调用，也可以引用后面才声明的
/// 函数与结构体。分析过程中收集全部诊断，最后一次性返回。
///
/// **强制警告机制**：安全类违规（不可变性、内存安全等）一律产生
/// `Warning` 诊断，编译继续；只有真正的语法错误、类型不匹配等
/// 致命问题才产生 `Error` 拦截编译。
///
/// 即使没有 Error，只要存在 Warning，也会把包含警告的
/// [`DiagnosticBag`] 通过 `Ok(Some(_))` 交给调用方；调用方
/// 可以把它打印到 stderr。
pub fn check(program: &Program) -> Result<Option<DiagnosticBag>, DiagnosticBag> {
    let mut analyzer = Analyzer::new();
    analyzer.run(program);

    if analyzer.diagnostics.has_errors() {
        Err(analyzer.diagnostics)
    } else if analyzer.diagnostics.has_warnings() {
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
    /// 全程序枚举定义：名称 → 枚举项载荷列表。
    enums: HashMap<String, EnumInfo>,
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
    /// 是否启用了 `#无垃圾回收`（no_std）模式。
    no_gc: bool,
}

impl Analyzer {
    fn new() -> Self {
        Self {
            functions: HashMap::new(),
            structs: HashMap::new(),
            enums: HashMap::new(),
            symbols: SymbolTable::default(),
            diagnostics: DiagnosticBag::new(),
            current_return: None,
            loop_depth: 0,
            unsafe_depth: 0,
            current_async: false,
            no_gc: false,
        }
    }

    /// 先登记声明，再检查所有函数体。
    fn run(&mut self, program: &Program) {
        self.no_gc = program.no_gc;
        let mut raw = RawDeclarations::default();
        collect_raw_declarations(&program.items, &mut raw);

        self.register_structs(&raw.structs);
        self.register_enums(&raw.enums);
        self.register_functions(&raw.functions);
        self.analyze_items(&program.items);
    }

    /// 登记枚举声明，把每个枚举项的载荷类型解析为语义类型。
    ///
    /// 与结构体登记相同，分两步：先登记全部枚举名称，再解析载荷类型；
    /// 这样枚举项的载荷可以引用后面声明的枚举或结构体。
    fn register_enums(&mut self, declarations: &[&EnumDecl]) {
        let mut unique: HashMap<String, &EnumDecl> = HashMap::new();
        for declaration in declarations {
            if unique.contains_key(&declaration.name.name) {
                self.report(
                    declaration.name.span,
                    format!("重复声明枚举「{}」", declaration.name.name),
                );
            } else {
                unique.insert(declaration.name.name.clone(), declaration);
            }
        }

        // 先登记全部名称，载荷才能引用后面声明的枚举或结构体。
        for name in unique.keys() {
            self.enums.insert(
                name.clone(),
                EnumInfo {
                    variants: Vec::new(),
                },
            );
        }

        for (name, declaration) in &unique {
            let mut variants = Vec::with_capacity(declaration.variants.len());
            for variant in &declaration.variants {
                let payload = variant
                    .args
                    .iter()
                    .map(|ty| self.resolve_type(ty))
                    .collect();
                variants.push((variant.name.name.clone(), payload));
            }

            if let Some(info) = self.enums.get_mut(name) {
                info.variants = variants;
            }
        }
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

            // C-ABI 外部函数没有函数体；登记签名但不允许调用，
            // VM / JS 后端在 IR 降级阶段会再次拦截。
            if declaration.is_extern {
                self.diagnostics.push(
                    Diagnostic::warning(format!(
                        "外部 函数「{}」将在 LLVM 后端生成 C-ABI 声明；\
                         VM 与 JS 后端无法在运行时绑定动态链接库",
                        declaration.name.name,
                    ))
                    .with_span(declaration.span)
                    .with_note("建议改用 `li 编译-原生` 编译并运行"),
                );

                let mut extern_params = Vec::with_capacity(declaration.params.len());
                for param in &declaration.params {
                    extern_params.push(self.resolve_type(&param.ty));
                }
                let extern_return = self.resolve_type(&declaration.return_type);

                self.functions.insert(
                    declaration.name.name.clone(),
                    FunctionSignature {
                        params: extern_params,
                        return_type: extern_return,
                        is_async: false,
                    },
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
    /// 在 `#无垃圾回收` 模式下，对依赖 GC 堆分配的类型发出黄色警告。
    /// 按强制警告机制，警告不阻断编译。
    fn warn_gc_dependent(&mut self, span: Span, type_name: &str) {
        if !self.no_gc {
            return;
        }
        self.diagnostics.push(
            Diagnostic::warning(format!("`#无垃圾回收` 模式下类型「{type_name}」依赖堆分配"))
                .with_span(span)
                .with_note("no_std 目标建议改用 `原生指针` 与 `不安全` 块手动管理内存"),
        );
    }

    fn resolve_type(&mut self, ty: &Type) -> Ty {
        match &ty.kind {
            TypeKind::Integer => Ty::Integer,
            TypeKind::Float => Ty::Float,
            TypeKind::Bool => Ty::Bool,
            TypeKind::String => {
                self.warn_gc_dependent(ty.span, "字符串");
                Ty::String
            }
            TypeKind::List(element) => {
                self.warn_gc_dependent(ty.span, "列表");
                Ty::List(Box::new(self.resolve_type(element)))
            }
            TypeKind::Dictionary { key, value } => {
                self.warn_gc_dependent(ty.span, "字典");
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
                } else if self.enums.contains_key(&ident.name) {
                    Ty::Enum(ident.name.clone())
                } else {
                    self.diagnostics.push(
                        Diagnostic::error(format!("未知类型「{}」", ident.name))
                            .with_span(ident.span)
                            .with_expected("已声明的结构体、枚举或基础类型")
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
                    self.analyze_items(&module.items);
                    self.symbols.pop_scope();
                }
                Item::Import(_) | Item::Type(_) | Item::View(_) | Item::Enum(_) => {}
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

        self.current_return = Some(return_type);
        self.current_async = function.is_async;
        self.analyze_block(&function.body);
        self.symbols.pop_scope();
        self.current_return = None;
        self.current_async = false;
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
    enums: Vec<&'a EnumDecl>,
}

/// 递归收集顶层与模块内的函数、结构体与枚举声明。
fn collect_raw_declarations<'a>(items: &'a [Item], raw: &mut RawDeclarations<'a>) {
    for item in items {
        match item {
            Item::Function(function) => raw.functions.push(function),
            Item::Type(declaration) => raw.structs.push(declaration),
            Item::Enum(declaration) => raw.enums.push(declaration),
            Item::Module(module) => collect_raw_declarations(&module.items, raw),
            Item::Import(_) | Item::View(_) => {}
        }
    }
}
