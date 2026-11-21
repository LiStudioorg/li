//! 导入解析与跨包链接。
//!
//! 链接器把外部包转换为独立的模块树，再与主程序合并为一份 AST：
//!
//! - `导入 math_lib;` 会优先查找项目内的 `math_lib.li` / `math_lib/主.li`，
//!   找不到时再查找 `.li/依赖/math_lib`；
//! - 包内函数以 `math_lib::加法` 这样的内部名称进入编译流水线，
//!   主程序中的 `math_lib.加法(1, 2)` 会被改写为对该内部函数的调用；
//! - 包内结构体同样加前缀，避免与主程序或其他依赖重名；
//! - 依赖包自身的 `导入` 会继续解析，形成独立的模块树。
//!
//! 链接发生在前端语义检查之前，后续阶段仍然只看到一份普通 AST。

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use li_ast::{
    Block, Expr, ExprKind, FuncDecl, Ident, ImportDecl, Item, ModulePath, Program, Stmt, Type,
    TypeDecl, TypeKind, ViewDecl,
};
use li_errors::{Diagnostic, DiagnosticBag};
use li_parser::parse_source;

use crate::fetch::{ensure_ready, project_entry};
use crate::lockfile::DEPENDENCY_DIR;
use crate::manifest::{Manifest, MANIFEST_FILE};

/// 从入口文件所在目录向上查找包含 `li.toml` 的项目根目录。
pub fn find_project_root(start: &Path) -> Option<PathBuf> {
    let mut current = Some(start);

    while let Some(directory) = current {
        if directory.join(MANIFEST_FILE).is_file() {
            return Some(directory.to_path_buf());
        }
        current = directory.parent();
    }

    None
}

/// 在进入语义检查前解析导入并链接外部模块。
///
/// 程序没有导入声明时原样返回，保持普通单文件编译行为不变。
pub fn prepare_compile(entry: &Path, program: &Program) -> Result<Program, DiagnosticBag> {
    let entry = absolute(entry);
    let entry_dir = entry
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf();
    let root = find_project_root(&entry_dir);
    let mut resolver = Resolver::new(root, entry_dir)?;

    if !has_imports(&program.items) {
        return Ok(program.clone());
    }

    let linked = resolver.link_root(program);

    if resolver.diagnostics.has_errors() {
        Err(resolver.diagnostics)
    } else {
        Ok(linked)
    }
}

/// 判断程序中是否包含导入声明；模块内部也算。
pub fn has_imports(items: &[Item]) -> bool {
    items.iter().any(|item| match item {
        Item::Import(_) => true,
        Item::Module(module) => has_imports(&module.items),
        Item::Function(_) | Item::Type(_) | Item::View(_) | Item::Enum(_) => false,
    })
}

/// 导入解析器。
struct Resolver {
    /// 项目根目录；单文件编译时为 `None`。
    root: Option<PathBuf>,
    /// 项目配置。
    manifest: Option<Manifest>,
    /// 入口文件所在目录。
    entry_dir: PathBuf,
    /// 正在加载的模块路径，用于循环导入检测。
    loading: Vec<PathBuf>,
    /// 累积诊断。
    diagnostics: DiagnosticBag,
}

impl Resolver {
    /// 创建解析器，并在项目声明依赖时检查安装状态与锁文件。
    fn new(root: Option<PathBuf>, entry_dir: PathBuf) -> Result<Self, DiagnosticBag> {
        let manifest = match &root {
            Some(root) => {
                let manifest = Manifest::load(root)?;
                ensure_ready(root, &manifest)?;
                Some(manifest)
            }
            None => None,
        };

        Ok(Self {
            root,
            manifest,
            entry_dir,
            loading: Vec::new(),
            diagnostics: DiagnosticBag::new(),
        })
    }

