//! li 语言 `.lui` 视图文件的单体 HTML 后端。
//!
//! 后端直接消费 [`li_ast::Program`]，把 `视图 { ... }` 声明翻译为
//! 一个**完整的 HTML5 文档**：结构、样式与交互脚本全部内联，不产生
//! `index.html` 与 `app.js` 的分离文件，也不引用任何外部资源。
//!
//! - `状态 { ... }` 中的变量成为内联脚本里的顶层状态；
//! - `文本 { 「计数：」, 次数 }` 与 `类: { 表达式 }` 注册渲染绑定；
//! - `点击 { 次数 = 次数 加 1; }` 注册 DOM 事件监听；
//! - 每次事件执行后自动重新渲染全部绑定。
//!
//! 中文标签与属性通过 [`mapping`] 中的固定表映射，也允许直接使用常见的
//! ASCII 静态 HTML 标签名与属性名。`script`、`style` 等标签与 `on*`
//! 事件属性仍会被拒绝，事件统一使用 `点击 { ... }` 形式声明。
//!
//! 所有诊断均为中文，并保留 `.lui` 源码中的行列位置。

mod escape;
mod mapping;
mod script;

use std::collections::HashSet;

use li_ast::{
    Ident, Item, Program, ViewAttribute, ViewContent, ViewDecl, ViewNode, ViewTextPartKind,
    ViewValue, ViewValueKind,
};
use li_errors::{Diagnostic, DiagnosticBag};

use script::ScriptTranslator;

/// 从一个 li 程序生成完整的单体 HTML5 文档。
///
/// 程序中必须恰好包含一个视图声明；视图可以位于顶层，也可以位于模块内部。
pub fn generate(program: &Program) -> Result<String, DiagnosticBag> {
    let mut views = Vec::new();
    collect_views(&program.items, &mut views);

    if views.is_empty() {
        let mut diagnostics = DiagnosticBag::new();
        diagnostics.push(
            Diagnostic::error("未找到视图声明").with_note("请在 .lui 文件中编写 `视图 { ... }`"),
        );
        return Err(diagnostics);
    }

    if views.len() > 1 {
        let mut diagnostics = DiagnosticBag::new();
        diagnostics.push(
            Diagnostic::error("一个视图文件只能包含一个视图声明")
                .with_span(views[1].span)
                .with_note("请把其他视图合并到第一个视图内部"),
        );
        return Err(diagnostics);
    }

    generate_view(views[0])
}

/// 直接从一个视图声明生成单体 HTML5 文档。
pub fn generate_view(view: &ViewDecl) -> Result<String, DiagnosticBag> {
    let mut generator = Generator::new(view);
    let output = generator.generate();

    if generator.diagnostics.has_errors() {
        Err(generator.diagnostics)
    } else {
        Ok(output)
    }
}

/// 递归收集顶层与模块内的视图声明。
fn collect_views<'a>(items: &'a [Item], views: &mut Vec<&'a ViewDecl>) {
    for item in items {
        match item {
            Item::View(view) => views.push(view),
            Item::Module(module) => collect_views(&module.items, views),
            Item::Import(_) | Item::Function(_) | Item::Type(_) | Item::Enum(_) => {}
        }
    }
}

/// HTML 生成器。
struct Generator<'a> {
    view: &'a ViewDecl,
    diagnostics: DiagnosticBag,
    /// 下一个生成的节点编号。
    next_node: usize,
    /// 渲染函数体内的绑定代码。
    bindings: Vec<String>,
    /// 事件监听注册代码。
    listeners: Vec<String>,
    /// 状态变量声明代码。
    state_code: String,
    /// 状态变量名称，供绑定表达式与事件处理共享。
    state_names: HashSet<String>,
    /// 是否需要在产物末尾内联交互脚本。
    interactive: bool,
}

impl<'a> Generator<'a> {
    fn new(view: &'a ViewDecl) -> Self {
        Self {
            view,
            diagnostics: DiagnosticBag::new(),
            next_node: 0,
            bindings: Vec::new(),
            listeners: Vec::new(),
            state_code: String::new(),
            state_names: HashSet::new(),
            interactive: false,
        }
    }

