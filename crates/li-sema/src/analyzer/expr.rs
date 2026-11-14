//! 表达式类型推导。
//!
//! 所有运算都要求两侧类型完全一致；唯一的“包装”例外是
//! 值可以进入可选类型，成功值可以进入结果类型，见
//! [`Analyzer::is_compatible`]。

use li_ast::{BinaryOp, Expr, ExprKind, Ident, UnaryOp};
use li_errors::Diagnostic;

use super::Analyzer;
use crate::symbols::FunctionSignature;
use crate::ty::Ty;

impl Analyzer {
    /// 推导表达式类型，并在发现错误时记录诊断。
    pub(super) fn infer_expr(&mut self, expr: &Expr) -> Ty {
        match &expr.kind {
            ExprKind::Literal(literal) => Self::literal_type(literal),
            ExprKind::Identifier(ident) => self.infer_identifier(ident),
            ExprKind::Input => Ty::String,
            ExprKind::Unary { op, operand } => self.infer_unary(operand, *op),
            ExprKind::Binary { left, op, right } => self.infer_binary(left, *op, right),
            ExprKind::Call { callee, args } => self.infer_call(expr, callee, args),
            ExprKind::FieldAccess { object, field } => self.infer_field_access(object, field),
            ExprKind::Index { object, index } => self.infer_index(object, index),
            ExprKind::ListLiteral(items) => self.infer_list_literal(items),
            ExprKind::Parenthesized(inner) => self.infer_expr(inner),
        }
    }

    fn infer_identifier(&mut self, ident: &Ident) -> Ty {
        match self.symbols.lookup(&ident.name) {
            Some(variable) => {
                let ty = variable.ty.clone();
                if ty.is_raw_pointer() && self.unsafe_depth == 0 {
                    self.diagnostics.push(
                        Diagnostic::error(format!(
                            "原生指针「{}」只能在「不安全」块中使用",
                            ident.name
                        ))
                        .with_span(ident.span)
                        .with_expected("位于「不安全 { ... }」块内")
                        .with_actual("安全代码"),
                    );
                }
                ty
            }
            None => {
                self.diagnostics.push(
                    Diagnostic::error(format!("未定义的变量「{}」", ident.name))
                        .with_span(ident.span)
                        .with_expected("已声明的变量或函数参数")
                        .with_actual(format!("`{}`", ident.name)),
                );
                Ty::Error
            }
        }
    }

    fn infer_unary(&mut self, operand: &Expr, op: UnaryOp) -> Ty {
        let operand_ty = self.infer_expr(operand);
        let operand_ty = self.ensure_plain(operand, operand_ty);

        match op {
            UnaryOp::Negate => {
                if operand_ty.is_error() {
                    Ty::Error
                } else if operand_ty.is_numeric() {
                    operand_ty
                } else {
                    self.diagnostics.push(
                        Diagnostic::type_mismatch(
                            operand.span,
                            "数字类型（整数或小数）",
                            operand_ty.to_string(),
                        )
                        .with_note("一元负号只能用于整数或小数"),
                    );
                    Ty::Error
                }
            }
            UnaryOp::Not => {
                if !operand_ty.is_error() && operand_ty != Ty::Bool {
                    self.diagnostics.push(
                        Diagnostic::type_mismatch(operand.span, "布尔", operand_ty.to_string())
                            .with_note("逻辑非只能用于布尔表达式"),
                    );
                }
                Ty::Bool
            }
        }
    }