    /// 链接主程序及其导入。
    fn link_root(&mut self, program: &Program) -> Program {
        let internal = internal_module_names(&program.items);
        let from_dir = self.entry_dir.clone();
        let imports = self.load_imports(&program.items, &from_dir, "", true, &internal);

        let (functions, types) = flatten_declarations(&program.items);
        let mut scope = Scope::root();
        for (path, function) in &functions {
            let target = function.name.name.clone();
            scope.local_functions.insert(target.clone(), target.clone());
            scope
                .local_paths
                .insert(function_path(path, function), target.clone());
        }
        for (path, declaration) in &types {
            scope
                .local_structs
                .insert(declaration.name.name.clone(), declaration.name.name.clone());
            let _ = path;
        }
        scope.imported = imports
            .into_iter()
            .map(|module| (module.alias.clone(), module))
            .collect();

        let mut items = Vec::new();

        for (_, mut function) in functions {
            rewrite_function(&mut function, &scope, &mut self.diagnostics);
            items.push(Item::Function(function));
        }

        for (_, mut declaration) in types {
            rewrite_type_decl(&mut declaration, &scope);
            items.push(Item::Type(declaration));
        }

        // 外部包在加载时已经完成重命名，直接追加到主程序之后。
        for module in scope.imported.values() {
            for item in &module.items {
                items.push(item.clone());
            }
        }

        Program {
            items,
            no_gc: program.no_gc,
            span: program.span,
        }
    }

    /// 加载一段源码中的全部导入。
    fn load_imports(
        &mut self,
        items: &[Item],
        from_dir: &Path,
        parent_namespace: &str,
        allow_root_src: bool,
        internal: &HashSet<String>,
    ) -> Vec<LinkedModule> {
        let mut imports = Vec::new();
        let mut seen = HashSet::new();

        for import in collect_imports(items) {
            let Some(alias) = import
                .path
                .segments
                .first()
                .map(|segment| segment.name.clone())
            else {
                continue;
            };

            if internal.contains(&alias) {
                continue;
            }

            if !seen.insert(alias.clone()) {
                continue;
            }

            if import.path.segments.len() != 1 {
                self.diagnostics.push(
                    Diagnostic::error("第一版只支持导入单个包或模块名")
                        .with_span(import.path.span)
                        .with_expected(format!("`导入 {alias};`"))
                        .with_actual(describe_import_path(&import.path))
                        .with_note("导入包后使用 `包名.函数(...)` 调用"),
                );
                continue;
            }

            let before = self.diagnostics.len();
            let Some(source) = self.resolve_source(&alias, from_dir, allow_root_src) else {
                // 依赖包自身的问题已经产生更具体的诊断，避免再报一次泛化的“未找到模块”。
                if self.diagnostics.len() == before {
                    self.diagnostics.push(self.missing_module_diagnostic(
                        &alias,
                        &import,
                        from_dir,
                        allow_root_src,
                    ));
                }
                continue;
            };

            let namespace = if parent_namespace.is_empty() {
                alias.clone()
            } else {
                format!("{parent_namespace}::{alias}")
            };

            if let Some(module) = self.load_module(&source, &alias, &namespace) {
                imports.push(module);
            }
        }

        imports
    }

    /// 按「本地模块优先、项目依赖其次」的顺序解析导入。
    fn resolve_source(
        &mut self,
        alias: &str,
        from_dir: &Path,
        allow_root_src: bool,
    ) -> Option<ModuleSource> {
        for candidate in local_candidates(alias, from_dir, allow_root_src, self.root.as_deref()) {
            if candidate.is_file() {
                return Some(ModuleSource::File(candidate));
            }
        }

        let root = self.root.clone()?;

        if let Some(manifest) = &self.manifest {
            if let Some(dependency) = manifest.dependency(alias) {
                let directory = root.join(DEPENDENCY_DIR).join(&dependency.name);
                return self
                    .package_entry(&directory, alias)
                    .map(|entry| ModuleSource::Package { entry });
            }
        }

        None
    }

    /// 为未解析的导入生成带查找位置的诊断。
    fn missing_module_diagnostic(
        &self,
        alias: &str,
        import: &ImportDecl,
        from_dir: &Path,
        allow_root_src: bool,
    ) -> Diagnostic {
        let mut diagnostic = Diagnostic::error(format!("未找到模块「{alias}」"))
            .with_span(import.path.span)
            .with_expected("本地模块文件或 li.toml 中声明的依赖");

        let candidates = local_candidates(alias, from_dir, allow_root_src, self.root.as_deref());
        let searched = candidates
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join("、");
        diagnostic = diagnostic.with_note(format!("已查找：{searched}"));

        match (&self.root, &self.manifest) {
            (Some(root), Some(manifest)) => {
                if manifest.dependencies.is_empty() {
                    diagnostic = diagnostic
                        .with_note(format!("项目「{}」还没有声明依赖", manifest.project.name));
                } else {
                    let names = manifest
                        .dependencies
                        .iter()
                        .map(|dependency| dependency.name.as_str())
                        .collect::<Vec<_>>()
                        .join("、");
                    diagnostic = diagnostic.with_note(format!("已声明依赖：{names}"));
                }
                diagnostic = diagnostic
                    .with_note(format!("依赖目录：{}", root.join(DEPENDENCY_DIR).display()));
            }
            _ => {
                diagnostic = diagnostic
                    .with_note("如果这是外部依赖，请在项目根目录创建 li.toml 并运行「li 获取」");
            }
        }

        diagnostic
    }

