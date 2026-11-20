//! 语句、控制流与匹配模式的语义检查。

use li_ast::{
    AssignStmt, Expr, ExprKind, Ident, IfStmt, LetStmt, Literal, MatchStmt, Pattern, PatternKind,
    PrintStmt, ReturnStmt, Stmt, UnsafeStmt, WhileStmt,
};
use li_errors::{Diagnostic, Span};

use super::Analyzer;
use crate::symbols::Variable;
use crate::ty::Ty;

impl Analyzer {
    /// 按语句种类分派检查。
    pub(super) fn analyze_statement(&mut self, statement: &Stmt) {
        match statement {
            Stmt::Let(stmt) => self.analyze_let(stmt),
            Stmt::Assign(stmt) => self.analyze_assign(stmt),
            Stmt::Expr(stmt) => {
                let ty = self.infer_expr(&stmt.expr);
                self.ensure_plain(&stmt.expr, ty);
            }
            Stmt::Return(stmt) => self.analyze_return(stmt),
            Stmt::If(stmt) => self.analyze_if(stmt),
            Stmt::While(stmt) => self.analyze_while(stmt),
            Stmt::Break(stmt) => {
                if self.loop_depth == 0 {
                    self.diagnostics.push(
                        Diagnostic::error("「跳出」只能用于循环内部")
                            .with_span(stmt.span)
                            .with_expected("位于「当」循环内部的语句")
                            .with_actual("循环外"),
                    );
                }
            }
            Stmt::Continue(stmt) => {
                if self.loop_depth == 0 {
                    self.diagnostics.push(
                        Diagnostic::error("「继续」只能用于循环内部")
                            .with_span(stmt.span)
                            .with_expected("位于「当」循环内部的语句")
                            .with_actual("循环外"),
                    );
                }
            }
            Stmt::Match(stmt) => self.analyze_match(stmt),
            Stmt::Unsafe(stmt) => self.analyze_unsafe(stmt),
            Stmt::Print(stmt) => self.analyze_print(stmt),
        }
    }

    fn analyze_let(&mut self, stmt: &LetStmt) {
        let initializer_ty = self.infer_expr(&stmt.initializer);

        let ty = if let Some(annotation) = &stmt.ty {
            let expected = self.resolve_type(annotation);
            if expected.is_raw_pointer() && self.unsafe_depth == 0 {
                self.diagnostics.push(
                    Diagnostic::error(format!(
                        "原生指针类型「{expected}」只能出现在「不安全」块或函数签名中"
                    ))
                    .with_span(annotation.span)
                    .with_expected("包裹在「不安全 { ... }」块内")
                    .with_actual("安全代码中的局部变量标注"),
                );
            }
            self.check_compatible(
                stmt.initializer.span,
                &expected,
                &initializer_ty,
                "变量初始化",
            );
            expected
        } else if initializer_ty == Ty::Null {
            self.report(
                stmt.initializer.span,
                "无法推导「空」的类型，请添加类型标注",
            );
            Ty::Error
        } else if matches!(
            &initializer_ty,
            Ty::Dictionary { key, value } if key.is_error() || value.is_error()
        ) {
            self.report(
                stmt.initializer.span,
                "无法推导字典的键值类型，请添加类型标注",
            );
            Ty::Error
        } else {
            initializer_ty
        };

        let declared = self.symbols.declare(
            &stmt.name.name,
            Variable {
                ty,
                mutable: stmt.mutable,
            },
        );

        if !declared {
            self.diagnostics.push(
                Diagnostic::error(format!("同一作用域内重复声明变量「{}」", stmt.name.name))
                    .with_span(stmt.name.span)
                    .with_expected("不同的变量名")
                    .with_actual(format!("`{}`", stmt.name.name)),
            );
        }
    }

    fn analyze_assign(&mut self, stmt: &AssignStmt) {
        let target_ty = self.infer_assign_target(&stmt.target);
        let value_ty = self.infer_expr(&stmt.value);

        if !target_ty.is_error() {
            self.check_compatible(stmt.value.span, &target_ty, &value_ty, "赋值");
        }
    }

