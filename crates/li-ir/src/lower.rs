//! 把 AST 降级为中立 IR。
//!
//! 降级过程完成三件事：
//! - 把表达式拆成三地址码，所有中间结果都有显式类型；
//! - 把 `如果`、`当`、`跳出`、`继续`、`返回` 转换为基本块之间的
//!   `LABEL`、`JUMP`、`BRANCH` 与 `RETURN`；
//! - 记录局部变量、参数、结构体字段和函数签名，供任意后端消费。

use std::collections::{HashMap, HashSet};

use li_ast::{
    AssignStmt, BinaryOp as AstBinaryOp, Block, Expr, ExprKind, FuncDecl, IfStmt, Item, LetStmt,
    Literal, MatchStmt, Pattern, PatternKind, PrintStmt, Program as AstProgram, ReturnStmt, Stmt,
    Type, TypeDecl, TypeKind, UnaryOp as AstUnaryOp, WhileStmt,
};
use li_errors::{Diagnostic, DiagnosticBag, Span};

use crate::ir::{
    BasicBlock, BinaryOp, Constant, Function, Instruction, IrType, Label, Local, Operand,
    Parameter, Place, Program, StructDecl, StructField, TempId, Terminator, UnaryOp,
};

/// 对整份 AST 程序执行降级。
///
/// 成功时返回中立 IR；遇到未知类型等无法安全降级的问题时，
/// 返回包含全部中文诊断的 [`DiagnosticBag`]。
pub fn lower(program: &AstProgram) -> Result<Program, DiagnosticBag> {
    let mut lowerer = Lowerer::new();
    let ir = lowerer.lower_program(program);

    if lowerer.diagnostics.has_errors() {
        Err(lowerer.diagnostics)
    } else {
        Ok(ir)
    }
}

/// 全局声明信息与类型上下文。
#[derive(Default)]
struct TypeContext {
    structs: HashMap<String, StructLayout>,
    functions: HashMap<String, Signature>,
}

impl TypeContext {
    /// 把 AST 类型解析为 IR 类型；未知类型降级为 `Unknown`。
    fn resolve_type(&self, ty: &Type) -> IrType {
        match &ty.kind {
            TypeKind::Integer => IrType::Integer,
            TypeKind::Float => IrType::Float,
            TypeKind::Bool => IrType::Bool,
            TypeKind::String => IrType::String,
            TypeKind::List(element) => IrType::List(Box::new(self.resolve_type(element))),
            TypeKind::Optional(inner) => IrType::Optional(Box::new(self.resolve_type(inner))),
            TypeKind::Result { value, error } => IrType::Result {
                ok: Box::new(self.resolve_type(value)),
                error: Box::new(self.resolve_type(error)),
            },
            TypeKind::Named(ident) => {
                if self.structs.contains_key(&ident.name) {
                    IrType::Struct(ident.name.clone())
                } else {
                    IrType::Unknown
                }
            }
        }
    }
}

/// 结构体布局。
#[derive(Clone, Debug, Default)]
struct StructLayout {
    fields: Vec<StructField>,
}

impl StructLayout {
    fn field_type(&self, name: &str) -> Option<&IrType> {
        self.fields
            .iter()
            .find(|field| field.name == name)
            .map(|field| &field.ty)
    }
}

/// 函数签名。
#[derive(Clone, Debug)]
struct Signature {
    params: Vec<IrType>,
    return_type: IrType,
}

/// 降级器的全局状态。
struct Lowerer {
    context: TypeContext,
    diagnostics: DiagnosticBag,
}

impl Lowerer {
    fn new() -> Self {
        Self {
            context: TypeContext::default(),
            diagnostics: DiagnosticBag::new(),
        }
    }

    fn lower_program(&mut self, program: &AstProgram) -> Program {
        let mut functions = Vec::new();
        let mut structs = Vec::new();
        collect_declarations(&program.items, &mut functions, &mut structs);

        self.register_structs(&structs);
        self.register_functions(&functions);

        let mut seen_structs = HashSet::new();
        let ir_structs = structs
            .iter()
            .filter(|declaration| seen_structs.insert(declaration.name.name.clone()))
            .filter_map(|declaration| {
                self.context
                    .structs
                    .get(&declaration.name.name)
                    .map(|layout| StructDecl {
                        name: declaration.name.name.clone(),
                        fields: layout.fields.clone(),
                    })
            })
            .collect();

        let mut ir_functions = Vec::new();
        for declaration in &functions {
            let context = &self.context;
            let diagnostics = &mut self.diagnostics;
            let mut lowerer = FunctionLowerer::new(context, diagnostics, declaration);
            lowerer.lower(&declaration.body);
            ir_functions.push(lowerer.finish());
        }

        Program {
            structs: ir_structs,
            functions: ir_functions,
        }
    }

