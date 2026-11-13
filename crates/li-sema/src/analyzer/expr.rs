//! 表达式类型推导。
//!
//! 所有运算都要求两侧类型完全一致；唯一的“包装”例外是
//! 值可以进入可选类型，成功值可以进入结果类型，见
//! [`Analyzer::is_compatible`]。

use li_ast::{BinaryOp, Expr, ExprKind, Ident, UnaryOp};
use li_errors::Diagnostic;

use super::Analyzer;
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
            Some(variable) => variable.ty.clone(),
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
                None => {
                    self.diagnostics.push(
                        Diagnostic::error(format!("未定义的函数「{name}」"))
                            .with_span(callee.span)
                            .with_expected("已声明的函数")
                            .with_actual(format!("`{name}`")),
                    );
                    None
                }
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

        signature.return_type
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