    fn generate(&mut self) -> String {
        let mut title = None;
        let mut lang = None;
        let mut description = None;
        let mut seen = HashSet::new();

        for attribute in &self.view.attributes {
            let name = attribute.name.name.as_str();

            if !seen.insert(name.to_owned()) {
                self.error_duplicate_attribute(attribute, name);
                continue;
            }

            match name {
                "标题" => title = Some(value_text(&attribute.value)),
                "语言" => lang = Some(value_text(&attribute.value)),
                "描述" => description = Some(value_text(&attribute.value)),
                _ => self.error_unknown_document_attribute(attribute),
            }
        }

        let title = title.unwrap_or_else(|| "li 视图".to_owned());
        let lang = lang.unwrap_or_else(|| "zh-CN".to_owned());

        self.prepare_interaction();

        let mut writer = Writer::new();
        writer.line("<!DOCTYPE html>");
        writer.line("<!-- 本文件由 li 编译器自动生成，请勿手动编辑。 -->");
        writer.open(format!("<html lang=\"{}\">", escape::attribute(&lang)));
        writer.open("<head>");
        writer.line("<meta charset=\"UTF-8\">");
        writer.line("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1.0\">");
        if let Some(description) = description {
            writer.line(format!(
                "<meta name=\"description\" content=\"{}\">",
                escape::attribute(&description)
            ));
        }
        writer.line(format!("<title>{}</title>", escape::text(&title)));
        if !self.view.state.is_empty() {
            self.interactive = true;
            writer.open("<style>");
            writer.line("body { font-family: system-ui, sans-serif; margin: 2rem; }");
            writer.line("button { cursor: pointer; }");
            writer.close("</style>");
        }
        writer.close("</head>");
        writer.open("<body>");

        match &self.view.content {
            ViewContent::Empty => {}
            ViewContent::Text(text) => writer.line(escape::text(text)),
            ViewContent::Parts(_) => {
                // 根节点只允许结构；根层绑定没有承载元素。
                self.diagnostics.push(
                    Diagnostic::error("视图根节点不能直接承载文本绑定")
                        .with_span(self.view.span)
                        .with_note("请把表达式放进 `文本 { ... }` 或具体节点中"),
                );
            }
            ViewContent::Children(children) => {
                for child in children {
                    self.render_node(&mut writer, child);
                }
            }
        }

        if self.interactive {
            self.render_script(&mut writer);
        }

        writer.close("</body>");
        writer.close("</html>");
        writer.finish()
    }

    /// 生成内联交互脚本。
    fn render_script(&mut self, writer: &mut Writer) {
        writer.open("<script>");
        writer.line("'use strict';");
        writer.line("function __li_显示(值) {");
        writer.line("    if (值 === true) return '真';");
        writer.line("    if (值 === false) return '假';");
        writer.line("    if (值 === null || 值 === undefined) return '空';");
        writer.line("    return String(值);");
        writer.line("}");

        for line in self.state_code.lines() {
            writer.line(line);
        }

        writer.line("const __li_绑定 = [];");
        writer.line("function __li_渲染() {");
        writer.line("    for (const 项 of __li_绑定) { 项(); }");
        writer.line("}");
        for binding in &self.bindings {
            writer.line(binding);
        }
        for listener in &self.listeners {
            for line in listener.lines() {
                writer.line(line);
            }
        }
        writer.line("__li_渲染();");
        writer.close("</script>");
    }

    /// 翻译状态声明；在渲染正文之前调用一次。
    fn prepare_interaction(&mut self) {
        let mut translator = ScriptTranslator::new(&mut self.diagnostics);
        self.state_code = translator.declare_state(&self.view.state);
        self.state_names = translator.names().clone();
    }

    /// 渲染一个普通元素节点。
    fn render_node(&mut self, writer: &mut Writer, node: &ViewNode) {
        if node.name.name == "文本" {
            self.render_text_node(writer, node);
            return;
        }

        let Some(tag) = self.resolve_tag(&node.name) else {
            return;
        };

        let dynamic = node_requires_runtime(node);
        let node_id = if dynamic {
            self.interactive = true;
            Some(self.next_node_id())
        } else {
            None
        };

        let attributes = self.render_attributes(&node.attributes, node_id.as_deref());
        self.register_events(node, node_id.as_deref());
        let void = mapping::is_void_tag(&tag);

        match &node.content {
            ViewContent::Empty => {
                if void {
                    writer.line(format!("<{tag}{attributes}>"));
                } else {
                    writer.line(format!("<{tag}{attributes}></{tag}>"));
                }
            }
            ViewContent::Text(text) => {
                if void {
                    self.error_void_content(node, &tag);
                    writer.line(format!("<{tag}{attributes}>"));
                } else {
                    writer.line(format!("<{tag}{attributes}>{}</{tag}>", escape::text(text)));
                }
            }
            ViewContent::Parts(parts) => {
                if void {
                    self.error_void_content(node, &tag);
                    writer.line(format!("<{tag}{attributes}>"));
                    return;
                }

                let Some(node_id) = node_id else {
                    return;
                };
                writer.line(format!("<{tag}{attributes}></{tag}>"));
                self.register_text_binding(&node_id, parts);
            }
            ViewContent::Children(children) => {
                if void {
                    self.error_void_content(node, &tag);
                    writer.line(format!("<{tag}{attributes}>"));
                    return;
                }

                if children.is_empty() {
                    writer.line(format!("<{tag}{attributes}></{tag}>"));
                } else if children.len() == 1 {
                    if let Some(text) = inline_text_child(&children[0]) {
                        writer.line(format!("<{tag}{attributes}>{}</{tag}>", escape::text(text)));
                    } else {
                        writer.open(format!("<{tag}{attributes}>"));
                        for child in children {
                            self.render_node(writer, child);
                        }
                        writer.close(format!("</{tag}>"));
                    }
                } else {
                    writer.open(format!("<{tag}{attributes}>"));
                    for child in children {
                        self.render_node(writer, child);
                    }
                    writer.close(format!("</{tag}>"));
                }
            }
        }
    }