    fn infer_binary(&mut self, left: &Expr, op: BinaryOp, right: &Expr) -> Ty {
        let left_ty = self.infer_expr(left);
        let left_ty = self.ensure_plain(left, left_ty);
        let right_ty = self.infer_expr(right);
        let right_ty = self.ensure_plain(right, right_ty);

        match op {
            BinaryOp::Add | BinaryOp::Subtract | BinaryOp::Multiply | BinaryOp::Divide => {
                // `加` 对两个字符串表示拼接；只要有一侧不是字符串，
                // 仍按原有的数值规则检查，保持严格的类型诊断。
                if op == BinaryOp::Add && left_ty == Ty::String && right_ty == Ty::String {
                    return Ty::String;
                }

                let Some(left_ty) = self.numeric_operand(left, left_ty) else {
                    return Ty::Error;
                };
                let Some(right_ty) = self.numeric_operand(right, right_ty) else {
                    return Ty::Error;
                };

                if left_ty != right_ty {
                    self.diagnostics.push(
                        Diagnostic::type_mismatch(
                            right.span,
                            left_ty.to_string(),
                            right_ty.to_string(),
                        )
                        .with_note("算术运算两侧类型必须一致，li 不支持隐式数值转换"),
                    );
                    Ty::Error
                } else {
                    left_ty
                }
            }
            BinaryOp::Less | BinaryOp::Greater => {
                let left_ty = self.numeric_operand(left, left_ty);
                let right_ty = self.numeric_operand(right, right_ty);

                if let (Some(left_ty), Some(right_ty)) = (left_ty, right_ty) {
                    if left_ty != right_ty {
                        self.diagnostics.push(
                            Diagnostic::type_mismatch(
                                right.span,
                                left_ty.to_string(),
                                right_ty.to_string(),
                            )
                            .with_note("比较运算两侧类型必须一致，li 不支持隐式数值转换"),
                        );
                    }
                }

                Ty::Bool
            }
            BinaryOp::Equal => {
                if !left_ty.is_error() && !right_ty.is_error() && left_ty != right_ty {
                    self.diagnostics.push(
                        Diagnostic::type_mismatch(
                            right.span,
                            left_ty.to_string(),
                            right_ty.to_string(),
                        )
                        .with_note("相等比较两侧类型必须一致，li 不支持隐式转换"),
                    );
                }
                Ty::Bool
            }
            BinaryOp::And | BinaryOp::Or => {
                if !left_ty.is_error() && left_ty != Ty::Bool {
                    self.diagnostics.push(
                        Diagnostic::type_mismatch(left.span, "布尔", left_ty.to_string())
                            .with_note("逻辑运算的操作数必须是布尔表达式"),
                    );
                }
                if !right_ty.is_error() && right_ty != Ty::Bool {
                    self.diagnostics.push(
                        Diagnostic::type_mismatch(right.span, "布尔", right_ty.to_string())
                            .with_note("逻辑运算的操作数必须是布尔表达式"),
                    );
                }
                Ty::Bool
            }
        }
    }