    fn register_structs(&mut self, declarations: &[&TypeDecl]) {
        let mut seen = HashSet::new();

        for declaration in declarations {
            if seen.insert(declaration.name.name.clone()) {
                self.context
                    .structs
                    .insert(declaration.name.name.clone(), StructLayout::default());
            } else {
                self.report(
                    declaration.name.span,
                    format!("重复声明类型「{}」", declaration.name.name),
                );
            }
        }

        let mut processed = HashSet::new();
        for declaration in declarations {
            if !processed.insert(declaration.name.name.clone()) {
                continue;
            }

            if !self.context.structs.contains_key(&declaration.name.name) {
                continue;
            }
            let mut fields = Vec::new();

            for field in &declaration.fields {
                let ty = self.resolve_type(&field.ty);

                if fields
                    .iter()
                    .any(|existing: &StructField| existing.name == field.name.name)
                {
                    self.report(
                        field.name.span,
                        format!(
                            "结构体「{}」重复声明字段「{}」",
                            declaration.name.name, field.name.name
                        ),
                    );
                } else {
                    fields.push(StructField {
                        name: field.name.name.clone(),
                        ty,
                    });
                }
            }

            self.context
                .structs
                .insert(declaration.name.name.clone(), StructLayout { fields });
        }
    }

    fn register_functions(&mut self, declarations: &[&FuncDecl]) {
        for declaration in declarations {
            if self.context.functions.contains_key(&declaration.name.name) {
                self.report(
                    declaration.name.span,
                    format!("重复声明函数「{}」", declaration.name.name),
                );
                continue;
            }

            let params = declaration
                .params
                .iter()
                .map(|param| self.resolve_type(&param.ty))
                .collect();
            let return_type = self.resolve_type(&declaration.return_type);

            self.context.functions.insert(
                declaration.name.name.clone(),
                Signature {
                    params,
                    return_type,
                },
            );
        }
    }

    /// 解析类型，并在遇到未知具名类型时生成中文诊断。
    fn resolve_type(&mut self, ty: &Type) -> IrType {
        match &ty.kind {
            TypeKind::Integer => IrType::Integer,
            TypeKind::Float => IrType::Float,
            TypeKind::Bool => IrType::Bool,
            TypeKind::String => IrType::String,
            TypeKind::List(element) => IrType::List(Box::new(self.resolve_type(element))),
            TypeKind::Optional(inner) => IrType::Optional(Box::new(self.resolve_type(inner))),
            TypeKind::Result { value, error } => IrType::Result {
                ok: Box::new(self.resolve_type(value)),
                error: Box::new(self.resolve_type(error)),
            },
            TypeKind::Named(ident) => {
                if self.context.structs.contains_key(&ident.name) {
                    IrType::Struct(ident.name.clone())
                } else {
                    self.report(ident.span, format!("未知类型「{}」", ident.name));
                    IrType::Unknown
                }
            }
        }
    }

    fn report(&mut self, span: Span, message: impl Into<String>) {
        self.diagnostics
            .push(Diagnostic::error(message).with_span(span));
    }
}

/// 递归收集顶层与模块内的函数、结构体声明。
fn collect_declarations<'a>(
    items: &'a [Item],
    functions: &mut Vec<&'a FuncDecl>,
    structs: &mut Vec<&'a TypeDecl>,
) {
    for item in items {
        match item {
            Item::Function(function) => functions.push(function),
            Item::Type(declaration) => structs.push(declaration),
            Item::Module(module) => collect_declarations(&module.items, functions, structs),
            Item::Import(_) | Item::View(_) => {}
        }
    }
}

/// 一个带静态类型的 IR 操作数。
#[derive(Clone, Debug)]
struct TypedValue {
    operand: Operand,
    ty: IrType,
}

/// 变量绑定。
#[derive(Clone, Debug)]
struct VarBinding {
    name: String,
    ty: IrType,
}