    /// 加载一个外部模块文件，并递归加载它自己的导入。
    fn load_module(
        &mut self,
        source: &ModuleSource,
        alias: &str,
        namespace: &str,
    ) -> Option<LinkedModule> {
        let entry = source.entry().to_path_buf();
        let entry = match fs::canonicalize(&entry) {
            Ok(entry) => entry,
            Err(error) => {
                self.diagnostics.push(
                    Diagnostic::error(format!("无法读取模块「{alias}」"))
                        .with_note(io_error_text(&error))
                        .with_note(format!("文件：{}", entry.display())),
                );
                return None;
            }
        };

        if let Some(position) = self.loading.iter().position(|path| path == &entry) {
            let chain = self
                .loading
                .iter()
                .skip(position)
                .chain(std::iter::once(&entry))
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(" → ");
            self.diagnostics.push(
                Diagnostic::error(format!("检测到循环导入「{alias}」"))
                    .with_span(source.span())
                    .with_note(format!("导入链：{chain}")),
            );
            return None;
        }

        self.loading.push(entry.clone());
        let module = self.load_module_inner(&entry, alias, namespace);
        self.loading.pop();
        module
    }

    /// 加载模块的实际实现。
    fn load_module_inner(
        &mut self,
        entry: &Path,
        alias: &str,
        namespace: &str,
    ) -> Option<LinkedModule> {
        let source = match fs::read_to_string(entry) {
            Ok(source) => source,
            Err(error) => {
                self.diagnostics.push(
                    Diagnostic::error(format!("无法读取模块「{alias}」的源码"))
                        .with_note(io_error_text(&error))
                        .with_note(format!("文件：{}", entry.display())),
                );
                return None;
            }
        };

        let program = match parse_source(&source) {
            Ok(program) => program,
            Err(errors) => {
                self.diagnostics.extend(annotate(
                    errors,
                    format!("模块「{alias}」：{}", entry.display()),
                ));
                return None;
            }
        };

        if let Some(view) = find_view(&program.items) {
            self.diagnostics.push(
                Diagnostic::error(format!("依赖模块「{alias}」不能包含视图声明"))
                    .with_span(view.span)
                    .with_note("视图文件请使用「li 编译-html」单独编译"),
            );
            return None;
        }

        let internal = internal_module_names(&program.items);
        let from_dir = entry.parent().unwrap_or_else(|| Path::new("."));
        let imports = self.load_imports(&program.items, from_dir, namespace, false, &internal);

        let (functions, types) = flatten_declarations(&program.items);
        let mut scope = Scope::root();
        let mut exports = HashMap::new();
        let mut export_names = Vec::new();

        for (path, function) in &functions {
            let target = qualify(namespace, path, &function.name.name);
            let key = function_path(path, function);
            exports.insert(key.clone(), target.clone());
            // 如果入口文件用同名模块包住全部函数，同时暴露更短的路径，
            // 例如 `模块 math_lib { 函数 加法 ... }` 可直接写 `math_lib.加法`。
            if key.first().is_some_and(|first| first == alias) && key.len() > 1 {
                let shorter = key[1..].to_vec();
                exports.entry(shorter).or_insert_with(|| target.clone());
            }
            export_names.push(key.join("."));
            scope
                .local_functions
                .insert(function.name.name.clone(), target.clone());
            scope.local_paths.insert(key, target);
        }

        for (path, declaration) in &types {
            let target = qualify(namespace, path, &declaration.name.name);
            scope
                .local_structs
                .insert(declaration.name.name.clone(), target);
        }

        scope.imported = imports
            .into_iter()
            .map(|module| (module.alias.clone(), module))
            .collect();

        let mut items = Vec::new();

        for (path, mut function) in functions {
            let target = scope
                .local_paths
                .get(&function_path(&path, &function))
                .cloned()
                .unwrap_or_else(|| qualify(namespace, &path, &function.name.name));
            function.name = Ident::new(target, function.name.span);
            rewrite_function(&mut function, &scope, &mut self.diagnostics);
            items.push(Item::Function(function));
        }

        for (path, mut declaration) in types {
            let target = scope
                .local_structs
                .get(&declaration.name.name)
                .cloned()
                .unwrap_or_else(|| qualify(namespace, &path, &declaration.name.name));
            declaration.name = Ident::new(target, declaration.name.span);
            rewrite_type_decl(&mut declaration, &scope);
            items.push(Item::Type(declaration));
        }

        // 把依赖包自己导入的模块也并入同一棵模块树。
        for module in scope.imported.values() {
            for item in &module.items {
                items.push(item.clone());
            }
        }

        Some(LinkedModule {
            alias: alias.to_owned(),
            items,
            exports,
            export_names,
        })
    }