    fn infer_call(&mut self, expr: &Expr, callee: &Expr, args: &[Expr]) -> Ty {
        if let ExprKind::FieldAccess { object, field } = &callee.kind {
            return self.infer_builtin_call(object, field, args);
        }

        let function_name = match &callee.kind {
            ExprKind::Identifier(ident) => Some(ident.name.clone()),
            _ => {
                self.diagnostics.push(
                    Diagnostic::error("不支持的函数调用形式")
                        .with_span(callee.span)
                        .with_expected("函数名")
                        .with_actual("复合表达式"),
                );
                None
            }
        };

        let signature = if let Some(name) = &function_name {
            match self.functions.get(name).cloned() {
                Some(signature) => Some(signature),
                None => match SemaBuiltinFunction::from_name(name) {
                    Some(builtin) => Some(builtin.signature()),
                    None => {
                        self.diagnostics.push(
                            Diagnostic::error(format!("未定义的函数「{name}」"))
                                .with_span(callee.span)
                                .with_expected("已声明的函数或内置函数")
                                .with_actual(format!("`{name}`")),
                        );
                        None
                    }
                },
            }
        } else {
            None
        };

        // 即使函数不存在，也要检查实参内部是否还有独立错误。
        let mut argument_types = Vec::with_capacity(args.len());
        for argument in args {
            argument_types.push(self.infer_expr(argument));
        }

        let Some(signature) = signature else {
            return Ty::Error;
        };

        if args.len() != signature.params.len() {
            let name = function_name.as_deref().unwrap_or("");
            self.diagnostics.push(
                Diagnostic::error("函数参数数量不匹配")
                    .with_span(expr.span)
                    .with_expected(format!("{} 个参数", signature.params.len()))
                    .with_actual(format!("{} 个参数", args.len()))
                    .with_note(format!("函数「{name}」")),
            );
        }

        for (index, (argument, actual)) in args.iter().zip(&argument_types).enumerate() {
            if let Some(expected) = signature.params.get(index) {
                self.check_compatible(
                    argument.span,
                    expected,
                    actual,
                    &format!("第 {} 个参数", index + 1),
                );
            }
        }

        if signature.return_type.is_raw_pointer() && self.unsafe_depth == 0 {
            self.diagnostics.push(
                Diagnostic::error(format!(
                    "函数「{}」返回原生指针，调用结果必须在「不安全」块中使用",
                    function_name.as_deref().unwrap_or("")
                ))
                .with_span(expr.span)
                .with_expected("位于「不安全 { ... }」块内")
                .with_actual("安全代码"),
            );
        }

        signature.return_type
    }

    /// 推导字符串与列表的内置方法调用。
    ///
    /// 方法调用与结构体字段访问共享点号语法，因此在调用上下文中必须先
    /// 判断接收者是否为字符串或列表；可选值与结果值仍然要求先经过匹配。
    fn infer_builtin_call(&mut self, object: &Expr, field: &Ident, args: &[Expr]) -> Ty {
        let receiver_ty = self.infer_expr(object);
        let receiver_ty = self.ensure_plain(object, receiver_ty);

        match receiver_ty {
            Ty::String => self.infer_string_method(field, args),
            Ty::List(element) => self.infer_list_method(field, args, *element),
            Ty::Dictionary { key, value } => {
                self.infer_dictionary_method(field, args, *key, *value)
            }
            Ty::Error => {
                self.infer_arguments(args);
                Ty::Error
            }
            other => {
                self.infer_arguments(args);
                self.diagnostics.push(
                    Diagnostic::error(format!("类型「{other}」不支持方法「{}」", field.name))
                        .with_span(field.span)
                        .with_expected("字符串或列表")
                        .with_actual(other.to_string())
                        .with_note("内置方法目前只支持字符串与列表"),
                );
                Ty::Error
            }
        }
    }

    fn infer_string_method(&mut self, field: &Ident, args: &[Expr]) -> Ty {
        match field.name.as_str() {
            "长度" => {
                self.check_method_args(field, "字符串.长度", args, &[]);
                Ty::Integer
            }
            "拼接" => {
                self.check_method_args(field, "字符串.拼接", args, &[Ty::String]);
                Ty::String
            }
            "转整数" => {
                self.check_method_args(field, "字符串.转整数", args, &[]);
                Ty::Result {
                    ok: Box::new(Ty::Integer),
                    error: Box::new(Ty::String),
                }
            }
            "包含" => {
                self.check_method_args(field, "字符串.包含", args, &[Ty::String]);
                Ty::Bool
            }
            "字符" => {
                self.check_method_args(field, "字符串.字符", args, &[Ty::Integer]);
                Ty::Optional(Box::new(Ty::String))
            }
            "切片" => {
                self.check_method_args(field, "字符串.切片", args, &[Ty::Integer, Ty::Integer]);
                Ty::String
            }
            "查找" => {
                self.check_method_args(field, "字符串.查找", args, &[Ty::String]);
                Ty::Optional(Box::new(Ty::Integer))
            }
            "替换" => {
                self.check_method_args(field, "字符串.替换", args, &[Ty::String, Ty::String]);
                Ty::String
            }
            "分割" => {
                self.check_method_args(field, "字符串.分割", args, &[Ty::String]);
                Ty::List(Box::new(Ty::String))
            }
            other => {
                self.infer_arguments(args);
                self.diagnostics.push(
                    Diagnostic::error(format!("字符串没有方法「{other}」"))
                        .with_span(field.span)
                        .with_expected("长度、拼接、转整数、包含、字符、切片、查找、替换 或 分割")
                        .with_actual(format!("`{other}`")),
                );
                Ty::Error
            }
        }
    }

