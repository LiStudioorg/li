//! `li-codegen-html` 的代码生成测试。

/// 解析 `.lui` 源码并生成 HTML。
fn generate(source: &str) -> Result<String, li_errors::DiagnosticBag> {
    let program =
        li_parser::parse_source(source).unwrap_or_else(|errors| panic!("语法分析失败：\n{errors}"));
    li_codegen_html::generate(&program)
}

/// 解析并断言 HTML 生成成功。
fn generate_ok(source: &str) -> String {
    generate(source).unwrap_or_else(|errors| panic!("HTML 生成失败：\n{errors}"))
}

/// 解析并断言 HTML 生成失败，返回全部诊断。
fn generate_err(source: &str) -> li_errors::DiagnosticBag {
    generate(source).expect_err("预期 HTML 生成失败，但编译器接受了视图")
}

#[test]
fn generates_static_html_from_view_fixture() {
    let html = generate_ok(include_str!("../../li-parser/tests/fixtures/hello.lui"));

    let expected = "\
<!DOCTYPE html>
<!-- 本文件由 li 编译器自动生成，请勿手动编辑。 -->
<html lang=\"zh-CN\">
    <head>
        <meta charset=\"UTF-8\">
        <meta name=\"viewport\" content=\"width=device-width, initial-scale=1.0\">
        <title>你好，li</title>
    </head>
    <body>
        <div>
            <h1>你好，li！</h1>
            <p>这是由 li 编译器从 .lui 视图文件生成的静态页面。</p>
            <ul>
                <li>声明式视图</li>
                <li>中文标签</li>
            </ul>
            <button class=\"主按钮\" type=\"button\">点我</button>
        </div>
    </body>
</html>
";

    assert_eq!(html, expected);
}