    /// 读取依赖包入口；包内 `li.toml` 可以覆盖默认布局。
    fn package_entry(&mut self, directory: &Path, alias: &str) -> Option<PathBuf> {
        if directory.join(MANIFEST_FILE).is_file() {
            let manifest = match Manifest::load(directory) {
                Ok(manifest) => manifest,
                Err(errors) => {
                    self.diagnostics.extend(annotate(
                        errors,
                        format!("依赖「{alias}」：{}", directory.display()),
                    ));
                    return None;
                }
            };

            match project_entry(directory, &manifest) {
                Ok(entry) => return Some(entry),
                Err(errors) => {
                    self.diagnostics.extend(annotate(
                        errors,
                        format!("依赖「{alias}」：{}", directory.display()),
                    ));
                    return None;
                }
            }
        }

        for candidate in common_package_entries(directory) {
            if candidate.is_file() {
                return Some(candidate);
            }
        }

        self.diagnostics.push(
            Diagnostic::error(format!("依赖「{alias}」缺少入口文件"))
                .with_expected("主.li、src/主.li、lib.li 或 li.toml 中的 `入口`")
                .with_note(format!("依赖目录：{}", directory.display()))
                .with_note("请检查依赖包结构，或重新运行「li 获取」"),
        );
        None
    }
}

/// 一个已加载并重命名的外部模块。
struct LinkedModule {
    /// 导入时使用的名称。
    alias: String,
    /// 已重命名并改写调用的声明。
    items: Vec<Item>,
    /// 相对模块路径 → 内部函数名。
    exports: HashMap<Vec<String>, String>,
    /// 便于诊断展示的导出函数名称。
    export_names: Vec<String>,
}

/// 导入来源。
enum ModuleSource {
    /// 项目内的普通模块文件。
    File(PathBuf),
    /// 已安装的依赖包。
    Package { entry: PathBuf },
}

impl ModuleSource {
    fn entry(&self) -> &Path {
        match self {
            Self::File(path) => path,
            Self::Package { entry } => entry,
        }
    }

    fn span(&self) -> li_errors::Span {
        li_errors::Span::point(li_errors::Position::new(1, 1, 0))
    }
}

/// 当前链接作用域。
struct Scope {
    /// 当前文件声明的函数：简单名 → 内部名。
    local_functions: HashMap<String, String>,
    /// 当前文件声明的函数：模块路径 → 内部名。
    local_paths: HashMap<Vec<String>, String>,
    /// 当前文件声明的结构体：简单名 → 内部名。
    local_structs: HashMap<String, String>,
    /// 导入模块：别名 → 模块信息；使用有序映射保证生成顺序稳定。
    imported: BTreeMap<String, LinkedModule>,
}

impl Scope {
    fn root() -> Self {
        Self {
            local_functions: HashMap::new(),
            local_paths: HashMap::new(),
            local_structs: HashMap::new(),
            imported: BTreeMap::new(),
        }
    }

    /// 解析 `模块.函数` 形式的调用路径。
    fn resolve_path(&self, segments: &[String]) -> Option<&str> {
        let first = segments.first()?;

        if let Some(module) = self.imported.get(first) {
            return module.exports.get(&segments[1..]).map(String::as_str);
        }

        self.local_paths.get(segments).map(String::as_str)
    }

    /// 判断路径首段是否为已知模块。
    fn is_known_prefix(&self, name: &str) -> bool {
        self.imported.contains_key(name)
            || self
                .local_paths
                .keys()
                .any(|path| path.first().is_some_and(|first| first == name))
    }