/// 循环的跳出与继续目标。
#[derive(Clone, Copy, Debug)]
struct LoopTargets {
    break_label: Label,
    continue_label: Label,
}

/// 单个函数的降级状态。
struct FunctionLowerer<'a> {
    context: &'a TypeContext,
    diagnostics: &'a mut DiagnosticBag,
    name: String,
    function_span: Span,
    params: Vec<Parameter>,
    return_type: IrType,
    locals: Vec<Local>,
    blocks: Vec<BasicBlock>,
    current: Option<usize>,
    next_label: usize,
    next_temp: usize,
    used_local_names: HashSet<String>,
    scopes: Vec<HashMap<String, VarBinding>>,
    loops: Vec<LoopTargets>,
}

impl<'a> FunctionLowerer<'a> {
    fn new(
        context: &'a TypeContext,
        diagnostics: &'a mut DiagnosticBag,
        declaration: &FuncDecl,
    ) -> Self {
        let mut lowerer = Self {
            context,
            diagnostics,
            name: declaration.name.name.clone(),
            function_span: declaration.span,
            params: Vec::new(),
            return_type: context.resolve_type(&declaration.return_type),
            locals: Vec::new(),
            blocks: Vec::new(),
            current: None,
            next_label: 0,
            next_temp: 0,
            used_local_names: HashSet::new(),
            scopes: Vec::new(),
            loops: Vec::new(),
        };

        lowerer.push_scope();

        for param in &declaration.params {
            let ty = context.resolve_type(&param.ty);
            let name = lowerer.fresh_local_name(&param.name.name);
            if let Some(scope) = lowerer.scopes.last_mut() {
                scope.insert(
                    param.name.name.clone(),
                    VarBinding {
                        name: name.clone(),
                        ty: ty.clone(),
                    },
                );
            }
            lowerer.params.push(Parameter { name, ty });
        }

        let entry = Label(0);
        lowerer.next_label = 1;
        lowerer.blocks.push(BasicBlock {
            label: entry,
            instructions: Vec::new(),
            instruction_spans: Vec::new(),
            terminator: Terminator::Return(None),
            terminator_span: declaration.span,
        });
        lowerer.current = Some(0);

        lowerer
    }

    fn lower(&mut self, body: &Block) {
        self.lower_block(body);
        if self.current.is_some() {
            self.terminate(Terminator::Return(None), body.span);
        }
    }

    fn finish(mut self) -> Function {
        if self.current.is_some() {
            let span = self.function_span;
            self.terminate(Terminator::Return(None), span);
        }

        Function {
            name: self.name,
            params: self.params,
            return_type: self.return_type,
            locals: self.locals,
            blocks: self.blocks,
            entry: Label(0),
        }
    }

    fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    fn lookup(&self, name: &str) -> Option<VarBinding> {
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name).cloned())
    }

    /// 生成函数内唯一的局部变量名。
    ///
    /// li 标识符不允许 `$`，因此 `$2`、`$3` 后缀不会与源码变量冲突。
    fn fresh_local_name(&mut self, source: &str) -> String {
        if self.used_local_names.insert(source.to_owned()) {
            return source.to_owned();
        }

        let mut index = 2;
        loop {
            let candidate = format!("{source}${index}");
            if self.used_local_names.insert(candidate.clone()) {
                return candidate;
            }
            index += 1;
        }
    }

    fn declare_local(&mut self, source: &str, ty: IrType, mutable: bool) -> String {
        let name = self.fresh_local_name(source);
        self.locals.push(Local {
            name: name.clone(),
            ty: ty.clone(),
            mutable,
        });

        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(
                source.to_owned(),
                VarBinding {
                    name: name.clone(),
                    ty,
                },
            );
        }

        name
    }

    fn new_temp(&mut self) -> Place {
        let id = TempId(self.next_temp);
        self.next_temp += 1;
        Place::Temp(id)
    }

    fn emit(&mut self, instruction: Instruction, span: Span) {
        if let Some(index) = self.current {
            if let Some(block) = self.blocks.get_mut(index) {
                block.instructions.push(instruction);
                block.instruction_spans.push(span);
            }
        }
    }

    fn start_block(&mut self, label: Label) {
        self.blocks.push(BasicBlock {
            label,
            instructions: Vec::new(),
            instruction_spans: Vec::new(),
            terminator: Terminator::Return(None),
            terminator_span: self.function_span,
        });
        self.current = Some(self.blocks.len() - 1);
    }

    fn terminate(&mut self, terminator: Terminator, span: Span) {
        if let Some(index) = self.current.take() {
            if let Some(block) = self.blocks.get_mut(index) {
                block.terminator = terminator;
                block.terminator_span = span;
            }
        }
    }

    fn lower_block(&mut self, block: &Block) {
        self.push_scope();

        for statement in &block.stmts {
            if self.current.is_none() {
                break;
            }
            self.lower_statement(statement);
        }

        self.pop_scope();
    }

    fn lower_statement(&mut self, statement: &Stmt) {
        match statement {
            Stmt::Let(stmt) => self.lower_let(stmt),
            Stmt::Assign(stmt) => self.lower_assign(stmt),
            Stmt::Expr(stmt) => {
                self.lower_expr(&stmt.expr);
            }
            Stmt::Return(stmt) => self.lower_return(stmt),
            Stmt::If(stmt) => self.lower_if(stmt),
            Stmt::While(stmt) => self.lower_while(stmt),
            Stmt::Break(stmt) => self.lower_break(stmt.span),
            Stmt::Continue(stmt) => self.lower_continue(stmt.span),
            Stmt::Match(stmt) => self.lower_match(stmt),
            Stmt::Print(stmt) => self.lower_print(stmt),
        }
    }

    fn lower_let(&mut self, statement: &LetStmt) {
        let value = self.lower_expr(&statement.initializer);
        let declared_ty = statement
            .ty
            .as_ref()
            .map(|ty| self.context.resolve_type(ty))
            .unwrap_or_else(|| value.ty.clone());
        let name = self.declare_local(&statement.name.name, declared_ty.clone(), statement.mutable);
        self.emit_coerced_copy(Place::Local(name), value, &declared_ty, statement.span);
    }

    fn lower_assign(&mut self, statement: &AssignStmt) {
        self.lower_assignment(&statement.target, &statement.value, statement.span);
    }

    fn lower_assignment(&mut self, target: &Expr, value: &Expr, span: Span) {
        match &target.kind {
            ExprKind::Identifier(ident) => {
                let Some(binding) = self.lookup(&ident.name) else {
                    self.report(ident.span, format!("未定义的变量「{}」", ident.name));
                    self.lower_expr(value);
                    return;
                };

                let value = self.lower_expr(value);
                self.emit_coerced_copy(Place::Local(binding.name), value, &binding.ty, span);
            }
            ExprKind::FieldAccess { object, field } => {
                let object_value = self.lower_expr(object);
                let field_ty = self.field_type(&object_value.ty, field, field.span);
                let value = self.lower_expr(value);
                let value = self.coerce_to_operand(value, &field_ty, span);
                self.emit(
                    Instruction::SetField {
                        object: object_value.operand,
                        field: field.name.clone(),
                        value,
                    },
                    span,
                );
            }
            ExprKind::Index { object, index } => {
                let object_value = self.lower_expr(object);
                let index_value = self.lower_expr(index);
                let element_ty = match &object_value.ty {
                    IrType::List(element) => (**element).clone(),
                    _ => IrType::Unknown,
                };
                let value = self.lower_expr(value);
                let value = self.coerce_to_operand(value, &element_ty, span);
                self.emit(
                    Instruction::SetIndex {
                        object: object_value.operand,
                        index: index_value.operand,
                        value,
                    },
                    span,
                );
            }
            ExprKind::Parenthesized(inner) => self.lower_assignment(inner, value, span),
            _ => {
                self.lower_expr(target);
                self.lower_expr(value);
                self.report(target.span, "无效的赋值目标");
            }
        }
    }

    fn lower_return(&mut self, statement: &ReturnStmt) {
        match &statement.value {
            Some(value) => {
                let value = self.lower_expr(value);
                let expected = self.return_type.clone();
                let value = self.coerce_to_operand(value, &expected, statement.span);
                self.terminate(Terminator::Return(Some(value)), statement.span);
            }
            None => self.terminate(Terminator::Return(None), statement.span),
        }
    }

    fn lower_if(&mut self, statement: &IfStmt) {
        let condition = self.lower_expr(&statement.condition);
        let then_label = self.new_label();
        let else_label = statement.else_branch.as_ref().map(|_| self.new_label());
        let join_label = self.new_label();
        let else_target = else_label.unwrap_or(join_label);

        self.terminate(
            Terminator::Branch {
                condition: condition.operand,
                then_label,
                else_label: else_target,
            },
            statement.span,
        );

        self.start_block(then_label);
        self.lower_block(&statement.then_branch);
        if self.current.is_some() {
            self.terminate(Terminator::Jump(join_label), statement.then_branch.span);
        }

        if let Some(else_branch) = &statement.else_branch {
            self.start_block(else_label.unwrap_or(join_label));
            self.lower_block(else_branch);
            if self.current.is_some() {
                self.terminate(Terminator::Jump(join_label), else_branch.span);
            }
        }

        self.start_block(join_label);
    }

    fn lower_while(&mut self, statement: &WhileStmt) {
        let header = self.new_label();
        let body = self.new_label();
        let exit = self.new_label();

        self.terminate(Terminator::Jump(header), statement.span);
        self.start_block(header);
        let condition = self.lower_expr(&statement.condition);
        self.terminate(
            Terminator::Branch {
                condition: condition.operand,
                then_label: body,
                else_label: exit,
            },
            statement.condition.span,
        );

        self.start_block(body);
        self.loops.push(LoopTargets {
            break_label: exit,
            continue_label: header,
        });
        self.lower_block(&statement.body);
        self.loops.pop();
        if self.current.is_some() {
            self.terminate(Terminator::Jump(header), statement.body.span);
        }

        self.start_block(exit);
    }

    fn lower_break(&mut self, span: Span) {
        match self.loops.last() {
            Some(targets) => self.terminate(Terminator::Jump(targets.break_label), span),
            None => self.report(span, "「跳出」只能用于循环内部"),
        }
    }

    fn lower_continue(&mut self, span: Span) {
        match self.loops.last() {
            Some(targets) => self.terminate(Terminator::Jump(targets.continue_label), span),
            None => self.report(span, "「继续」只能用于循环内部"),
        }
    }

    fn lower_print(&mut self, statement: &PrintStmt) {
        let value = self.lower_expr(&statement.value);
        self.emit(
            Instruction::Print {
                value: value.operand,
            },
            statement.span,
        );
    }

    fn lower_match(&mut self, statement: &MatchStmt) {
        let scrutinee = self.lower_expr(&statement.value);
        let join = self.new_label();
        let arm_labels: Vec<Label> = statement.arms.iter().map(|_| self.new_label()).collect();

        let mut checks_reachable = true;
        for (index, arm) in statement.arms.iter().enumerate() {
            if !checks_reachable || self.current.is_none() {
                break;
            }

            match &arm.pattern.kind {
                PatternKind::Wildcard | PatternKind::Binding(_) => {
                    self.terminate(Terminator::Jump(arm_labels[index]), arm.span);
                    checks_reachable = false;
                }
                PatternKind::Literal(literal) => {
                    let condition =
                        if matches!(literal, Literal::Null) && scrutinee.ty.is_optional() {
                            let success = self.emit_is_success(scrutinee.operand.clone(), arm.span);
                            self.emit_unary(UnaryOp::Not, success, IrType::Bool, arm.span)
                        } else {
                            let constant = Operand::Constant(constant_from_literal(literal));
                            self.emit_binary(
                                BinaryOp::Equal,
                                scrutinee.operand.clone(),
                                constant,
                                IrType::Bool,
                                arm.span,
                            )
                        };
                    let next = self.new_label();
                    self.terminate(
                        Terminator::Branch {
                            condition,
                            then_label: arm_labels[index],
                            else_label: next,
                        },
                        arm.span,
                    );
                    self.start_block(next);
                }
                PatternKind::Success(_) | PatternKind::Failure(_) => {
                    let success = self.emit_is_success(scrutinee.operand.clone(), arm.span);
                    let condition = if matches!(arm.pattern.kind, PatternKind::Failure(_)) {
                        self.emit_unary(UnaryOp::Not, success, IrType::Bool, arm.span)
                    } else {
                        success
                    };
                    let next = self.new_label();
                    self.terminate(
                        Terminator::Branch {
                            condition,
                            then_label: arm_labels[index],
                            else_label: next,
                        },
                        arm.span,
                    );
                    self.start_block(next);
                }
            }
        }

        if self.current.is_some() {
            self.terminate(Terminator::Jump(join), statement.span);
        }

        for (index, arm) in statement.arms.iter().enumerate() {
            self.start_block(arm_labels[index]);
            self.push_scope();
            self.bind_pattern(&arm.pattern, &scrutinee);
            self.lower_block(&arm.body);
            self.pop_scope();
            if self.current.is_some() {
                self.terminate(Terminator::Jump(join), arm.span);
            }
        }

        self.start_block(join);
    }

    fn bind_pattern(&mut self, pattern: &Pattern, value: &TypedValue) {
        match &pattern.kind {
            PatternKind::Wildcard | PatternKind::Literal(_) => {}
            PatternKind::Binding(ident) => {
                let name = self.declare_local(&ident.name, value.ty.clone(), false);
                self.emit(
                    Instruction::Copy {
                        dest: Place::Local(name),
                        src: value.operand.clone(),
                        ty: value.ty.clone(),
                    },
                    pattern.span,
                );
            }
            PatternKind::Success(binding) => {
                if let Some(ident) = binding {
                    let (ty, operand) = if let Some(inner) = value.ty.optional_inner() {
                        let inner = inner.clone();
                        let operand =
                            self.emit_unwrap(value.operand.clone(), inner.clone(), pattern.span);
                        (inner, operand)
                    } else if let Some(ok) = value.ty.result_ok() {
                        let ok = ok.clone();
                        let operand =
                            self.emit_unwrap(value.operand.clone(), ok.clone(), pattern.span);
                        (ok, operand)
                    } else {
                        (IrType::Unknown, value.operand.clone())
                    };

                    let name = self.declare_local(&ident.name, ty.clone(), false);
                    self.emit(
                        Instruction::Copy {
                            dest: Place::Local(name),
                            src: operand,
                            ty,
                        },
                        pattern.span,
                    );
                }
            }
            PatternKind::Failure(binding) => {
                if let Some(ident) = binding {
                    let (ty, operand) = if let Some(error) = value.ty.result_error() {
                        let error = error.clone();
                        let operand = self.emit_unwrap_error(
                            value.operand.clone(),
                            error.clone(),
                            pattern.span,
                        );
                        (error, operand)
                    } else {
                        (IrType::Unknown, value.operand.clone())
                    };

                    let name = self.declare_local(&ident.name, ty.clone(), false);
                    self.emit(
                        Instruction::Copy {
                            dest: Place::Local(name),
                            src: operand,
                            ty,
                        },
                        pattern.span,
                    );
                }
            }
        }
    }

    fn lower_expr(&mut self, expr: &Expr) -> TypedValue {
        match &expr.kind {
            ExprKind::Literal(literal) => {
                let constant = constant_from_literal(literal);
                TypedValue {
                    ty: constant.ty(),
                    operand: Operand::Constant(constant),
                }
            }
            ExprKind::Identifier(ident) => match self.lookup(&ident.name) {
                Some(binding) => TypedValue {
                    operand: Operand::Place(Place::Local(binding.name)),
                    ty: binding.ty,
                },
                None => {
                    self.report(ident.span, format!("未定义的变量「{}」", ident.name));
                    TypedValue {
                        operand: Operand::Constant(Constant::Null),
                        ty: IrType::Unknown,
                    }
                }
            },
            ExprKind::Input => {
                let dest = self.new_temp();
                self.emit(
                    Instruction::Input {
                        dest: dest.clone(),
                        ty: IrType::String,
                    },
                    expr.span,
                );
                TypedValue {
                    operand: Operand::Place(dest),
                    ty: IrType::String,
                }
            }
            ExprKind::Unary { op, operand } => {
                let operand = self.lower_expr(operand);
                let (op, ty) = match op {
                    AstUnaryOp::Negate => (UnaryOp::Negate, operand.ty.clone()),
                    AstUnaryOp::Not => (UnaryOp::Not, IrType::Bool),
                };
                let result = self.emit_unary(op, operand.operand, ty.clone(), expr.span);
                TypedValue {
                    operand: result,
                    ty,
                }
            }
            ExprKind::Binary { left, op, right } => {
                let left = self.lower_expr(left);
                let right = self.lower_expr(right);
                let result_ty = match op {
                    AstBinaryOp::Add
                    | AstBinaryOp::Subtract
                    | AstBinaryOp::Multiply
                    | AstBinaryOp::Divide => left.ty.clone(),
                    _ => IrType::Bool,
                };
                let op = match op {
                    AstBinaryOp::Add => BinaryOp::Add,
                    AstBinaryOp::Subtract => BinaryOp::Subtract,
                    AstBinaryOp::Multiply => BinaryOp::Multiply,
                    AstBinaryOp::Divide => BinaryOp::Divide,
                    AstBinaryOp::Equal => BinaryOp::Equal,
                    AstBinaryOp::Less => BinaryOp::Less,
                    AstBinaryOp::Greater => BinaryOp::Greater,
                    AstBinaryOp::And => BinaryOp::And,
                    AstBinaryOp::Or => BinaryOp::Or,
                };
                let operand = self.emit_binary(
                    op,
                    left.operand,
                    right.operand,
                    result_ty.clone(),
                    expr.span,
                );
                TypedValue {
                    operand,
                    ty: result_ty,
                }
            }
            ExprKind::Call { callee, args } => self.lower_call(callee, args, expr.span),
            ExprKind::FieldAccess { object, field } => {
                let object = self.lower_expr(object);
                let ty = self.field_type(&object.ty, field, field.span);
                let dest = self.new_temp();
                self.emit(
                    Instruction::GetField {
                        dest: dest.clone(),
                        object: object.operand,
                        field: field.name.clone(),
                        ty: ty.clone(),
                    },
                    expr.span,
                );
                TypedValue {
                    operand: Operand::Place(dest),
                    ty,
                }
            }
            ExprKind::Index { object, index } => {
                let object = self.lower_expr(object);
                let index = self.lower_expr(index);
                let ty = match &object.ty {
                    IrType::List(element) => (**element).clone(),
                    _ => IrType::Unknown,
                };
                let dest = self.new_temp();
                self.emit(
                    Instruction::GetIndex {
                        dest: dest.clone(),
                        object: object.operand,
                        index: index.operand,
                        ty: ty.clone(),
                    },
                    expr.span,
                );
                TypedValue {
                    operand: Operand::Place(dest),
                    ty,
                }
            }
            ExprKind::ListLiteral(items) => {
                let mut elements = Vec::with_capacity(items.len());
                let mut element_ty = IrType::Unknown;

                for item in items {
                    let item = self.lower_expr(item);
                    if matches!(element_ty, IrType::Unknown) {
                        element_ty = item.ty.clone();
                    }
                    elements.push(item.operand);
                }

                let ty = IrType::List(Box::new(element_ty));
                let dest = self.new_temp();
                self.emit(
                    Instruction::MakeList {
                        dest: dest.clone(),
                        elements,
                        ty: ty.clone(),
                    },
                    expr.span,
                );
                TypedValue {
                    operand: Operand::Place(dest),
                    ty,
                }
            }
            ExprKind::Parenthesized(inner) => self.lower_expr(inner),
        }
    }

    fn lower_call(&mut self, callee: &Expr, args: &[Expr], span: Span) -> TypedValue {
        let function_name = match &callee.kind {
            ExprKind::Identifier(ident) => Some(ident.name.clone()),
            _ => {
                self.report(callee.span, "不支持的函数调用形式");
                None
            }
        };

        let signature = function_name
            .as_ref()
            .and_then(|name| self.context.functions.get(name).cloned());

        if let Some(name) = &function_name {
            if signature.is_none() {
                self.report(callee.span, format!("未定义的函数「{name}」"));
            }
        }

        let mut arguments = Vec::with_capacity(args.len());
        for (index, argument) in args.iter().enumerate() {
            let value = self.lower_expr(argument);
            let value = if let Some(expected) = signature
                .as_ref()
                .and_then(|signature| signature.params.get(index))
            {
                self.coerce_to_operand(value, expected, argument.span)
            } else {
                value.operand
            };
            arguments.push(value);
        }

        let return_type = signature
            .as_ref()
            .map(|signature| signature.return_type.clone())
            .unwrap_or(IrType::Unknown);
        let dest = self.new_temp();
        self.emit(
            Instruction::Call {
                dest: Some(dest.clone()),
                function: function_name.unwrap_or_else(|| "<未知函数>".to_owned()),
                args: arguments,
                ty: return_type.clone(),
            },
            span,
        );

        TypedValue {
            operand: Operand::Place(dest),
            ty: return_type,
        }
    }

    fn field_type(&mut self, object_ty: &IrType, field: &li_ast::Ident, span: Span) -> IrType {
        match object_ty {
            IrType::Struct(name) => match self
                .context
                .structs
                .get(name)
                .and_then(|layout| layout.field_type(&field.name))
            {
                Some(ty) => ty.clone(),
                None => {
                    self.report(span, format!("结构体「{name}」没有字段「{}」", field.name));
                    IrType::Unknown
                }
            },
            IrType::Unknown => IrType::Unknown,
            other => {
                self.report(span, format!("不能访问字段：类型「{other}」不是结构体"));
                IrType::Unknown
            }
        }
    }

    fn emit_binary(
        &mut self,
        op: BinaryOp,
        left: Operand,
        right: Operand,
        ty: IrType,
        span: Span,
    ) -> Operand {
        let dest = self.new_temp();
        self.emit(
            Instruction::Binary {
                dest: dest.clone(),
                op,
                left,
                right,
                ty: ty.clone(),
            },
            span,
        );
        Operand::Place(dest)
    }

    fn emit_unary(&mut self, op: UnaryOp, operand: Operand, ty: IrType, span: Span) -> Operand {
        let dest = self.new_temp();
        self.emit(
            Instruction::Unary {
                dest: dest.clone(),
                op,
                operand,
                ty: ty.clone(),
            },
            span,
        );
        Operand::Place(dest)
    }

    fn emit_is_success(&mut self, value: Operand, span: Span) -> Operand {
        let dest = self.new_temp();
        self.emit(
            Instruction::IsSuccess {
                dest: dest.clone(),
                value,
                ty: IrType::Bool,
            },
            span,
        );
        Operand::Place(dest)
    }

    fn emit_unwrap(&mut self, value: Operand, ty: IrType, span: Span) -> Operand {
        let dest = self.new_temp();
        self.emit(
            Instruction::Unwrap {
                dest: dest.clone(),
                value,
                ty: ty.clone(),
            },
            span,
        );
        Operand::Place(dest)
    }

    fn emit_unwrap_error(&mut self, value: Operand, ty: IrType, span: Span) -> Operand {
        let dest = self.new_temp();
        self.emit(
            Instruction::UnwrapError {
                dest: dest.clone(),
                value,
                ty: ty.clone(),
            },
            span,
        );
        Operand::Place(dest)
    }

    fn emit_coerced_copy(&mut self, dest: Place, value: TypedValue, expected: &IrType, span: Span) {
        let value = self.coerce_to_operand(value, expected, span);
        self.emit(
            Instruction::Copy {
                dest,
                src: value,
                ty: expected.clone(),
            },
            span,
        );
    }

    /// 按目标类型包装可选值或结果值。
    ///
    /// 已经是对应包装类型的值直接透传，避免重复包装。
    fn coerce_to_operand(&mut self, value: TypedValue, expected: &IrType, span: Span) -> Operand {
        if expected.is_optional() {
            if value.ty.is_optional() {
                return value.operand;
            }

            let dest = self.new_temp();
            let wrapped = if matches!(value.ty, IrType::Null) {
                None
            } else {
                Some(value.operand)
            };
            self.emit(
                Instruction::WrapOptional {
                    dest: dest.clone(),
                    value: wrapped,
                    ty: expected.clone(),
                },
                span,
            );
            return Operand::Place(dest);
        }

        if expected.is_result() {
            if value.ty.is_result() {
                return value.operand;
            }

            let dest = self.new_temp();
            self.emit(
                Instruction::WrapResult {
                    dest: dest.clone(),
                    value: value.operand,
                    ty: expected.clone(),
                },
                span,
            );
            return Operand::Place(dest);
        }

        value.operand
    }

    fn new_label(&mut self) -> Label {
        let label = Label(self.next_label);
        self.next_label += 1;
        label
    }

    fn report(&mut self, span: Span, message: impl Into<String>) {
        self.diagnostics
            .push(Diagnostic::error(message).with_span(span));
    }
}

fn constant_from_literal(literal: &Literal) -> Constant {
    match literal {
        Literal::Integer(value) => Constant::Integer(*value),
        Literal::Float(value) => Constant::Float(*value),
        Literal::Bool(value) => Constant::Bool(*value),
        Literal::String(value) => Constant::String(value.clone()),
        Literal::Null => Constant::Null,
    }
}