    /// 渲染 `文本` 叶子节点；带属性时用 `<span>` 承载属性。
    fn render_text_node(&mut self, writer: &mut Writer, node: &ViewNode) {
        match &node.content {
            ViewContent::Text(text) => {
                let attributes = self.render_attributes(&node.attributes, None);
                if attributes.is_empty() {
                    writer.line(escape::text(text));
                } else {
                    writer.line(format!("<span{attributes}>{}</span>", escape::text(text)));
                }
            }
            ViewContent::Parts(parts) => {
                self.interactive = true;
                let node_id = self.next_node_id();
                let attributes = self.render_attributes(&node.attributes, Some(&node_id));
                writer.line(format!("<span{attributes}></span>"));
                self.register_text_binding(&node_id, parts);
            }
            ViewContent::Empty => {
                self.diagnostics.push(
                    Diagnostic::error("`文本` 节点需要字符串内容")
                        .with_span(node.span)
                        .with_note("例如：`文本 { 「你好」 }`"),
                );
            }
            ViewContent::Children(_) => {
                self.diagnostics.push(
                    Diagnostic::error("`文本` 节点只能包含字符串内容")
                        .with_span(node.span)
                        .with_note("请删除嵌套节点，或改用 `容器` 等标签"),
                );
            }
        }
    }

    /// 生成文本插值绑定。
    fn register_text_binding(&mut self, node_id: &str, parts: &[li_ast::ViewTextPart]) {
        let mut translator =
            ScriptTranslator::with_names(&mut self.diagnostics, self.state_names.clone());
        let mut pieces = Vec::new();

        for part in parts {
            match &part.kind {
                ViewTextPartKind::Text(text) => {
                    pieces.push(format!("\"{}\"", script::escape_js_string(text)));
                }
                ViewTextPartKind::Expression(expression) => {
                    let code = translator.binding_expression(expression);
                    pieces.push(format!(
                        "__li_显示({})",
                        code.unwrap_or_else(|| "''".to_owned())
                    ));
                }
            }
        }

        if pieces.is_empty() {
            pieces.push("\"\"".to_owned());
        }

        let selector = format!("[data-li-node=\"{node_id}\"]");
        self.bindings.push(format!(
            "__li_绑定.push(() => {{ document.querySelector('{selector}').textContent = {}; }});",
            pieces.join(" + ")
        ));
    }

    /// 注册节点上的全部事件监听。
    fn register_events(&mut self, node: &ViewNode, node_id: Option<&str>) {
        if node.events.is_empty() {
            return;
        }

        let Some(node_id) = node_id else {
            return;
        };

        for event in &node.events {
            let Some(dom_event) = mapping::event(&event.name.name) else {
                self.diagnostics.push(
                    Diagnostic::error(format!("未知视图事件「{}」", event.name.name))
                        .with_span(event.name.span)
                        .with_expected("已支持的中文事件名")
                        .with_note(mapping::EVENT_HELP),
                );
                continue;
            };

            let mut translator =
                ScriptTranslator::with_names(&mut self.diagnostics, self.state_names.clone());
            let body = translator.handler(&event.body);
            let Some(body) = body else {
                continue;
            };

            let selector = format!("[data-li-node=\"{node_id}\"]");
            let indented = body
                .lines()
                .map(|line| format!("    {line}"))
                .collect::<Vec<_>>()
                .join("\n");
            self.listeners.push(format!(
                "document.querySelector('{selector}').addEventListener(\"{dom_event}\", function (__事件) {{\n{indented}\n    __li_渲染();\n}});"
            ));
        }
    }