    /// 生成可用函数提示。
    fn available_for(&self, name: &str) -> Option<String> {
        if let Some(module) = self.imported.get(name) {
            let mut names = module.export_names.clone();
            names.sort();
            names.dedup();
            return Some(names.join("、"));
        }

        let mut names = self
            .local_paths
            .keys()
            .filter(|path| path.first().is_some_and(|first| first == name))
            .map(|path| path[1..].join("."))
            .collect::<Vec<_>>();
        names.sort();
        names.dedup();
        Some(names.join("、"))
    }
}

/// 展平后的函数声明：模块路径 + 函数。
type FlattenedFunctions = Vec<(Vec<String>, FuncDecl)>;

/// 展平后的类型声明：模块路径 + 类型。
type FlattenedTypes = Vec<(Vec<String>, TypeDecl)>;

/// 把声明展平为「模块路径 + 声明」。
fn flatten_declarations(items: &[Item]) -> (FlattenedFunctions, FlattenedTypes) {
    let mut functions = Vec::new();
    let mut types = Vec::new();
    flatten_into(items, &mut Vec::new(), &mut functions, &mut types);
    (functions, types)
}

fn flatten_into(
    items: &[Item],
    prefix: &mut Vec<String>,
    functions: &mut FlattenedFunctions,
    types: &mut FlattenedTypes,
) {
    for item in items {
        match item {
            Item::Function(function) => functions.push((prefix.clone(), function.clone())),
            Item::Type(declaration) => types.push((prefix.clone(), declaration.clone())),
            // 枚举声明不参与跨包改写；保持原样即可。
            Item::Enum(_) => {}
            Item::Module(module) => {
                prefix.push(module.name.name.clone());
                flatten_into(&module.items, prefix, functions, types);
                prefix.pop();
            }
            Item::Import(_) | Item::View(_) => {}
        }
    }
}

/// 递归改写函数体中的调用。
fn rewrite_function(function: &mut FuncDecl, scope: &Scope, diagnostics: &mut DiagnosticBag) {
    for param in &mut function.params {
        rewrite_type(&mut param.ty, scope);
    }
    rewrite_type(&mut function.return_type, scope);
    rewrite_block(&mut function.body, scope, diagnostics);
}

/// 递归改写结构体字段类型。
fn rewrite_type_decl(declaration: &mut TypeDecl, scope: &Scope) {
    for field in &mut declaration.fields {
        rewrite_type(&mut field.ty, scope);
    }
}

/// 改写类型标注中的外部结构体名称。
fn rewrite_type(ty: &mut Type, scope: &Scope) {
    match &mut ty.kind {
        TypeKind::List(inner)
        | TypeKind::RawPointer(inner)
        | TypeKind::Optional(inner)
        | TypeKind::Task(inner) => {
            rewrite_type(inner, scope);
        }
        TypeKind::Dictionary { key, value } => {
            rewrite_type(key, scope);
            rewrite_type(value, scope);
        }
        TypeKind::Result { value, error } => {
            rewrite_type(value, scope);
            rewrite_type(error, scope);
        }
        TypeKind::Named(ident) => {
            if let Some(target) = scope.local_structs.get(&ident.name) {
                ident.name = target.clone();
            }
        }
        TypeKind::Integer | TypeKind::Float | TypeKind::Bool | TypeKind::String => {}
    }
}

/// 递归改写语句块。
fn rewrite_block(block: &mut Block, scope: &Scope, diagnostics: &mut DiagnosticBag) {
    for stmt in &mut block.stmts {
        rewrite_stmt(stmt, scope, diagnostics);
    }
}

