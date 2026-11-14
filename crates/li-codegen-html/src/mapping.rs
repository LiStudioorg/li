//! 中文视图名称到标准 HTML 标签与属性的映射。

/// 一个 HTML 属性的映射结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct AttributeSpec {
    /// 标准 HTML 属性名。
    pub name: &'static str,
    /// 是否为布尔属性；布尔属性只输出属性名，不输出值。
    pub boolean: bool,
}

/// 把中文视图标签映射为标准 HTML 标签。
pub(crate) fn html_tag(name: &str) -> Option<&'static str> {
    let tag = match name {
        "容器" | "区块" => "div",
        "区域" => "section",
        "页面" => "main",
        "头部" => "header",
        "底部" => "footer",
        "导航" => "nav",
        "文章" => "article",
        "侧栏" => "aside",
        "标题" | "主标题" | "一级标题" => "h1",
        "副标题" | "二级标题" => "h2",
        "三级标题" => "h3",
        "四级标题" => "h4",
        "五级标题" => "h5",
        "六级标题" => "h6",
        "段落" => "p",
        "强调" | "加粗" => "strong",
        "斜体" => "em",
        "标记" => "mark",
        "小号文本" => "small",
        "引用" => "blockquote",
        "代码" => "code",
        "预格式化" => "pre",
        "链接" => "a",
        "按钮" => "button",
        "图片" => "img",
        "换行" => "br",
        "分隔线" => "hr",
        "列表" => "ul",
        "有序列表" => "ol",
        "列表项" => "li",
        "表格" => "table",
        "表头" => "thead",
        "表体" => "tbody",
        "行" => "tr",
        "单元格" => "td",
        "表头单元格" => "th",
        "表单" => "form",
        "标签" => "label",
        "输入框" => "input",
        "多行输入" => "textarea",
        "下拉框" => "select",
        "选项" => "option",
        "分组" => "fieldset",
        "图例" => "legend",
        "音频" => "audio",
        "视频" => "video",
        "画布" => "canvas",
        "详情" => "details",
        "摘要" => "summary",
        "进度" => "progress",
        "度量" => "meter",
        "上标" => "sup",
        "下标" => "sub",
        "删除线" => "del",
        "插入" => "ins",
        _ => return None,
    };

    Some(tag)
}

/// 允许直接使用的标准 HTML 标签。
///
/// 只开放安全的静态结构标签；`script`、`style`、`iframe` 等标签
/// 不属于第一版静态 HTML 后端的范围。
pub(crate) fn raw_html_tag(name: &str) -> Option<&'static str> {
    let tag = match name {
        "div" => "div",
        "section" => "section",
        "main" => "main",
        "header" => "header",
        "footer" => "footer",
        "nav" => "nav",
        "article" => "article",
        "aside" => "aside",
        "h1" => "h1",
        "h2" => "h2",
        "h3" => "h3",
        "h4" => "h4",
        "h5" => "h5",
        "h6" => "h6",
        "p" => "p",
        "strong" => "strong",
        "em" => "em",
        "mark" => "mark",
        "small" => "small",
        "blockquote" => "blockquote",
        "code" => "code",
        "pre" => "pre",
        "a" => "a",
        "button" => "button",
        "img" => "img",
        "br" => "br",
        "hr" => "hr",
        "ul" => "ul",
        "ol" => "ol",
        "li" => "li",
        "span" => "span",
        "table" => "table",
        "thead" => "thead",
        "tbody" => "tbody",
        "tr" => "tr",
        "td" => "td",
        "th" => "th",
        "form" => "form",
        "label" => "label",
        "input" => "input",
        "textarea" => "textarea",
        "select" => "select",
        "option" => "option",
        "fieldset" => "fieldset",
        "legend" => "legend",
        "audio" => "audio",
        "video" => "video",
        "canvas" => "canvas",
        "details" => "details",
        "summary" => "summary",
        "progress" => "progress",
        "meter" => "meter",
        "sup" => "sup",
        "sub" => "sub",
        "del" => "del",
        "ins" => "ins",
        _ => return None,
    };

    Some(tag)
}

/// 判断标准 HTML 标签是否为空元素。
pub(crate) fn is_void_tag(tag: &str) -> bool {
    matches!(tag, "br" | "hr" | "img" | "input" | "meta" | "link")
}

/// 把中文视图属性映射为标准 HTML 属性。
pub(crate) fn attribute(name: &str) -> Option<AttributeSpec> {
    let (name, boolean) = match name {
        "类" => ("class", false),
        "标识" => ("id", false),
        "标题" => ("title", false),
        "样式" => ("style", false),
        "链接" => ("href", false),
        "来源" => ("src", false),
        "替代" | "替代文本" => ("alt", false),
        "类型" => ("type", false),
        "名称" => ("name", false),
        "值" => ("value", false),
        "占位" | "占位符" => ("placeholder", false),
        "宽度" => ("width", false),
        "高度" => ("height", false),
        "目标" | "打开方式" => ("target", false),
        "语言" => ("lang", false),
        "角色" => ("role", false),
        "编号" => ("for", false),
        "动作" => ("action", false),
        "方法" => ("method", false),
        "行数" => ("rows", false),
        "列数" => ("cols", false),
        "最大长度" => ("maxlength", false),
        "最小" => ("min", false),
        "最大" => ("max", false),
        "步骤" => ("step", false),
        "列宽" => ("colspan", false),
        "行宽" => ("rowspan", false),
        "下载" => ("download", false),
        "自动完成" => ("autocomplete", false),
        "接受" => ("accept", false),
        "预加载" => ("preload", false),
        "禁用" => ("disabled", true),
        "只读" => ("readonly", true),
        "选中" => ("checked", true),
        "必填" => ("required", true),
        "多选" => ("multiple", true),
        "自动聚焦" => ("autofocus", true),
        "控制" => ("controls", true),
        "循环" => ("loop", true),
        "静音" => ("muted", true),
        "自动播放" => ("autoplay", true),
        _ => return None,
    };

    Some(AttributeSpec { name, boolean })
}

/// 未知标签诊断中展示的常用标签提示。
pub(crate) const TAG_HELP: &str =
    "常用标签：容器、区域、标题、段落、文本、按钮、链接、图片、列表、列表项、表格、表单、输入框";

/// 未知属性诊断中展示的常用属性提示。
pub(crate) const ATTRIBUTE_HELP: &str =
    "常用属性：类、标识、样式、链接、来源、替代、类型、名称、值、占位、宽度、高度、禁用";