    /// 分配一个新的节点编号。
    fn next_node_id(&mut self) -> String {
        let id = format!("li-n{}", self.next_node);
        self.next_node += 1;
        id
    }

    /// 把视图标签名解析为标准 HTML 标签名。
    fn resolve_tag(&mut self, name: &Ident) -> Option<String> {
        if let Some(tag) = mapping::html_tag(&name.name) {
            return Some(tag.to_owned());
        }

        if let Some(tag) = mapping::raw_html_tag(&name.name.to_ascii_lowercase()) {
            return Some(tag.to_owned());
        }

        self.diagnostics.push(
            Diagnostic::error("未知视图标签")
                .with_span(name.span)
                .with_expected("已支持的中文标签或标准 HTML 标签名")
                .with_actual(format!("「{}」", name.name))
                .with_note(mapping::TAG_HELP),
        );
        None
    }

    /// 渲染节点属性，返回带前导空格的属性片段。
    ///
    /// `node_id` 存在时，带 `{ 表达式 }` 值的属性会被注册为运行时绑定，
    /// 而非静态输出。
    fn render_attributes(&mut self, attributes: &[ViewAttribute], node_id: Option<&str>) -> String {
        let mut rendered = Vec::new();
        let mut seen = HashSet::new();

        if let Some(node_id) = node_id {
            rendered.push(format!(" data-li-node=\"{node_id}\""));
        }

        for attribute in attributes {
            let Some((html_name, boolean)) = self.resolve_attribute(attribute) else {
                continue;
            };

            if !seen.insert(html_name.clone()) {
                self.error_duplicate_attribute(attribute, &html_name);
                continue;
            }

            if let ViewValueKind::Expression(expression) = &attribute.value.kind {
                let Some(node_id) = node_id else {
                    self.diagnostics.push(
                        Diagnostic::error("绑定属性缺少运行时节点")
                            .with_span(attribute.value.span)
                            .with_note("请把绑定属性写在普通元素或 `文本` 节点上"),
                    );
                    continue;
                };

                let mut translator =
                    ScriptTranslator::with_names(&mut self.diagnostics, self.state_names.clone());
                let Some(code) = translator.binding_expression(expression) else {
                    continue;
                };

                self.interactive = true;
                let selector = format!("[data-li-node=\"{node_id}\"]");
                self.bindings.push(format!(
                    "__li_绑定.push(() => {{ const __值 = {code}; const __元素 = document.querySelector('{selector}'); if (__值 === false || __值 === null || __值 === undefined || __值 === '') {{ __元素.removeAttribute('{html_name}'); }} else {{ __元素.setAttribute('{html_name}', __li_显示(__值)); }} }});"
                ));
                continue;
            }

            if boolean {
                match &attribute.value.kind {
                    ViewValueKind::Bool(true) => {
                        rendered.push(format!(" {html_name}"));
                    }
                    ViewValueKind::Bool(false) => {}
                    _ => {
                        self.diagnostics.push(
                            Diagnostic::error("布尔属性的值必须是 `真` 或 `假`")
                                .with_span(attribute.value.span)
                                .with_expected("`真` 或 `假`")
                                .with_actual(describe_value(&attribute.value)),
                        );
                    }
                }
            } else if matches!(&attribute.value.kind, ViewValueKind::Bool(_)) {
                self.diagnostics.push(
                    Diagnostic::error("普通属性不能使用布尔值")
                        .with_span(attribute.value.span)
                        .with_note(
                            "只有 `禁用`、`只读`、`选中`、`必填` 等布尔属性可以写 `真` 或 `假`",
                        ),
                );
            } else {
                rendered.push(format!(
                    " {}=\"{}\"",
                    html_name,
                    escape::attribute(&value_text(&attribute.value))
                ));
            }
        }

        rendered.join("")
    }

    /// 把视图属性名解析为标准 HTML 属性名。
    ///
    /// 返回属性名与是否为布尔属性。中文属性经过映射表转换，
    /// 合法 ASCII 属性名直接使用并转为小写。
    fn resolve_attribute(&mut self, attribute: &ViewAttribute) -> Option<(String, bool)> {
        if let Some(spec) = mapping::attribute(&attribute.name.name) {
            return Some((spec.name.to_owned(), spec.boolean));
        }

        let lower = attribute.name.name.to_ascii_lowercase();

        if lower.starts_with("on") {
            self.diagnostics.push(
                Diagnostic::error("不支持 `on*` 事件属性")
                    .with_span(attribute.name.span)
                    .with_expected("`点击 { ... }` 形式的事件块")
                    .with_actual(format!("「{}」", attribute.name.name))
                    .with_note("事件必须写成节点内的 `点击 { 语句 }`"),
            );
            return None;
        }

        if is_valid_html_name(&attribute.name.name) {
            return Some((lower, false));
        }

        self.diagnostics.push(
            Diagnostic::error("未知视图属性")
                .with_span(attribute.name.span)
                .with_expected("已支持的中文属性或标准 HTML 属性名")
                .with_actual(format!("「{}」", attribute.name.name))
                .with_note(mapping::ATTRIBUTE_HELP),
        );
        None
    }