/// 改写单条语句。
fn rewrite_stmt(stmt: &mut Stmt, scope: &Scope, diagnostics: &mut DiagnosticBag) {
    match stmt {
        Stmt::Let(declaration) => {
            if let Some(ty) = &mut declaration.ty {
                rewrite_type(ty, scope);
            }
            rewrite_expr(&mut declaration.initializer, scope, diagnostics);
        }
        Stmt::Assign(assignment) => {
            rewrite_expr(&mut assignment.target, scope, diagnostics);
            rewrite_expr(&mut assignment.value, scope, diagnostics);
        }
        Stmt::Expr(statement) => rewrite_expr(&mut statement.expr, scope, diagnostics),
        Stmt::Return(statement) => {
            if let Some(value) = &mut statement.value {
                rewrite_expr(value, scope, diagnostics);
            }
        }
        Stmt::If(statement) => {
            rewrite_expr(&mut statement.condition, scope, diagnostics);
            rewrite_block(&mut statement.then_branch, scope, diagnostics);
            if let Some(branch) = &mut statement.else_branch {
                rewrite_block(branch, scope, diagnostics);
            }
        }
        Stmt::While(statement) => {
            rewrite_expr(&mut statement.condition, scope, diagnostics);
            rewrite_block(&mut statement.body, scope, diagnostics);
        }
        Stmt::Match(statement) => {
            rewrite_expr(&mut statement.value, scope, diagnostics);
            for arm in &mut statement.arms {
                rewrite_block(&mut arm.body, scope, diagnostics);
            }
        }
        Stmt::Unsafe(statement) => {
            rewrite_block(&mut statement.body, scope, diagnostics);
        }
        Stmt::Print(statement) => rewrite_expr(&mut statement.value, scope, diagnostics),
        Stmt::Break(_) | Stmt::Continue(_) | Stmt::InlineAsm(_) => {}
    }
}

/// 改写表达式中的跨模块调用。
fn rewrite_expr(expr: &mut Expr, scope: &Scope, diagnostics: &mut DiagnosticBag) {
    match &mut expr.kind {
        ExprKind::Call { callee, args } => {
            if let Some(segments) = call_path(callee) {
                if segments.len() > 1 {
                    if let Some(target) = scope.resolve_path(&segments) {
                        let span = callee.span;
                        **callee = Expr::new(
                            ExprKind::Identifier(Ident::new(target.to_owned(), span)),
                            span,
                        );
                    } else if scope.is_known_prefix(&segments[0]) {
                        let available = scope
                            .available_for(&segments[0])
                            .unwrap_or_else(|| "无".to_owned());
                        diagnostics.push(
                            Diagnostic::error(format!(
                                "模块「{}」没有可调用的函数「{}」",
                                segments[0],
                                segments[1..].join(".")
                            ))
                            .with_span(callee.span)
                            .with_note(format!("可用函数：{available}")),
                        );
                    }
                } else if let Some(name) = segments.first() {
                    if let Some(target) = scope.local_functions.get(name) {
                        if target != name {
                            let span = callee.span;
                            **callee = Expr::new(
                                ExprKind::Identifier(Ident::new(target.clone(), span)),
                                span,
                            );
                        }
                    }
                }
            }

            rewrite_expr(callee, scope, diagnostics);
            for argument in args {
                rewrite_expr(argument, scope, diagnostics);
            }
        }
        ExprKind::FieldAccess { object, .. } => {
            rewrite_expr(object, scope, diagnostics);
        }
        ExprKind::Index { object, index } => {
            rewrite_expr(object, scope, diagnostics);
            rewrite_expr(index, scope, diagnostics);
        }
        ExprKind::Unary { operand, .. } => rewrite_expr(operand, scope, diagnostics),
        ExprKind::Binary { left, right, .. } => {
            rewrite_expr(left, scope, diagnostics);
            rewrite_expr(right, scope, diagnostics);
        }
        ExprKind::ListLiteral(items) => {
            for item in items {
                rewrite_expr(item, scope, diagnostics);
            }
        }
        ExprKind::Parenthesized(inner) => rewrite_expr(inner, scope, diagnostics),
        ExprKind::Await(inner) => rewrite_expr(inner, scope, diagnostics),
        ExprKind::EnumVariant(variant) => {
            for arg in &mut variant.args {
                rewrite_expr(arg, scope, diagnostics);
            }
        }
        ExprKind::Literal(_) | ExprKind::Identifier(_) | ExprKind::Input => {}
    }
}

/// 把 `模块.函数` 表达式展平为名称片段。
fn call_path(expr: &Expr) -> Option<Vec<String>> {
    match &expr.kind {
        ExprKind::Identifier(ident) => Some(vec![ident.name.clone()]),
        ExprKind::FieldAccess { object, field } => {
            let mut path = call_path(object)?;
            path.push(field.name.clone());
            Some(path)
        }
        _ => None,
    }
}

/// 收集全部导入声明。
fn collect_imports(items: &[Item]) -> Vec<ImportDecl> {
    let mut imports = Vec::new();
    collect_imports_into(items, &mut imports);
    imports
}

