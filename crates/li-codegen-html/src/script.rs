//! 视图交互代码到 JavaScript 的翻译。
//!
//! HTML 后端是视图专用例外：它直接消费 `.lui` 的视图 AST，不经过中立 IR。
//! 状态声明、事件体与绑定表达式都属于视图 AST 的一部分，因此这里提供
//! 一个受控的 li 子集翻译器，把语句与表达式内联进同一个 HTML 产物。
//!
//! 支持的语句：`让`、赋值、表达式语句、`打印`、`如果`、`当`、`跳出`、`继续`。
//! 支持的表达式：字面量、标识符、一元/二元运算、括号与 `打印(...)` 调用。
//! 其他结构会得到带行列号的中文诊断，不会被静默忽略。

use std::collections::HashSet;

use li_ast::{BinaryOp, Block, Expr, ExprKind, Literal, Stmt, UnaryOp};
use li_errors::{Diagnostic, DiagnosticBag};

/// 把视图中的状态、事件与绑定表达式翻译为 JavaScript。
pub(crate) struct ScriptTranslator<'a> {
    diagnostics: &'a mut DiagnosticBag,
    /// 作用域栈；每个元素是一层块级作用域中的名称集合。
    scopes: Vec<HashSet<String>>,
    /// 当前嵌套的循环层数，用于检查 `跳出` 与 `继续`。
    loop_depth: usize,
}

impl<'a> ScriptTranslator<'a> {
    pub(crate) fn new(diagnostics: &'a mut DiagnosticBag) -> Self {
        Self {
            diagnostics,
            scopes: vec![HashSet::new()],
            loop_depth: 0,
        }
    }

    /// 基于已声明的状态名称创建翻译器，用于绑定表达式与事件处理。
    pub(crate) fn with_names(diagnostics: &'a mut DiagnosticBag, names: HashSet<String>) -> Self {
        Self {
            diagnostics,
            scopes: vec![names],
            loop_depth: 0,
        }
    }

    /// 返回当前最外层作用域中的名称集合。
    pub(crate) fn names(&self) -> &HashSet<String> {
        self.scopes.first().expect("作用域栈至少包含全局层")
    }

    /// 声明视图状态变量，返回对应的 `let` 声明代码。
    pub(crate) fn declare_state(&mut self, state: &[Stmt]) -> String {
        let mut output = String::new();

        for stmt in state {
            let Stmt::Let(declaration) = stmt else {
                self.diagnostics.push(
                    Diagnostic::error("`状态` 块只能包含变量声明")
                        .with_span(stmt.span())
                        .with_expected("`让` 或 `让 可变` 声明")
                        .with_note("事件处理逻辑请写在节点的事件块中"),
                );
                continue;
            };

            if !self.declare(&declaration.name.name, declaration.name.span) {
                continue;
            }

            let Some(value) = self.expression(&declaration.initializer) else {
                continue;
            };

            output.push_str(&format!("let {} = {};\n", declaration.name.name, value));
        }

        output
    }

    /// 检查一个标识符是否已在状态或外层作用域中声明。
    pub(crate) fn is_declared(&self, name: &str) -> bool {
        self.scopes.iter().rev().any(|scope| scope.contains(name))
    }

    /// 翻译一个绑定表达式；只能引用状态变量。
    pub(crate) fn binding_expression(&mut self, expression: &Expr) -> Option<String> {
        self.expression(expression)
    }

    /// 翻译事件处理语句块，返回可直接嵌入函数的 JavaScript 代码。
    pub(crate) fn handler(&mut self, body: &Block) -> Option<String> {
        self.scopes.push(HashSet::new());
        let output = self.block(body);
        self.scopes.pop();
        output
    }

    /// 翻译语句块中的全部语句。
    fn block(&mut self, block: &Block) -> Option<String> {
        let mut output = String::new();
        for stmt in &block.stmts {
            output.push_str(&self.statement(stmt)?);
        }
        Some(output)
    }