#[test]
fn renders_document_title_language_and_description() {
    let html = generate_ok("视图 { 标题: 「页面」, 语言: 「zh-CN」, 描述: 「说明 & 内容」 }");

    assert!(
        html.contains(r#"<html lang="zh-CN">"#),
        "缺少语言属性：\n{html}"
    );
    assert!(
        html.contains("<title>页面</title>"),
        "缺少文档标题：\n{html}"
    );
    assert!(
        html.contains(r#"<meta name="description" content="说明 &amp; 内容">"#),
        "缺少文档描述或未转义：\n{html}"
    );
}

#[test]
fn escapes_text_and_attribute_values() {
    let html = generate_ok(
        r#"视图 { 容器 { 标题: 「A & B <C> "D"」, 文本 { 「<script>alert(1)</script>」 } } }"#,
    );

    assert!(
        html.contains(r#"<div title="A &amp; B &lt;C&gt; &quot;D&quot;">"#),
        "属性值未正确转义：\n{html}"
    );
    assert!(
        html.contains("&lt;script&gt;alert(1)&lt;/script&gt;"),
        "文本未正确转义：\n{html}"
    );
    assert!(
        !html.contains("<script>alert(1)</script>"),
        "不应输出原始脚本标签：\n{html}"
    );
}

#[test]
fn accepts_raw_ascii_tags_and_attributes() {
    let html = generate_ok("视图 { div { class: 「卡片」, 文本 { 「内容」 } } }");

    assert!(
        html.contains(r#"<div class="卡片">内容</div>"#),
        "HTML：\n{html}"
    );
}

#[test]
fn renders_boolean_and_normal_attributes() {
    let html =
        generate_ok("视图 { 输入框 { 类型: 「文本」, 占位: 「名字」, 必填: 真, 禁用: 假 } }");

    assert!(
        html.contains(r#"<input type="文本" placeholder="名字" required>"#),
        "HTML：\n{html}"
    );
    assert!(!html.contains("disabled"), "假值不应输出布尔属性：\n{html}");
}

#[test]
fn wraps_attributed_text_in_span() {
    let html = generate_ok("视图 { 文本 { 类: 「高亮」, 「你好」 } }");

    assert!(
        html.contains(r#"<span class="高亮">你好</span>"#),
        "HTML：\n{html}"
    );
}

#[test]
fn rejects_script_tags_in_the_static_html_backend() {
    let errors = generate_err("视图 { script { 文本 { 「alert(1)」 } } }");
    let message = errors.to_string();

    assert!(message.contains("未知视图标签"), "诊断：\n{message}");
    assert!(message.contains("「script」"), "诊断：\n{message}");
}

#[test]
fn rejects_event_attributes_in_the_static_html_backend() {
    let errors = generate_err("视图 { 按钮 { onclick: 「alert(1)」, 文本 { 「点我」 } } }");
    let message = errors.to_string();

    assert!(
        message.contains("不支持 `on*` 事件属性"),
        "诊断：\n{message}"
    );
    assert!(
        message.contains("点击 { ... }"),
        "诊断应提示事件块写法：\n{message}"
    );
    assert!(message.contains("「onclick」"), "诊断：\n{message}");
}

#[test]
fn rejects_unknown_chinese_tag() {
    let errors = generate_err("视图 { 未知标签 { } }");
    let message = errors.to_string();

    assert!(message.contains("未知视图标签"), "诊断：\n{message}");
    assert!(message.contains("第 1 行"), "诊断：\n{message}");
    assert!(message.contains("「未知标签」"), "诊断：\n{message}");
}

#[test]
fn rejects_unknown_attribute() {
    let errors = generate_err("视图 { 容器 { 未知属性: 「值」 } }");
    let message = errors.to_string();

    assert!(message.contains("未知视图属性"), "诊断：\n{message}");
    assert!(message.contains("「未知属性」"), "诊断：\n{message}");
}

#[test]
fn rejects_duplicate_attributes() {
    let errors = generate_err("视图 { 容器 { 类: 「甲」, 类: 「乙」 } }");
    let message = errors.to_string();

    assert!(message.contains("属性重复"), "诊断：\n{message}");
    assert!(message.contains("「class」"), "诊断：\n{message}");
}

#[test]
fn rejects_content_inside_void_elements() {
    let errors = generate_err("视图 { 图片 { 文本 { 「图」 } } }");
    let message = errors.to_string();

    assert!(
        message.contains("标签「图片」不能包含内容"),
        "诊断：\n{message}"
    );
    assert!(message.contains("`img` 是空元素"), "诊断：\n{message}");
}

#[test]
fn rejects_multiple_view_declarations() {
    let errors = generate_err("视图 { 容器 { } } 视图 { 按钮 { } }");
    let message = errors.to_string();

    assert!(
        message.contains("一个视图文件只能包含一个视图声明"),
        "诊断：\n{message}"
    );
}

#[test]
fn rejects_a_program_without_views() {
    let errors = generate_err("函数 主() -> 整数 { 返回 0; }");
    let message = errors.to_string();

    assert!(message.contains("未找到视图声明"), "诊断：\n{message}");
}

#[test]
fn rejects_boolean_values_on_normal_attributes() {
    let errors = generate_err("视图 { 容器 { 类: 真 } }");
    let message = errors.to_string();

    assert!(
        message.contains("普通属性不能使用布尔值"),
        "诊断：\n{message}"
    );
}

#[test]
fn rejects_a_text_node_without_text() {
    let errors = generate_err("视图 { 文本 { } }");
    let message = errors.to_string();

    assert!(
        message.contains("`文本` 节点需要字符串内容"),
        "诊断：\n{message}"
    );
}

#[test]
fn generates_single_file_interactive_html() {
    let html = generate_ok(include_str!("../../li-parser/tests/fixtures/counter.lui"));

    assert!(
        !html.contains("src=") && !html.contains("<link"),
        "单体产物不能引用外部脚本或样式：\n{html}"
    );
    assert!(html.contains("<script>"), "缺少内联脚本：\n{html}");
    assert!(
        html.contains("let 次数 = 0;") && html.contains("let 提示 = \"尚未点击\";"),
        "状态声明未内联：\n{html}"
    );
    assert!(
        html.contains("addEventListener(\"click\""),
        "点击事件未注册：\n{html}"
    );
    assert!(
        html.contains("data-li-node=\""),
        "缺少运行时节点标记：\n{html}"
    );
}

#[test]
fn runs_generated_interaction_in_node_with_dom_shim() {
    use std::fs;
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};

    if Command::new("node").arg("--version").output().is_err() {
        eprintln!("未找到 Node.js，跳过交互运行验证。");
        return;
    }

    let html = generate_ok(include_str!("../../li-parser/tests/fixtures/counter.lui"));
    let script = html
        .split_once("<script>")
        .and_then(|(_, rest)| rest.split_once("</script>"))
        .map(|(script, _)| script)
        .expect("生成产物缺少内联脚本");

    let harness = format!(
        r#"
const 元素 = new Map();
function 取元素(选择器) {{
    const 匹配 = /^\[data-li-node="(.+)"\]$/.exec(选择器);
    const 编号 = 匹配 ? 匹配[1] : 选择器;
    if (!元素.has(编号)) {{
        元素.set(编号, {{
            textContent: "",
            attributes: new Map(),
            listeners: {{}},
            setAttribute(键, 值) {{ this.attributes.set(键, String(值)); }},
            removeAttribute(键) {{ this.attributes.delete(键); }},
            addEventListener(类型, 处理器) {{
                (this.listeners[类型] ||= []).push(处理器);
            }},
        }});
    }}
    return 元素.get(编号);
}}
globalThis.document = {{ querySelector: 取元素 }};
{script}
function 快照(标题) {{
    for (const [编号, 节点] of 元素) {{
        console.log(标题, 编号, JSON.stringify(节点.textContent));
    }}
}}
快照("初始");
for (const [, 节点] of 元素) {{
    for (const 处理器 of (节点.listeners.click || [])) {{
        处理器({{ type: "click" }});
    }}
}}
快照("点击");
"#
    );

    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("系统时间早于 UNIX 纪元")
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "li-html-interaction-{}-{nanos}.js",
        std::process::id()
    ));
    fs::write(&path, harness).expect("无法写入临时脚本");

    let output = Command::new("node")
        .arg(&path)
        .output()
        .expect("无法启动 Node.js");
    let _ = fs::remove_file(&path);

    assert!(
        output.status.success(),
        "交互脚本执行失败：\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("\"当前计数：0\""),
        "初始渲染不正确：\n{stdout}"
    );
    assert!(
        stdout.contains("\"尚未点击\""),
        "初始提示不正确：\n{stdout}"
    );
    assert!(
        stdout.contains("\"当前计数：1\""),
        "点击后计数未更新：\n{stdout}"
    );
    assert!(
        stdout.contains("\"已点击\""),
        "点击后提示未更新：\n{stdout}"
    );
}

#[test]
fn reports_undefined_variable_in_event_handler() {
    let errors =
        generate_err("视图 { 容器 { 按钮 { 点击 { 计数 = 计数 加 1; } 文本 { 「点我」 } } } }");
    let message = errors.to_string();

    assert!(
        message.contains("未定义的变量「计数」"),
        "诊断：\n{message}"
    );
    assert!(message.contains("状态"), "诊断应提示状态声明：\n{message}");
}

#[test]
fn rejects_unsupported_statements_in_view_script() {
    let errors = generate_err(
        "视图 { 状态 { 让 可变 次数 = 0; } 按钮 { 点击 { 返回 0; } 文本 { 「点我」 } } }",
    );
    let message = errors.to_string();

    assert!(
        message.contains("视图交互代码暂不支持"),
        "诊断：\n{message}"
    );
    assert!(message.contains("`返回`"), "诊断：\n{message}");
}

#[test]
fn reports_non_declaration_in_state_block() {
    let errors = generate_err("视图 { 状态 { 次数 = 0; } 容器 { 文本 { 「你好」 } } }");
    let message = errors.to_string();

    assert!(
        message.contains("`状态` 块只能包含变量声明"),
        "诊断：\n{message}"
    );
}
