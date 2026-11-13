//! 符号表与声明信息。
//!
//! 符号表按块维护变量作用域：每进入一个函数体或语句块就压入新作用域，
//! 离开时弹出。函数与结构体声明在分析函数体之前统一登记，因此支持递归调用、
//! 前向引用以及结构体字段引用其他结构体。

use std::collections::HashMap;

use crate::ty::Ty;

/// 一个变量的符号信息。
#[derive(Clone, Debug)]
pub(crate) struct Variable {
    /// 变量的静态类型。
    pub ty: Ty,
    /// 是否由 `可变` 修饰。
    pub mutable: bool,
}

/// 单个词法作用域内的变量。
#[derive(Default)]
struct Scope {
    variables: HashMap<String, Variable>,
}

/// 支持嵌套作用域的符号表。
#[derive(Default)]
pub(crate) struct SymbolTable {
    scopes: Vec<Scope>,
}

impl SymbolTable {
    /// 进入一个新的作用域。
    pub(crate) fn push_scope(&mut self) {
        self.scopes.push(Scope::default());
    }

    /// 离开当前作用域。
    pub(crate) fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    /// 在当前作用域声明变量。
    ///
    /// 同一作用域内重复声明会返回 `false`，由调用方生成诊断；
    /// 嵌套作用域允许遮蔽外层同名变量。
    pub(crate) fn declare(&mut self, name: &str, variable: Variable) -> bool {
        let Some(scope) = self.scopes.last_mut() else {
            return false;
        };

        if scope.variables.contains_key(name) {
            return false;
        }

        scope.variables.insert(name.to_owned(), variable);
        true
    }

    /// 从内向外查找变量。
    pub(crate) fn lookup(&self, name: &str) -> Option<&Variable> {
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.variables.get(name))
    }
}

/// 函数签名。
#[derive(Clone, Debug)]
pub(crate) struct FunctionSignature {
    /// 参数类型，按声明顺序排列。
    pub params: Vec<Ty>,
    /// 返回类型。
    pub return_type: Ty,
}

/// 结构体信息。
#[derive(Clone, Debug)]
pub(crate) struct StructInfo {
    /// 字段名称到字段类型的映射。
    pub fields: HashMap<String, Ty>,
}