    /// 翻译一条语句。
    fn statement(&mut self, stmt: &Stmt) -> Option<String> {
        match stmt {
            Stmt::Let(declaration) => {
                if !self.declare(&declaration.name.name, declaration.name.span) {
                    return None;
                }
                let value = self.expression(&declaration.initializer)?;
                Some(format!("let {} = {};\n", declaration.name.name, value))
            }
            Stmt::Assign(assignment) => {
                let ExprKind::Identifier(ident) = &assignment.target.kind else {
                    self.diagnostics.push(
                        Diagnostic::error("视图事件中的赋值目标必须是变量")
                            .with_span(assignment.target.span)
                            .with_note("暂不支持给字段或下标赋值"),
                    );
                    return None;
                };

                if !self.is_declared(&ident.name) {
                    self.report_undefined(ident);
                    return None;
                }

                let value = self.expression(&assignment.value)?;
                Some(format!("{} = {};\n", ident.name, value))
            }
            Stmt::Expr(statement) => {
                let expression = self.expression(&statement.expr)?;
                Some(format!("{expression};\n"))
            }
            Stmt::Print(statement) => {
                let value = self.expression(&statement.value)?;
                Some(format!("console.log(__li_显示({value}));\n"))
            }
            Stmt::If(statement) => {
                let condition = self.expression(&statement.condition)?;
                let then_branch = self.scoped_block(&statement.then_branch)?;

                let mut output = format!("if ({condition}) {{\n{then_branch}}}");
                if let Some(else_branch) = &statement.else_branch {
                    let else_code = self.scoped_block(else_branch)?;
                    output.push_str(&format!(" else {{\n{else_code}}}"));
                }
                output.push('\n');
                Some(output)
            }
            Stmt::While(statement) => {
                let condition = self.expression(&statement.condition)?;
                self.loop_depth += 1;
                let body = self.scoped_block(&statement.body);
                self.loop_depth -= 1;
                let body = body?;
                Some(format!("while ({condition}) {{\n{body}}}\n"))
            }
            Stmt::Break(statement) => {
                if self.loop_depth == 0 {
                    self.diagnostics.push(
                        Diagnostic::error("`跳出` 只能出现在循环内部").with_span(statement.span),
                    );
                    return None;
                }
                Some("break;\n".to_owned())
            }
            Stmt::Continue(statement) => {
                if self.loop_depth == 0 {
                    self.diagnostics.push(
                        Diagnostic::error("`继续` 只能出现在循环内部").with_span(statement.span),
                    );
                    return None;
                }
                Some("continue;\n".to_owned())
            }
            Stmt::Return(statement) => {
                self.unsupported("`返回`", statement.span);
                None
            }
            Stmt::Match(statement) => {
                self.unsupported("`匹配`", statement.span);
                None
            }
            Stmt::Unsafe(statement) => {
                self.diagnostics.push(
                    Diagnostic::error("视图交互代码不支持 `不安全` 块")
                        .with_span(statement.span)
                        .with_note("浏览器环境不提供底层内存操作"),
                );
                None
            }
        }
    }

    /// 在新作用域中翻译语句块。
    fn scoped_block(&mut self, block: &Block) -> Option<String> {
        self.scopes.push(HashSet::new());
        let output = self.block(block);
        self.scopes.pop();
        output
    }