    fn error_duplicate_attribute(&mut self, attribute: &ViewAttribute, html_name: &str) {
        self.diagnostics.push(
            Diagnostic::error("属性重复")
                .with_span(attribute.name.span)
                .with_actual(format!("「{html_name}」"))
                .with_note("同一个视图节点不能重复声明同一个属性"),
        );
    }

    fn error_unknown_document_attribute(&mut self, attribute: &ViewAttribute) {
        self.diagnostics.push(
            Diagnostic::error("未知视图文档属性")
                .with_span(attribute.name.span)
                .with_expected("`标题`、`语言` 或 `描述`")
                .with_actual(format!("「{}」", attribute.name.name)),
        );
    }

    fn error_void_content(&mut self, node: &ViewNode, tag: &str) {
        self.diagnostics.push(
            Diagnostic::error(format!("标签「{}」不能包含内容", node.name.name))
                .with_span(node.span)
                .with_note(format!("`{tag}` 是空元素，只能使用属性")),
        );
    }
}

/// 判断节点是否需要运行时支撑（事件、绑定属性或文本插值）。
fn node_requires_runtime(node: &ViewNode) -> bool {
    !node.events.is_empty()
        || node
            .attributes
            .iter()
            .any(|attribute| matches!(attribute.value.kind, ViewValueKind::Expression(_)))
        || matches!(node.content, ViewContent::Parts(_))
}

/// 如果节点是纯 `文本` 叶子，返回它的文本，供父元素内联渲染。
fn inline_text_child(node: &ViewNode) -> Option<&str> {
    if node.name.name != "文本" || !node.attributes.is_empty() || !node.events.is_empty() {
        return None;
    }

    match &node.content {
        ViewContent::Text(text) => Some(text),
        ViewContent::Empty | ViewContent::Parts(_) | ViewContent::Children(_) => None,
    }
}

/// 判断名称是否可以直接作为标准 HTML 属性名使用。
///
/// 只接受 ASCII 字母开头、由 ASCII 字母、数字、下划线与连字符组成的名称；
/// 中文名称必须经过映射表。事件属性会在调用处提前拒绝。
fn is_valid_html_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };

    first.is_ascii_alphabetic()
        && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
}

/// 把属性值转换为 HTML 文本。
fn value_text(value: &ViewValue) -> String {
    match &value.kind {
        ViewValueKind::Text(text) => text.clone(),
        ViewValueKind::Integer(number) => number.to_string(),
        ViewValueKind::Float(number) => number.to_string(),
        ViewValueKind::Bool(value) => {
            if *value {
                "true".to_owned()
            } else {
                "false".to_owned()
            }
        }
        ViewValueKind::Identifier(name) => name.clone(),
        ViewValueKind::Expression(_) => String::new(),
    }
}

/// 为诊断生成属性值的中文描述。
fn describe_value(value: &ViewValue) -> String {
    match &value.kind {
        ViewValueKind::Text(text) => format!("字符串「{text}」"),
        ViewValueKind::Integer(number) => format!("整数 `{number}`"),
        ViewValueKind::Float(number) => format!("小数 `{number}`"),
        ViewValueKind::Bool(value) => format!("布尔值 `{}`", if *value { "真" } else { "假" }),
        ViewValueKind::Identifier(name) => format!("标识符 `{name}`"),
        ViewValueKind::Expression(_) => "绑定表达式".to_owned(),
    }
}

/// 带缩进的 HTML 写入器。
struct Writer {
    output: String,
    indent: usize,
}

impl Writer {
    fn new() -> Self {
        Self {
            output: String::new(),
            indent: 0,
        }
    }

    fn line(&mut self, text: impl AsRef<str>) {
        for _ in 0..self.indent {
            self.output.push_str("    ");
        }
        self.output.push_str(text.as_ref());
        self.output.push('\n');
    }

    fn open(&mut self, text: impl AsRef<str>) {
        self.line(text);
        self.indent += 1;
    }

    fn close(&mut self, text: impl AsRef<str>) {
        self.indent = self.indent.saturating_sub(1);
        self.line(text);
    }

    fn finish(self) -> String {
        self.output
    }
}