    fn infer_list_method(&mut self, field: &Ident, args: &[Expr], element: Ty) -> Ty {
        match field.name.as_str() {
            "长度" => {
                self.check_method_args(field, "列表.长度", args, &[]);
                Ty::Integer
            }
            "添加" => {
                self.check_method_args(field, "列表.添加", args, std::slice::from_ref(&element));
                Ty::List(Box::new(element))
            }
            "获取" => {
                self.check_method_args(field, "列表.获取", args, &[Ty::Integer]);
                Ty::Optional(Box::new(element))
            }
            "删除" => {
                self.check_method_args(field, "列表.删除", args, &[Ty::Integer]);
                Ty::Bool
            }
            other => {
                self.infer_arguments(args);
                self.diagnostics.push(
                    Diagnostic::error(format!("列表没有方法「{other}」"))
                        .with_span(field.span)
                        .with_expected("长度、添加、获取 或 删除")
                        .with_actual(format!("`{other}`")),
                );
                Ty::Error
            }
        }
    }

    fn infer_dictionary_method(&mut self, field: &Ident, args: &[Expr], key: Ty, value: Ty) -> Ty {
        match field.name.as_str() {
            "长度" => {
                self.check_method_args(field, "字典.长度", args, &[]);
                Ty::Integer
            }
            "插入" => {
                self.check_method_args(field, "字典.插入", args, &[key.clone(), value.clone()]);
                Ty::Dictionary {
                    key: Box::new(key),
                    value: Box::new(value),
                }
            }
            "获取" => {
                self.check_method_args(field, "字典.获取", args, &[key]);
                Ty::Optional(Box::new(value))
            }
            "删除" => {
                self.check_method_args(field, "字典.删除", args, &[key]);
                Ty::Bool
            }
            "包含键" => {
                self.check_method_args(field, "字典.包含键", args, &[key]);
                Ty::Bool
            }
            other => {
                self.infer_arguments(args);
                self.diagnostics.push(
                    Diagnostic::error(format!("字典没有方法「{other}」"))
                        .with_span(field.span)
                        .with_expected("长度、插入、获取、删除 或 包含键")
                        .with_actual(format!("`{other}`")),
                );
                Ty::Error
            }
        }
    }

    /// 检查内置方法的实参数量与类型。
    fn check_method_args(&mut self, field: &Ident, method: &str, args: &[Expr], expected: &[Ty]) {
        let actual = args
            .iter()
            .map(|argument| self.infer_expr(argument))
            .collect::<Vec<_>>();

        if args.len() != expected.len() {
            self.diagnostics.push(
                Diagnostic::error("方法参数数量不匹配")
                    .with_span(field.span)
                    .with_expected(format!("{} 个参数", expected.len()))
                    .with_actual(format!("{} 个参数", args.len()))
                    .with_note(format!("方法「{method}」")),
            );
        }

        for (index, (argument, actual)) in args.iter().zip(&actual).enumerate() {
            if let Some(expected) = expected.get(index) {
                self.check_compatible(
                    argument.span,
                    expected,
                    actual,
                    &format!("第 {} 个参数", index + 1),
                );
            }
        }
    }

    /// 只推导实参类型，用于未知方法等场景，以便继续发现实参内部的独立错误。
    fn infer_arguments(&mut self, args: &[Expr]) {
        for argument in args {
            let _ = self.infer_expr(argument);
        }
    }