    /// 检查赋值目标并返回其类型。
    fn infer_assign_target(&mut self, target: &Expr) -> Ty {
        match &target.kind {
            ExprKind::Identifier(ident) => match self.symbols.lookup(&ident.name).cloned() {
                Some(variable) => {
                    if !variable.mutable {
                        self.report_immutable(&ident.name, ident.span);
                    }
                    variable.ty
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
            },
            ExprKind::FieldAccess { .. } | ExprKind::Index { .. } => {
                let ty = self.infer_expr(target);
                self.check_root_mutable(target);
                ty
            }
            ExprKind::Parenthesized(inner) => self.infer_assign_target(inner),
            _ => {
                self.infer_expr(target);
                self.diagnostics.push(
                    Diagnostic::error("无效的赋值目标")
                        .with_span(target.span)
                        .with_expected("变量、字段或下标")
                        .with_actual("其他表达式"),
                );
                Ty::Error
            }
        }
    }

    /// 字段或下标赋值时，检查最内层变量是否可变。
    fn check_root_mutable(&mut self, target: &Expr) {
        match &target.kind {
            ExprKind::Identifier(ident) => {
                if let Some(variable) = self.symbols.lookup(&ident.name).cloned() {
                    if !variable.mutable {
                        self.report_immutable(&ident.name, ident.span);
                    }
                }
            }
            ExprKind::FieldAccess { object, .. } | ExprKind::Index { object, .. } => {
                self.check_root_mutable(object);
            }
            ExprKind::Parenthesized(inner) => self.check_root_mutable(inner),
            _ => {
                self.report(target.span, "赋值目标必须是可写变量");
            }
        }
    }

    fn report_immutable(&mut self, name: &str, span: Span) {
        // 按「注解 > 项目配置 > 默认」的优先级决定不可变性违规的等级。
        let level = self.level_for(li_errors::Rule::Immutability);
        if level.is_ignoring() {
            return;
        }

        let message = format!("「{name}」不可变，不能赋值；如需可变请用「让 可变」声明");
        if level.is_error() {
            self.diagnostics
                .push(Diagnostic::error(message).with_span(span));
        } else {
            self.diagnostics
                .push(Diagnostic::warning(message).with_span(span));
        }
    }

    /// 按等级生成诊断；`忽略` 等级完全静默，`报错` 等级为 Error，
    /// `警告` 等级为 Warning。
    #[allow(dead_code)]
    pub(super) fn report_rule(
        &mut self,
        rule: li_errors::Rule,
        span: Span,
        message: impl Into<String>,
    ) {
        let level = self.level_for(rule);
        if level.is_ignoring() {
            return;
        }

        if level.is_error() {
            self.diagnostics
                .push(Diagnostic::error(message).with_span(span));
        } else {
            self.diagnostics
                .push(Diagnostic::warning(message).with_span(span));
        }
    }

    fn analyze_print(&mut self, stmt: &PrintStmt) {
        let ty = self.infer_expr(&stmt.value);
        self.ensure_plain(&stmt.value, ty);
    }

    fn analyze_return(&mut self, stmt: &ReturnStmt) {
        let expected = self.current_return.clone().unwrap_or(Ty::Error);

        if let Some(value) = &stmt.value {
            let actual = self.infer_expr(value);
            if !expected.is_error() {
                self.check_compatible(value.span, &expected, &actual, "函数返回类型");
            }
        } else if !expected.is_error() {
            self.diagnostics.push(
                Diagnostic::error("缺少返回值")
                    .with_span(stmt.span)
                    .with_expected(expected.to_string())
                    .with_actual("返回;"),
            );
        }
    }

    fn analyze_if(&mut self, stmt: &IfStmt) {
        let condition_ty = self.infer_expr(&stmt.condition);
        self.expect_bool(&stmt.condition, condition_ty, "如果条件必须是布尔表达式");

        self.analyze_block(&stmt.then_branch);
        if let Some(else_branch) = &stmt.else_branch {
            self.analyze_block(else_branch);
        }
    }

    fn analyze_while(&mut self, stmt: &WhileStmt) {
        let condition_ty = self.infer_expr(&stmt.condition);
        self.expect_bool(&stmt.condition, condition_ty, "当循环条件必须是布尔表达式");

        self.loop_depth += 1;
        self.analyze_block(&stmt.body);
        self.loop_depth -= 1;
    }

    /// 检查不安全块：块内允许使用原生指针，离开后立即恢复安全检查。
    fn analyze_unsafe(&mut self, stmt: &UnsafeStmt) {
        self.unsafe_depth += 1;
        self.analyze_block(&stmt.body);
        self.unsafe_depth -= 1;
    }

    /// 条件表达式必须是 `布尔`；可选值或结果值先去重处理。
    fn expect_bool(&mut self, expr: &Expr, ty: Ty, note: &str) {
        let ty = self.ensure_plain(expr, ty);
        if ty.is_error() || ty == Ty::Bool {
            return;
        }

        self.diagnostics
            .push(Diagnostic::type_mismatch(expr.span, "布尔", ty.to_string()).with_note(note));
    }

    fn analyze_match(&mut self, stmt: &MatchStmt) {
        let scrutinee_ty = self.infer_expr(&stmt.value);
        let mut has_success = false;
        let mut has_failure = false;
        let mut catch_all = false;

        for arm in &stmt.arms {
            self.symbols.push_scope();
            self.analyze_pattern(
                &arm.pattern,
                &scrutinee_ty,
                &mut has_success,
                &mut has_failure,
                &mut catch_all,
            );
            self.analyze_block(&arm.body);
            self.symbols.pop_scope();
        }

        if stmt.arms.is_empty() {
            self.diagnostics.push(
                Diagnostic::error("匹配语句至少需要一个分支")
                    .with_span(stmt.span)
                    .with_expected("至少一个匹配分支")
                    .with_actual("没有分支"),
            );
        }

        let needs_both = matches!(&scrutinee_ty, Ty::Result { .. } | Ty::Optional(_));

        if needs_both && !catch_all && !scrutinee_ty.is_error() {
            if !has_success {
                self.report_missing_match_arm(stmt, "成功");
            }
            if !has_failure {
                self.report_missing_match_arm(stmt, "失败");
            }
        }
    }

    fn report_missing_match_arm(&mut self, stmt: &MatchStmt, pattern: &str) {
        self.diagnostics.push(
            Diagnostic::error(format!("匹配缺少「{pattern}」分支"))
                .with_span(stmt.span)
                .with_expected(format!("包含「{pattern}」的匹配分支"))
                .with_actual("分支不完整"),
        );
    }

    /// 检查单个匹配模式，并将绑定的名称放入当前分支作用域。
    fn analyze_pattern(
        &mut self,
        pattern: &Pattern,
        scrutinee: &Ty,
        has_success: &mut bool,
        has_failure: &mut bool,
        catch_all: &mut bool,
    ) {
        match &pattern.kind {
            PatternKind::Wildcard => {
                *catch_all = true;
            }
            PatternKind::Binding(ident) => {
                *catch_all = true;
                self.declare_pattern_binding(ident, scrutinee.clone());
            }
            PatternKind::Literal(literal) => {
                self.analyze_literal_pattern(pattern, literal, scrutinee);
                if matches!(literal, Literal::Null) && matches!(scrutinee, Ty::Optional(_)) {
                    *has_failure = true;
                }
            }
            PatternKind::Success(binding) => {
                if *has_success {
                    self.report(pattern.span, "重复的「成功」分支");
                } else {
                    *has_success = true;
                }

                let binding_ty = match scrutinee {
                    Ty::Optional(inner) => Some((**inner).clone()),
                    Ty::Result { ok, .. } => Some((**ok).clone()),
                    Ty::Error => Some(Ty::Error),
                    other => {
                        self.report_success_pattern_mismatch(pattern, other);
                        Some(Ty::Error)
                    }
                };

                if let (Some(ident), Some(ty)) = (binding, binding_ty) {
                    self.declare_pattern_binding(ident, ty);
                }
            }
            PatternKind::Failure(binding) => {
                if *has_failure {
                    self.report(pattern.span, "重复的「失败」分支");
                } else {
                    *has_failure = true;
                }

                match scrutinee {
                    Ty::Optional(_) => {
                        if let Some(ident) = binding {
                            self.diagnostics.push(
                                Diagnostic::error("可选类型的「失败」分支不携带值")
                                    .with_span(ident.span)
                                    .with_expected("「失败」")
                                    .with_actual(format!("「失败({})」", ident.name)),
                            );
                        }
                    }
                    Ty::Result { error, .. } => {
                        if let Some(ident) = binding {
                            self.declare_pattern_binding(ident, (**error).clone());
                        }
                    }
                    Ty::Error => {
                        if let Some(ident) = binding {
                            self.declare_pattern_binding(ident, Ty::Error);
                        }
                    }
                    other => {
                        self.report_failure_pattern_mismatch(pattern, other);
                        if let Some(ident) = binding {
                            self.declare_pattern_binding(ident, Ty::Error);
                        }
                    }
                }
            }
        }
    }

    fn analyze_literal_pattern(&mut self, pattern: &Pattern, literal: &Literal, scrutinee: &Ty) {
        if scrutinee.is_error() {
            return;
        }

        let literal_ty = Self::literal_type(literal);
        let optional_null =
            matches!(scrutinee, Ty::Optional(_)) && matches!(literal, Literal::Null);

        if !optional_null && &literal_ty != scrutinee {
            self.diagnostics.push(
                Diagnostic::type_mismatch(
                    pattern.span,
                    scrutinee.to_string(),
                    literal_ty.to_string(),
                )
                .with_note("匹配模式的类型必须与匹配值一致"),
            );
        }
    }

    fn report_success_pattern_mismatch(&mut self, pattern: &Pattern, actual: &Ty) {
        self.diagnostics.push(
            Diagnostic::error("匹配模式与值的类型不匹配")
                .with_span(pattern.span)
                .with_expected("「成功」模式只能用于 结果 或 可选 类型")
                .with_actual(actual.to_string()),
        );
    }

    fn report_failure_pattern_mismatch(&mut self, pattern: &Pattern, actual: &Ty) {
        self.diagnostics.push(
            Diagnostic::error("匹配模式与值的类型不匹配")
                .with_span(pattern.span)
                .with_expected("「失败」模式只能用于 结果 或 可选 类型")
                .with_actual(actual.to_string()),
        );
    }

    fn declare_pattern_binding(&mut self, ident: &Ident, ty: Ty) {
        let declared = self
            .symbols
            .declare(&ident.name, Variable { ty, mutable: false });

        if !declared {
            self.report(
                ident.span,
                format!("匹配分支重复声明绑定「{}」", ident.name),
            );
        }
    }
}