    /// 翻译一个表达式。
    fn expression(&mut self, expression: &Expr) -> Option<String> {
        let code = match &expression.kind {
            ExprKind::Literal(literal) => literal_code(literal),
            ExprKind::Identifier(ident) => {
                if !self.is_declared(&ident.name) {
                    self.report_undefined(ident);
                    return None;
                }
                ident.name.clone()
            }
            ExprKind::Input => {
                self.unsupported("`输入` 表达式", expression.span);
                return None;
            }
            ExprKind::Unary { op, operand } => {
                let operand = self.expression(operand)?;
                match op {
                    UnaryOp::Negate => format!("(-{operand})"),
                    UnaryOp::Not => format!("(!{operand})"),
                }
            }
            ExprKind::Binary { left, op, right } => {
                let left = self.expression(left)?;
                let right = self.expression(right)?;
                let operator = match op {
                    BinaryOp::Add => "+",
                    BinaryOp::Subtract => "-",
                    BinaryOp::Multiply => "*",
                    BinaryOp::Divide => "/",
                    BinaryOp::Equal => "===",
                    BinaryOp::Less => "<",
                    BinaryOp::Greater => ">",
                    BinaryOp::And => "&&",
                    BinaryOp::Or => "||",
                };
                format!("({left} {operator} {right})")
            }
            ExprKind::Call { callee, args } => {
                let ExprKind::Identifier(ident) = &callee.kind else {
                    self.diagnostics.push(
                        Diagnostic::error("视图交互代码只支持按名称调用 `打印`")
                            .with_span(callee.span),
                    );
                    return None;
                };

                if ident.name != "打印" {
                    self.diagnostics.push(
                        Diagnostic::error(format!("视图交互代码暂不支持调用「{}」", ident.name))
                            .with_span(callee.span)
                            .with_expected("`打印(...)`")
                            .with_note("外部函数无法直接访问视图状态"),
                    );
                    return None;
                }

                let mut arguments = Vec::new();
                for argument in args {
                    arguments.push(self.expression(argument)?);
                }
                format!("console.log({})", arguments.join(", "))
            }
            ExprKind::FieldAccess { .. } => {
                self.unsupported("字段访问", expression.span);
                return None;
            }
            ExprKind::Index { .. } => {
                self.unsupported("下标访问", expression.span);
                return None;
            }
            ExprKind::ListLiteral(_) => {
                self.unsupported("列表字面量", expression.span);
                return None;
            }
            ExprKind::Parenthesized(inner) => format!("({})", self.expression(inner)?),
            ExprKind::Await(_) => {
                // 视图交互代码不支持等待；给出带行列的中文诊断。
                self.unsupported("`等待` 表达式", expression.span);
                return None;
            }
        };

        Some(code)
    }

    /// 在当前作用域声明一个名称；重复声明会报告诊断。
    fn declare(&mut self, name: &str, span: li_errors::Span) -> bool {
        let scope = self.scopes.last_mut().expect("作用域栈至少包含全局层");

        if !scope.insert(name.to_owned()) {
            self.diagnostics.push(
                Diagnostic::error(format!("变量「{name}」重复声明"))
                    .with_span(span)
                    .with_note("同一作用域内不能重复声明同名变量"),
            );
            return false;
        }

        true
    }

    fn report_undefined(&mut self, ident: &li_ast::Ident) {
        self.diagnostics.push(
            Diagnostic::error(format!("未定义的变量「{}」", ident.name))
                .with_span(ident.span)
                .with_expected("在 `状态 { ... }` 中声明的变量或事件内声明的局部变量")
                .with_actual(format!("`{}`", ident.name)),
        );
    }

    fn unsupported(&mut self, what: &str, span: li_errors::Span) {
        self.diagnostics.push(
            Diagnostic::error(format!("视图交互代码暂不支持{what}"))
                .with_span(span)
                .with_note("当前支持字面量、变量、运算、`打印`、`如果`、`当`、`跳出` 与 `继续`"),
        );
    }
}

/// 把字面量转换为 JavaScript 字面量。
fn literal_code(literal: &Literal) -> String {
    match literal {
        Literal::Integer(value) => value.to_string(),
        Literal::Float(value) => {
            let text = value.to_string();
            if text.contains('.') || text.contains('e') || text.contains('E') {
                text
            } else {
                format!("{text}.0")
            }
        }
        Literal::String(value) => format!("\"{}\"", escape_js_string(value)),
        Literal::Bool(value) => value.to_string(),
        Literal::Null => "null".to_owned(),
    }
}

/// 转义嵌入 JavaScript 双引号字符串的内容。
///
/// 同时转义 `<`，避免字符串中出现 `</script>` 提前结束内联脚本。
pub(crate) fn escape_js_string(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());

    for ch in text.chars() {
        match ch {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            '<' => escaped.push_str("\\u003c"),
            '\u{2028}' => escaped.push_str("\\u2028"),
            '\u{2029}' => escaped.push_str("\\u2029"),
            other => escaped.push(other),
        }
    }

    escaped
}