    fn infer_field_access(&mut self, object: &Expr, field: &Ident) -> Ty {
        let object_ty = self.infer_expr(object);
        let object_ty = self.ensure_plain(object, object_ty);

        match object_ty {
            Ty::Struct(name) => {
                match self
                    .structs
                    .get(&name)
                    .and_then(|info| info.fields.get(&field.name))
                {
                    Some(ty) => ty.clone(),
                    None => {
                        self.diagnostics.push(
                            Diagnostic::error(format!(
                                "结构体「{name}」没有字段「{}」",
                                field.name
                            ))
                            .with_span(field.span)
                            .with_expected("已声明的结构体字段")
                            .with_actual(format!("`{}`", field.name)),
                        );
                        Ty::Error
                    }
                }
            }
            Ty::Error => Ty::Error,
            Ty::String => {
                if matches!(field.name.as_str(), "长度" | "拼接" | "转整数" | "包含") {
                    self.report_method_requires_call(field);
                } else {
                    self.diagnostics.push(
                        Diagnostic::error(format!("字符串没有字段「{}」", field.name))
                            .with_span(field.span)
                            .with_expected("字符串内置方法调用")
                            .with_actual(format!("`{}`", field.name)),
                    );
                }
                Ty::Error
            }
            Ty::List(_) => {
                if matches!(field.name.as_str(), "长度" | "添加" | "获取" | "删除") {
                    self.report_method_requires_call(field);
                } else {
                    self.diagnostics.push(
                        Diagnostic::error(format!("列表没有字段「{}」", field.name))
                            .with_span(field.span)
                            .with_expected("列表内置方法调用")
                            .with_actual(format!("`{}`", field.name)),
                    );
                }
                Ty::Error
            }
            Ty::Dictionary { .. } => {
                if matches!(
                    field.name.as_str(),
                    "长度" | "插入" | "获取" | "删除" | "包含键"
                ) {
                    self.report_method_requires_call(field);
                } else {
                    self.diagnostics.push(
                        Diagnostic::error(format!("字典没有字段「{}」", field.name))
                            .with_span(field.span)
                            .with_expected("字典内置方法调用")
                            .with_actual(format!("`{}`", field.name)),
                    );
                }
                Ty::Error
            }
            other => {
                self.diagnostics.push(
                    Diagnostic::error("不能访问字段")
                        .with_span(field.span)
                        .with_expected("结构体类型")
                        .with_actual(other.to_string())
                        .with_note(format!("字段「{}」", field.name)),
                );
                Ty::Error
            }
        }
    }

    /// 点号后跟已知方法名但没有括号时，给出明确的调用提示。
    fn report_method_requires_call(&mut self, field: &Ident) {
        self.diagnostics.push(
            Diagnostic::error(format!("方法「{}」必须使用括号调用", field.name))
                .with_span(field.span)
                .with_expected(format!("`{}(...)`", field.name))
                .with_actual(format!("`{}`", field.name))
                .with_note("内置方法通过点号调用，例如 `列表.长度()`"),
        );
    }

    fn infer_index(&mut self, object: &Expr, index: &Expr) -> Ty {
        let object_ty = self.infer_expr(object);
        let object_ty = self.ensure_plain(object, object_ty);
        let index_ty = self.infer_expr(index);
        let index_ty = self.ensure_plain(index, index_ty);

        if !index_ty.is_error() && index_ty != Ty::Integer {
            self.diagnostics.push(
                Diagnostic::type_mismatch(index.span, "整数", index_ty.to_string())
                    .with_note("列表下标必须是整数"),
            );
        }

        match object_ty {
            Ty::List(element) => *element,
            Ty::Error => Ty::Error,
            other => {
                self.diagnostics.push(
                    Diagnostic::error("不能使用下标访问")
                        .with_span(object.span)
                        .with_expected("列表类型")
                        .with_actual(other.to_string()),
                );
                Ty::Error
            }
        }
    }

