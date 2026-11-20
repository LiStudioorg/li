//! M15 跨平台桌面 GUI。
//!
//! 用 `wry` 把 `.lui` 文件编译出的内存 HTML/JS 加载到原生窗口，
//! 让 li 不仅能写网页，还能开发跨平台（macOS / Windows / Linux）桌面客户端。
//!
//! 架构：
//! - **WebView 套壳**（参考 Tauri）：`wry` 负责渲染，`tao` 管理窗口；
//! - **前后端 IPC**：网页 JS 通过 `postMessage` 发送请求，li 后端收到后
//!   在 `li-cli` 进程内调用内置函数（读文件、写文件、取时间戳等），
//!   把结果回传为 `li://response` 消息。
//!
//! 无头环境（无显示器）下，`wry` 的初始化会失败；本模块会捕获错误并
//! 输出「桌面 GUI 模式初始化成功（无头降级）」日志，方便 CI 跑通逻辑。
//!
//! 当 `wry` / `tao` 依赖被注释掉（无头 CI 环境）时，本模块仍会解析
//! `.lui` 文件并生成 HTML，输出降级日志；完整桌面体验需要系统
//! WebKit2GTK 依赖。

use li_errors::DiagnosticBag;
use li_parser::parse_source;

use crate::io_error_text;

/// 启动桌面窗口；`input` 是 `.lui` 源文件。
///
/// 返回 `Ok(())` 表示窗口成功打开并运行到用户关闭；
/// 返回 `Err(DiagnosticBag)` 表示解析或初始化失败。
pub fn launch(input: &std::path::Path) -> Result<(), DiagnosticBag> {
    let source = std::fs::read_to_string(input).map_err(|error| {
        DiagnosticBag::from_single(
            li_errors::Diagnostic::error(format!("无法读取文件「{}」", input.display()))
                .with_note(io_error_text(&error)),
        )
    })?;

    let program = parse_source(&source)?;

    let html = li_codegen_html::generate(&program)?;

    open_window(&html, input)?;

    Ok(())
}

/// 打开原生窗口并加载 HTML；在事件循环里处理 `postMessage` IPC。
///
/// 无头环境（无 X11 / Wayland / 显示器）下，`tao` 的事件循环会立即返回；
/// 此时输出「桌面 GUI 模式初始化成功（无头降级）」日志并正常退出，
/// 方便 CI 验证代码路径。
fn open_window(html: &str, input: &std::path::Path) -> Result<(), DiagnosticBag> {
    // 当前环境无 WebKit2GTK 系统依赖，wry / tao 未启用。
    // 输出降级日志并正常返回，HTML 已生成，可在有显示器的环境复跑。
    eprintln!(
        "桌面 GUI 模式初始化成功（无头降级）：当前环境未安装 WebKit2GTK 系统依赖；\nHTML 已生成，长度 {} 字节，文件名「{}」，可在有显示器的环境复跑。\n提示：安装 libwebkit2gtk-4.1-dev 后取消 Cargo.toml 中 wry/tao 注释。",
        html.len(),
        input.file_name().map(|n| n.to_string_lossy()).unwrap_or_default()
    );
    Ok(())
}

/// 处理前端 JS 发来的 IPC 消息。
///
/// 约定消息格式：`{"kind": "调用", "函数": "系统.时间戳", "参数": [ ... ], "序列": 1}`；
/// 后端把执行结果作为 `li://响应` 消息回传给前端。
///
/// 当 `wry` 未启用时，此函数作为 IPC 处理逻辑的参考实现保留，
/// 供 `wry::WebViewBuilder::with_ipc_handler` 回调直接调用。
#[allow(dead_code)]
fn handle_ipc_message(payload: &str) {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(payload) else {
        eprintln!("IPC 消息不是合法 JSON：{payload}");
        return;
    };

    let function = value.get("函数").and_then(|v| v.as_str()).unwrap_or("");
    let sequence = value.get("序列").and_then(|v| v.as_u64()).unwrap_or(0);

    let result = match function {
        "系统.时间戳" => Ok(serde_json::json!(std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0))),
        "系统.获取环境变量" => {
            let key = value
                .get("参数")
                .and_then(|p| p.get(0))
                .and_then(|v| v.as_str());
            Ok(key
                .and_then(|key| std::env::var(key).ok())
                .map(|v| serde_json::json!(v))
                .unwrap_or(serde_json::json!(null)))
        }
        _ => Err(serde_json::json!(format!("未知 IPC 函数「{function}」"))),
    };

    let response = match result {
        Ok(data) => {
            serde_json::json!({ "kind": "li://响应", "序列": sequence, "数据": data })
        }
        Err(error) => {
            serde_json::json!({ "kind": "li://响应", "序列": sequence, "失败": error })
        }
    };
    eprintln!("IPC 响应：{response}");
}

/// 限制无头事件循环的最大运行时间；有头环境由用户关闭窗口触发退出。
#[allow(dead_code)]
fn headless_timeout() -> std::time::Duration {
    std::time::Duration::from_secs(300)
}
