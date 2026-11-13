//! JavaScript 标识符安全化。
//!
//! li 的标识符大多可以直接作为 JavaScript 标识符使用，但中文源码中
//! 也可能出现 JavaScript 保留字或运行时全局名。这里统一为函数名、
//! 参数与局部变量生成不冲突的 JavaScript 名称。

use std::collections::HashSet;

/// 生成 JavaScript 标识符并保证同一作用域内不冲突。
pub(crate) struct NameMangler {
    used: HashSet<String>,
}

impl NameMangler {
    /// 创建一个空的名称生成器。
    pub(crate) fn new() -> Self {
        Self {
            used: HashSet::new(),
        }
    }

    /// 把 li 名称转换为唯一的 JavaScript 标识符。
    pub(crate) fn mangle(&mut self, name: &str) -> String {
        let mut candidate = sanitize(name);

        if is_reserved(&candidate) {
            candidate = format!("_li_{candidate}");
        }

        let base = candidate.clone();
        let mut suffix = 2;
        while self.used.contains(&candidate) {
            candidate = format!("{base}_{suffix}");
            suffix += 1;
        }

        self.used.insert(candidate.clone());
        candidate
    }
}

/// 判断名称是否可以直接用作 JavaScript 属性名。
///
/// 属性访问允许保留字，因此这里只检查字符是否构成标识符。
pub(crate) fn is_js_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };

    is_identifier_start(first) && chars.all(is_identifier_continue)
}

fn sanitize(name: &str) -> String {
    let mut result = String::new();

    for (index, ch) in name.chars().enumerate() {
        let valid = if index == 0 {
            is_identifier_start(ch)
        } else {
            is_identifier_continue(ch)
        };

        if valid {
            result.push(ch);
        } else {
            result.push('_');
        }
    }

    if result.is_empty() {
        result.push_str("__li_匿名");
    }

    result
}

fn is_identifier_start(ch: char) -> bool {
    ch == '_' || ch == '$' || ch.is_alphabetic()
}

fn is_identifier_continue(ch: char) -> bool {
    is_identifier_start(ch) || ch.is_ascii_digit()
}

fn is_reserved(name: &str) -> bool {
    matches!(
        name,
        // JavaScript 关键字与严格模式保留字
        "await"
            | "break"
            | "case"
            | "catch"
            | "class"
            | "const"
            | "continue"
            | "debugger"
            | "default"
            | "delete"
            | "do"
            | "else"
            | "enum"
            | "export"
            | "extends"
            | "false"
            | "finally"
            | "for"
            | "function"
            | "if"
            | "implements"
            | "import"
            | "in"
            | "instanceof"
            | "interface"
            | "let"
            | "new"
            | "null"
            | "package"
            | "private"
            | "protected"
            | "public"
            | "return"
            | "static"
            | "super"
            | "switch"
            | "this"
            | "throw"
            | "true"
            | "try"
            | "typeof"
            | "var"
            | "void"
            | "while"
            | "with"
            | "yield"
            // 代码生成器依赖的运行时全局名
            | "console"
            | "Math"
            | "Error"
            | "undefined"
            | "NaN"
            | "Infinity"
            | "prompt"
            | "globalThis"
    ) || name.starts_with("__li_")
        || is_temp_name(name)
}

fn is_temp_name(name: &str) -> bool {
    match name.strip_prefix("__t") {
        Some(rest) => !rest.is_empty() && rest.chars().all(|ch| ch.is_ascii_digit()),
        None => false,
    }
}