fn collect_imports_into(items: &[Item], imports: &mut Vec<ImportDecl>) {
    for item in items {
        match item {
            Item::Import(import) => imports.push(import.clone()),
            Item::Module(module) => collect_imports_into(&module.items, imports),
            Item::Function(_) | Item::Type(_) | Item::View(_) | Item::Enum(_) => {}
        }
    }
}

/// 收集源码中声明的模块名，用于区分本地模块与外部包。
fn internal_module_names(items: &[Item]) -> HashSet<String> {
    let mut names = HashSet::new();
    collect_module_names(items, &mut names);
    names
}

fn collect_module_names(items: &[Item], names: &mut HashSet<String>) {
    for item in items {
        if let Item::Module(module) = item {
            names.insert(module.name.name.clone());
            collect_module_names(&module.items, names);
        }
    }
}

/// 查找视图声明，用于拒绝依赖包中的视图。
fn find_view(items: &[Item]) -> Option<&ViewDecl> {
    for item in items {
        match item {
            Item::View(view) => return Some(view),
            Item::Module(module) => {
                if let Some(view) = find_view(&module.items) {
                    return Some(view);
                }
            }
            Item::Import(_) | Item::Function(_) | Item::Type(_) | Item::Enum(_) => {}
        }
    }
    None
}

/// 生成函数的完整模块路径（含函数名）。
fn function_path(prefix: &[String], function: &FuncDecl) -> Vec<String> {
    let mut path = prefix.to_vec();
    path.push(function.name.name.clone());
    path
}

/// 生成内部限定名。
fn qualify(namespace: &str, prefix: &[String], name: &str) -> String {
    let mut segments = Vec::with_capacity(prefix.len() + 2);
    segments.push(namespace.to_owned());
    segments.extend(prefix.iter().cloned());
    segments.push(name.to_owned());
    segments.join("::")
}

/// 列出本地模块候选路径。
fn local_candidates(
    alias: &str,
    from_dir: &Path,
    allow_root_src: bool,
    root: Option<&Path>,
) -> Vec<PathBuf> {
    let mut candidates = vec![
        from_dir.join(format!("{alias}.li")),
        from_dir.join(alias).join("主.li"),
    ];

    if allow_root_src {
        if let Some(root) = root {
            candidates.push(root.join("src").join(format!("{alias}.li")));
            candidates.push(root.join("src").join(alias).join("主.li"));
            candidates.push(root.join(alias).join("主.li"));
        }
    }

    candidates
}

/// 依赖包未声明入口时的常见布局。
fn common_package_entries(directory: &Path) -> Vec<PathBuf> {
    let name = directory
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();

    let mut candidates = vec![
        directory.join("主.li"),
        directory.join("src/主.li"),
        directory.join("lib.li"),
        directory.join("src/lib.li"),
    ];

    if !name.is_empty() {
        candidates.push(directory.join(format!("{name}.li")));
        candidates.push(directory.join("src").join(format!("{name}.li")));
    }

    candidates
}

/// 给一组诊断追加模块来源说明。
fn annotate(errors: DiagnosticBag, note: String) -> DiagnosticBag {
    let mut annotated = DiagnosticBag::new();
    for diagnostic in errors {
        annotated.push(diagnostic.with_note(note.clone()));
    }
    annotated
}

/// 描述导入路径，用于诊断。
fn describe_import_path(path: &ModulePath) -> String {
    path.segments
        .iter()
        .map(|segment| segment.name.as_str())
        .collect::<Vec<_>>()
        .join("::")
}

/// 转换为绝对路径；文件不存在时保留原路径。
fn absolute(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| {
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join(path)
        }
    })
}

/// 把输入输出错误转换为中文说明。
fn io_error_text(error: &std::io::Error) -> &'static str {
    use std::io::ErrorKind;

    match error.kind() {
        ErrorKind::NotFound => "文件不存在",
        ErrorKind::PermissionDenied => "权限不足",
        ErrorKind::InvalidData => "文件内容不是有效的 UTF-8",
        ErrorKind::InvalidInput => "路径或参数无效",
        ErrorKind::AlreadyExists => "文件已存在",
        ErrorKind::WriteZero => "写入失败",
        ErrorKind::UnexpectedEof => "文件内容不完整",
        _ => "输入输出错误",
    }
}