    fn infer_list_literal(&mut self, items: &[Expr]) -> Ty {
        let mut element_ty: Option<Ty> = None;

        for item in items {
            let item_ty = self.infer_expr(item);

            let Some(expected) = element_ty.clone() else {
                element_ty = Some(item_ty);
                continue;
            };

            if expected.is_error() {
                element_ty = Some(item_ty);
            } else if !item_ty.is_error() && !Self::is_compatible(&expected, &item_ty) {
                self.diagnostics.push(
                    Diagnostic::type_mismatch(item.span, expected.to_string(), item_ty.to_string())
                        .with_note("列表元素类型必须一致，li 不支持隐式转换"),
                );
            }
        }

        Ty::List(Box::new(element_ty.unwrap_or(Ty::Error)))
    }

    /// 检查值能否作为普通值使用。
    ///
    /// 可选类型必须先经过 `匹配` 判空；结果类型必须先经过 `匹配` 处理。
    /// 这里只拦截“真正消费值”的场景；把值存进变量、作为实参传递或作为
    /// `匹配` 的待匹配表达式仍然允许。
    pub(super) fn ensure_plain(&mut self, expr: &Expr, ty: Ty) -> Ty {
        if matches!(&ty, Ty::Optional(_)) {
            self.diagnostics.push(
                Diagnostic::error("可选值未判空")
                    .with_span(expr.span)
                    .with_expected("先使用「匹配」处理「成功」与「失败」分支")
                    .with_actual(ty.to_string()),
            );
            return Ty::Error;
        }

        if matches!(&ty, Ty::Result { .. }) {
            self.diagnostics.push(
                Diagnostic::error("结果值未处理")
                    .with_span(expr.span)
                    .with_expected("使用「匹配」处理「成功」与「失败」分支")
                    .with_actual(ty.to_string()),
            );
            return Ty::Error;
        }

        ty
    }

    /// 检查算术操作数，并返回可继续参与运算的类型。
    fn numeric_operand(&mut self, expr: &Expr, ty: Ty) -> Option<Ty> {
        if ty.is_error() {
            None
        } else if ty.is_numeric() {
            Some(ty)
        } else {
            self.diagnostics.push(
                Diagnostic::type_mismatch(expr.span, "数字类型（整数或小数）", ty.to_string())
                    .with_note("算术运算只接受整数或小数"),
            );
            None
        }
    }
}

/// 语义分析阶段识别的语言级内置函数。
///
/// 这里不依赖 `li-ir`，以保持前端只依赖 AST 与诊断；IR 层会用自己的
/// [`li_ir::BuiltinFunction`] 表达同一组能力。
#[derive(Clone, Copy, Debug)]
enum SemaBuiltinFunction {
    ReadText,
    WriteText,
    NewDictionary,
}

impl SemaBuiltinFunction {
    fn from_name(name: &str) -> Option<Self> {
        let function = match name {
            "读取文本" => Self::ReadText,
            "写入文本" => Self::WriteText,
            "新建字典" => Self::NewDictionary,
            _ => return None,
        };
        Some(function)
    }

    fn signature(self) -> FunctionSignature {
        match self {
            Self::ReadText => FunctionSignature {
                params: vec![Ty::String],
                return_type: Ty::Result {
                    ok: Box::new(Ty::String),
                    error: Box::new(Ty::String),
                },
            },
            Self::WriteText => FunctionSignature {
                params: vec![Ty::String, Ty::String],
                return_type: Ty::Result {
                    ok: Box::new(Ty::Integer),
                    error: Box::new(Ty::String),
                },
            },
            Self::NewDictionary => FunctionSignature {
                params: Vec::new(),
                return_type: Ty::Dictionary {
                    key: Box::new(Ty::Error),
                    value: Box::new(Ty::Error),
                },
            },
        }
    }
}
