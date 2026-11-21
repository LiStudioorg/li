//! li 编译器命令行入口。
//!
//! 提供以下主要命令：
//! - `li 获取 [目录]`：解析 `li.toml`，获取依赖并生成 `li.lock`；
//! - `li 运行 [文件.li]`：编译并直接在虚拟机中执行；
//! - `li 编译 <文件.li>`：编译为同名 `.lic` 字节码文件；
//! - `li 执行 <文件.lic>`：在栈式虚拟机中加载并执行字节码；
//! - `li 编译-js <文件.li>`：编译为同名 JavaScript 文件；
//! - `li 编译-llvm <文件.li>`：生成同名文本 LLVM IR（`.ll`）；
//! - `li 编译-原生 <文件.li> [--target <三元组>]`：调用外部 clang 把 `.ll`
//!   链接为原生二进制；指定目标三元组时自动启用静态链接，用于交叉编译到
//!   Android / Termux 等缺少 glibc 的场景。
//! - `li 编译-html <文件.lui>`：把视图文件编译为同名单体 HTML 文件；
//! - `li 服务 <文件.lui> [端口]`：编译视图并在内存中直接提供 HTTP 服务；
//! - `li 桌面 <文件.lui>`：M15 跨平台桌面 GUI，把 `.lui` 加载到原生窗口。
//!
//! 前端流程完全复用：语法分析 → 依赖链接 → 语义检查 → 中立 IR 降级 →
//! 后端代码生成；HTML 后端直接消费视图 AST，不经过 IR。

mod desktop;

use std::env;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();

    let Some(command) = args.first() else {
        print_usage();
        return ExitCode::SUCCESS;
    };

    match command.as_str() {
        "获取" => {
            if args.len() > 2 {
                eprintln!("错误：命令「获取」最多接收一个项目目录参数");
                print_usage();
                return ExitCode::FAILURE;
            }
            let directory = args
                .get(1)
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."));
            fetch_project(&directory)
        }
        "运行" => {
            if args.len() > 2 {
                eprintln!("错误：命令「运行」最多接收一个 .li 文件参数");
                print_usage();
                return ExitCode::FAILURE;
            }
            match args.get(1) {
                Some(path) => run_source(Path::new(path)),
                None => match default_project_entry() {
                    Ok(entry) => run_source(&entry),
                    Err(code) => code,
                },
            }
        }
        "编译" => {
            if args.len() != 2 {
                eprintln!("错误：命令「编译」需要一个 .li 文件参数");
                print_usage();
                return ExitCode::FAILURE;
            }
            compile_to_bytecode(Path::new(&args[1]))
        }
        "执行" => {
            if args.len() != 2 {
                eprintln!("错误：命令「执行」需要一个 .lic 文件参数");
                print_usage();
                return ExitCode::FAILURE;
            }
            execute_bytecode(Path::new(&args[1]))
        }
        "编译-js" => {
            if args.len() != 2 {
                eprintln!("错误：命令「编译-js」需要一个 .li 文件参数");
                print_usage();
                return ExitCode::FAILURE;
            }
            compile_to_javascript(Path::new(&args[1]))
        }
        "编译-llvm" => {
            if args.len() != 2 {
                eprintln!("错误：命令「编译-llvm」需要一个 .li 文件参数");
                print_usage();
                return ExitCode::FAILURE;
            }
            compile_to_llvm(Path::new(&args[1]))
        }
        "编译-原生" => {
            if args.len() < 2 {
                eprintln!(
                    "错误：命令「编译-原生」需要一个 .li 文件参数（可选 --target、--release）"
                );
                print_usage();
                return ExitCode::FAILURE;
            }

            let mut target: Option<String> = None;
            let mut release = false;
            let mut index = 2;
            while index < args.len() {
                match args[index].as_str() {
                    "--target" => match args.get(index + 1) {
                        Some(triple) => {
                            target = Some(triple.clone());
                            index += 2;
                        }
                        None => {
                            eprintln!("错误：--target 参数缺少目标三元组");
                            return ExitCode::FAILURE;
                        }
                    },
                    "--release" => {
                        release = true;
                        index += 1;
                    }
                    other => {
                        eprintln!(
                            "错误：无法识别的参数「{other}」；支持 --target <三元组> 与 --release"
                        );
                        return ExitCode::FAILURE;
                    }
                }
            }

            compile_to_native(Path::new(&args[1]), target.as_deref(), release)
        }
        "编译-html" => {
            if args.len() != 2 {
                eprintln!("错误：命令「编译-html」需要一个 .lui 文件参数");
                print_usage();
                return ExitCode::FAILURE;
            }
            compile_to_html(Path::new(&args[1]))
        }
        "服务" => {
            if args.len() < 2 || args.len() > 3 {
                eprintln!("错误：命令「服务」需要一个 .lui 文件参数与可选端口");
                print_usage();
                return ExitCode::FAILURE;
            }

            let port = match args.get(2) {
                Some(text) => match text.parse::<u16>() {
                    Ok(port) => port,
                    Err(_) => {
                        eprintln!("错误：端口必须是 0 到 65535 之间的整数，实际为「{text}」");
                        return ExitCode::FAILURE;
                    }
                },
                None => 8080,
            };

            serve_view(Path::new(&args[1]), port)
        }
        "桌面" => {
            if args.len() != 2 {
                eprintln!("错误：命令「桌面」需要一个 .lui 文件参数");
                print_usage();
                return ExitCode::FAILURE;
            }
            run_desktop(Path::new(&args[1]))
        }
        "版本" | "--version" | "-V" => {
            println!("li 编译器 v{}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        "帮助" | "--help" | "-h" => {
            print_usage();
            ExitCode::SUCCESS
        }
        other => {
            eprintln!("错误：未知命令「{other}」");
            print_usage();
            ExitCode::FAILURE
        }
    }
}

fn print_usage() {
    println!("li 编译器 v{}", env!("CARGO_PKG_VERSION"));
    println!("用法：");
    println!("  li 获取 [目录]         获取 li.toml 声明的依赖并生成 li.lock");
    println!("  li 运行 [文件.li]      编译并直接运行；省略文件时使用项目入口");
    println!("  li 编译 <文件.li>      将 li 源码编译为同名 .lic 字节码文件");
    println!("  li 执行 <文件.lic>     在虚拟机中加载并执行字节码");
    println!("  li 编译-js <文件.li>   将 li 源码编译为同名 .js 文件");
    println!("  li 编译-llvm <文件.li> 生成同名文本 LLVM IR（.ll）");
    println!("  li 编译-原生 <文件.li> [--target t]  通过 clang 链接为原生二进制（可交叉编译 + 静态链接）");
    println!("  li 编译-html <文件.lui> 将 .lui 视图文件编译为同名 .html 文件");
    println!("  li 服务 <文件.lui> [端口] 编译单体视图并启动 HTTP 服务（默认 8080）");
    println!("  li 桌面 <文件.lui>    M15 跨平台桌面 GUI：加载 .lui 到原生窗口");
    println!("  li 版本                显示版本信息");
    println!("  li 帮助                显示帮助信息");
}

/// 依次执行读取、语法分析、语义检查与 IR 降级。
///
/// 如果文件位于 li 项目根目录，则读取 `li.toml` 的 `[检查]` 表覆盖默认等级；
/// 函数/模块上的 `@检查(...)` 注解会进一步覆盖项目级配置。
fn compile_to_ir(input: &Path) -> Result<li_ir::Program, ExitCode> {
    if input.extension().and_then(|extension| extension.to_str()) == Some("lui") {
        eprintln!(
            "错误：可执行代码编译命令不处理 .lui 视图文件，请改用「li 编译-html {}」",
            input.display()
        );
        return Err(ExitCode::FAILURE);
    }

    let source = match fs::read_to_string(input) {
        Ok(source) => source,
        Err(error) => {
            eprintln!(
                "错误：无法读取文件「{}」：{}",
                input.display(),
                io_error_text(&error)
            );
            return Err(ExitCode::FAILURE);
        }
    };

    let program = match li_parser::parse_source(&source) {
        Ok(program) => program,
        Err(errors) => {
            eprintln!("{}", li_errors::render(&errors, true));
            return Err(ExitCode::FAILURE);
        }
    };

    if contains_view(&program.items) {
        eprintln!("错误：可执行代码编译命令不处理视图声明，请改用「li 编译-html <文件.lui>」");
        return Err(ExitCode::FAILURE);
    }

    let program = match li_package::prepare_compile(input, &program) {
        Ok(program) => program,
        Err(errors) => {
            eprintln!("{}", li_errors::render(&errors, true));
            return Err(ExitCode::FAILURE);
        }
    };

    // 强制警告机制：li-sema 的安全类违规一律产生 Warning（黄色），
    // 编译继续。只有真正的语法错误、类型不匹配等致命问题才产生
    // Error 拦截编译。成功路径也要把警告打印到 stderr，让开发者看到
    // 黄色提示，但产物（.lic / .js / .ll / 原生二进制）仍然生成。
    match li_sema::check(&program) {
        Ok(Some(warnings)) => {
            eprintln!("{}", li_errors::render(&warnings, true));
        }
        Ok(None) => {}
        Err(errors) => {
            eprintln!("{}", li_errors::render(&errors, true));
            return Err(ExitCode::FAILURE);
        }
    }

    match li_ir::lower(&program) {
        Ok(ir) => Ok(ir),
        Err(errors) => {
            eprintln!("{}", li_errors::render(&errors, true));
            Err(ExitCode::FAILURE)
        }
    }
}

/// 解析 `li.toml`、获取全部依赖并写入 `li.lock`。
fn fetch_project(directory: &Path) -> ExitCode {
    let start = if directory.is_file() {
        directory.parent().unwrap_or_else(|| Path::new("."))
    } else {
        directory
    };

    if !start.exists() {
        eprintln!("错误：目录「{}」不存在", start.display());
        return ExitCode::FAILURE;
    }
    if !start.is_dir() {
        eprintln!("错误：「{}」不是目录", start.display());
        return ExitCode::FAILURE;
    }

    let root = if start.join(li_package::MANIFEST_FILE).is_file() {
        start.to_path_buf()
    } else {
        match li_package::find_project_root(start) {
            Some(root) => root,
            None => {
                eprintln!(
                    "错误：在「{}」及其上级目录中未找到项目配置「{}」",
                    start.display(),
                    li_package::MANIFEST_FILE
                );
                eprintln!("提示：请先在项目根目录创建 li.toml，再运行「li 获取」");
                return ExitCode::FAILURE;
            }
        }
    };

    let result = match li_package::fetch(&root) {
        Ok(result) => result,
        Err(errors) => {
            eprintln!("{errors}");
            return ExitCode::FAILURE;
        }
    };

    if result.fetched.is_empty() {
        println!("项目未声明依赖，已生成空的依赖锁文件。");
    } else {
        println!("已获取 {} 个依赖：", result.fetched.len());
        for dependency in &result.fetched {
            match &dependency.commit {
                Some(commit) => println!(
                    "  {}：{}（提交 {}）",
                    dependency.name, dependency.source, commit
                ),
                None => println!("  {}：{}", dependency.name, dependency.source),
            }
        }
    }

    println!("依赖锁文件：{}", root.join(li_package::LOCK_FILE).display());
    ExitCode::SUCCESS
}

/// 编译 .li 源码并直接在虚拟机中执行。
fn run_source(input: &Path) -> ExitCode {
    if input.extension().and_then(|extension| extension.to_str()) == Some("lic") {
        eprintln!(
            "错误：命令「运行」需要 .li 源码文件；执行字节码请使用「li 执行 {}」",
            input.display()
        );
        return ExitCode::FAILURE;
    }

    let ir = match compile_to_ir(input) {
        Ok(ir) => ir,
        Err(code) => return code,
    };

    let bytecode = li_codegen_bytecode::generate(&ir);

    if let Some(extern_name) = bytecode
        .functions
        .iter()
        .find_map(|function| function.is_extern.then_some(function.name.clone()))
    {
        eprintln!(
            "错误：外部 函数「{extern_name}」无法在 VM 中绑定动态链接库；\
             请改用 `li 编译-原生` 生成原生二进制"
        );
        return ExitCode::FAILURE;
    }

    if let Err(message) = bytecode.validate() {
        eprintln!("错误：生成的字节码校验失败：{message}");
        return ExitCode::FAILURE;
    }

    // M13：默认走异步入口（Tokio 多线程运行时），`网络.获取` 在 I/O 挂起
    // 时不阻塞 Tokio 工作线程；同步的 li_vm::execute 仍可用于纯计算程序。
    let result = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => {
            let program = std::sync::Arc::new(bytecode.clone());
            let output = runtime.block_on(async move { li_vm::execute_async(&program).await });
            match output {
                Ok(()) => Ok(ExitCode::SUCCESS),
                Err(error) => {
                    eprintln!("{error}");
                    Ok(ExitCode::FAILURE)
                }
            }
        }
        Err(error) => {
            eprintln!("错误：无法创建 Tokio 运行时：{error}");
            Ok(ExitCode::FAILURE)
        }
    };
    match result {
        Ok(code) => code,
        Err(code) => code,
    }
}

/// 在项目根目录中定位默认入口文件。
fn default_project_entry() -> Result<PathBuf, ExitCode> {
    let current = match env::current_dir() {
        Ok(current) => current,
        Err(error) => {
            eprintln!("错误：无法读取当前目录：{}", io_error_text(&error));
            return Err(ExitCode::FAILURE);
        }
    };

    let Some(root) = li_package::find_project_root(&current) else {
        eprintln!(
            "错误：当前目录不在 li 项目中，未找到「{}」",
            li_package::MANIFEST_FILE
        );
        eprintln!("提示：请传入 .li 文件，或在项目根目录创建 li.toml 后运行「li 运行」");
        return Err(ExitCode::FAILURE);
    };

    let manifest = match li_package::Manifest::load(&root) {
        Ok(manifest) => manifest,
        Err(errors) => {
            eprintln!("{errors}");
            return Err(ExitCode::FAILURE);
        }
    };

    match li_package::project_entry(&root, &manifest) {
        Ok(entry) => Ok(entry),
        Err(errors) => {
            eprintln!("{errors}");
            Err(ExitCode::FAILURE)
        }
    }
}

fn compile_to_javascript(input: &Path) -> ExitCode {
    let ir = match compile_to_ir(input) {
        Ok(ir) => ir,
        Err(code) => return code,
    };

    let javascript = match li_codegen_js::generate(&ir) {
        Ok(javascript) => javascript,
        Err(message) => {
            eprintln!("错误：{message}");
            return ExitCode::FAILURE;
        }
    };
    let output = input.with_extension("js");

    if let Err(error) = fs::write(&output, javascript) {
        eprintln!(
            "错误：无法写入文件「{}」：{}",
            output.display(),
            io_error_text(&error)
        );
        return ExitCode::FAILURE;
    }

    println!("已生成 JavaScript：{}", output.display());
    ExitCode::SUCCESS
}

/// 把 li 源码生成为同名的文本 LLVM IR（`.ll`）。
///
/// 该命令只依赖编译器本身，不需要本机安装 LLVM；生成的 `.ll`
/// 可以后续交给 `clang` 或 `llc` 生成原生可执行文件。
fn compile_to_llvm(input: &Path) -> ExitCode {
    let ir = match compile_to_ir(input) {
        Ok(ir) => ir,
        Err(code) => return code,
    };

    let (module, diagnostics) = li_codegen_llvm::generate_reporting(&ir, None);
    if !diagnostics.is_empty() {
        eprintln!("{}", li_errors::render(&diagnostics, true));
    }
    let llvm_ir = match module {
        Some(llvm_ir) => llvm_ir,
        None => return ExitCode::FAILURE,
    };
    let output = input.with_extension("ll");

    if let Err(error) = fs::write(&output, llvm_ir) {
        eprintln!(
            "错误：无法写入文件「{}」：{}",
            output.display(),
            io_error_text(&error)
        );
        return ExitCode::FAILURE;
    }

    println!("已生成 LLVM IR：{}", output.display());
    ExitCode::SUCCESS
}

/// 把 li 源码通过 clang 链接为原生二进制；指定 `--target` 时启用静态链接
/// 以支持交叉编译（Android / Termux / musl 等缺少 glibc 的目标）。
///
/// `--release` 会启用空间优化（`-Os`）、按节裁剪（`-ffunction-sections`
/// 与 `--gc-sections`）并剥离符号表（`-s`），以尽量减小二进制体积。
fn compile_to_native(input: &Path, target: Option<&str>, release: bool) -> ExitCode {
    // Android 目标必须依赖 NDK 提供的 bionic libc 与 crt，宿主机的
    // glibc sysroot 无法替代，因此在编译前先做一次环境预检。
    if let Some(triple) = target {
        if triple.contains("android") {
            let architecture = triple.split('-').next().unwrap_or("aarch64");
            if !android_toolchain_available(architecture) {
                eprintln!("错误：未找到 Android NDK 工具链，无法交叉编译到「{triple}」。");
                eprintln!("提示：请安装 Android NDK，并让 li 能找到 NDK 的 clang：");
                eprintln!("  export ANDROID_NDK_ROOT=$HOME/android-ndk-r26d");
                eprintln!(
                    "  export PATH=$ANDROID_NDK_ROOT/toolchains/llvm/prebuilt/linux-x86_64/bin:$PATH"
                );
                eprintln!("若目标只是在 Termux 中运行，可改用：");
                eprintln!("  li 编译-原生 文件.li --target aarch64-linux-gnu");
                return ExitCode::FAILURE;
            }
        }
    }

    let ir = match compile_to_ir(input) {
        Ok(ir) => ir,
        Err(code) => return code,
    };

    let llvm_ir = match target {
        Some(triple) => li_codegen_llvm::generate_reporting(&ir, Some(triple)),
        None => li_codegen_llvm::generate_reporting(&ir, None),
    };
    let (module, diagnostics) = llvm_ir;

    // LLVM 后端的黄色警告（例如"暂只支持纯标签枚举"）要打印出来，
    // 但按强制警告机制不得中断编译；只有致命错误才拦截。
    if !diagnostics.is_empty() {
        eprintln!("{}", li_errors::render(&diagnostics, true));
    }
    let llvm_ir = match module {
        Some(llvm_ir) => llvm_ir,
        None => return ExitCode::FAILURE,
    };

    let llvm_path = input.with_extension("ll");
    if let Err(error) = fs::write(&llvm_path, llvm_ir) {
        eprintln!(
            "错误：无法写入文件「{}」：{}",
            llvm_path.display(),
            io_error_text(&error)
        );
        return ExitCode::FAILURE;
    }

    let executable = if target.is_some() {
        input.with_extension(target.unwrap_or_default())
    } else {
        input.with_extension("")
    };

    let compiler = "clang";
    let mut command = Command::new(compiler);
    if release {
        // 空间优化：-Os 优先减小体积；按函数/数据分节后由链接器裁剪
        // 未引用内容；-s 剥离符号表与调试信息。
        command
            .arg("-Os")
            .arg("-ffunction-sections")
            .arg("-fdata-sections")
            .arg("-Wl,--gc-sections")
            .arg("-Wl,-s");
    } else {
        command.arg("-O2");
    }
    command.arg(&llvm_path).arg("-o").arg(&executable);

    if let Some(triple) = target {
        command.arg(format!("--target={triple}"));
        for flag in cross_link_flags(triple) {
            command.arg(flag);
        }
        // clang 需要显式把 sysroot 作为 `-B` 指向，避免找不到目标的 crt
        // 与 libc；linker 选择交给 clang 通过 `-B` 中的工具目录。
        if let Some(sysroot) = cross_sysroot_for_target(triple) {
            command.arg(format!("-B{sysroot}bin/"));
        }
    }

    let output = match command.output() {
        Ok(output) => output,
        Err(error) => {
            eprintln!("错误：无法启动 {compiler}：{}", io_error_text(&error));
            return ExitCode::FAILURE;
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        eprintln!("错误：{compiler} 链接失败：\n{stderr}");
        if let Some(triple) = target {
            eprintln!(
                "提示：交叉编译目标「{triple}」可能需要安装对应工具链，例如：
  sudo apt install gcc-aarch64-linux-gnu    # aarch64-linux-gnu
  Android 目标请使用 NDK 的 clang 并设置 ANDROID_NDK_ROOT。"
            );
        }
        return ExitCode::FAILURE;
    }

    let suffix = target
        .map(|triple| format!("，目标 {triple}，静态链接"))
        .unwrap_or_default();
    println!("已生成原生二进制：{}{suffix}", executable.display());
    ExitCode::SUCCESS
}

/// 为交叉编译目标挑选 sysroot（`-B` 指向的目录）；没有匹配目录时返回 None。
fn cross_sysroot_for_target(target: &str) -> Option<&'static str> {
    match target {
        "aarch64-linux-gnu" => Some("/usr/aarch64-linux-gnu/"),
        "arm-linux-gnueabihf" => Some("/usr/arm-linux-gnueabihf/"),
        "i686-linux-gnu" | "i686-unknown-linux" => Some("/usr/i686-linux-gnu/"),
        _ => None,
    }
}

/// 交叉链接的固定参数；`-static` 让产物在手机 Termux 上无需依赖系统 glibc。
///
/// 注意：`-static-libc` 并不是 clang 的合法参数（`-static` 已经包含静态 libc），
/// 显式传入会让 clang 直接报 `unknown argument`。
fn cross_link_flags(_target: &str) -> Vec<String> {
    vec!["-static".to_owned()]
}

/// 检查 Android NDK 工具链是否可用：优先看 `ANDROID_NDK_ROOT` / `ANDROID_NDK_HOME`，
/// 其次尝试直接调用带 API 级别的 NDK clang。
fn android_toolchain_available(architecture: &str) -> bool {
    for variable in ["ANDROID_NDK_ROOT", "ANDROID_NDK_HOME"] {
        if let Ok(root) = std::env::var(variable) {
            if !root.is_empty() && Path::new(&root).is_dir() {
                return true;
            }
        }
    }

    for api in ["21", "24", "26", "34"] {
        let candidate = format!("{architecture}-linux-android{api}-clang");
        if Command::new(&candidate)
            .arg("--version")
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
        {
            return true;
        }
    }

    false
}

/// 解析 `.lui` 视图文件并生成单体 HTML 文本。
fn compile_view_to_html(input: &Path) -> Result<String, ExitCode> {
    if input.extension().and_then(|extension| extension.to_str()) != Some("lui") {
        eprintln!("错误：需要 .lui 视图文件，实际为「{}」", input.display());
        return Err(ExitCode::FAILURE);
    }

    let source = match fs::read_to_string(input) {
        Ok(source) => source,
        Err(error) => {
            eprintln!(
                "错误：无法读取文件「{}」：{}",
                input.display(),
                io_error_text(&error)
            );
            return Err(ExitCode::FAILURE);
        }
    };

    let program = match li_parser::parse_source(&source) {
        Ok(program) => program,
        Err(errors) => {
            eprintln!("{errors}");
            return Err(ExitCode::FAILURE);
        }
    };

    match li_codegen_html::generate(&program) {
        Ok(html) => Ok(html),
        Err(errors) => {
            eprintln!("{errors}");
            Err(ExitCode::FAILURE)
        }
    }
}

/// 编译 `.lui` 视图并在内存中提供单体 HTML 服务。
///
/// 不写入任何 `.html` 文件，也不依赖外部静态资源目录：产物完全内嵌在
/// 进程内，HTTP 响应直接来自内存。
fn serve_view(input: &Path, port: u16) -> ExitCode {
    use std::io::{Read, Write};
    use std::net::TcpListener;

    let html = match compile_view_to_html(input) {
        Ok(html) => html,
        Err(code) => return code,
    };

    let listener = match TcpListener::bind(("127.0.0.1", port)) {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("错误：无法监听端口 {port}：{error}");
            return ExitCode::FAILURE;
        }
    };

    let address = match listener.local_addr() {
        Ok(address) => address,
        Err(error) => {
            eprintln!("错误：无法读取本地地址：{error}");
            return ExitCode::FAILURE;
        }
    };

    println!("服务已启动：http://{address}/");
    println!("按 Ctrl+C 停止服务。");

    for stream in listener.incoming() {
        let Ok(mut stream) = stream else {
            continue;
        };

        let html = html.clone();
        std::thread::spawn(move || {
            let mut buffer = [0_u8; 4096];
            let Ok(read) = stream.read(&mut buffer) else {
                return;
            };
            let request = String::from_utf8_lossy(&buffer[..read]);
            let path = request
                .lines()
                .next()
                .and_then(|line| line.split_whitespace().nth(1))
                .unwrap_or("/");

            let (status, content_type, body) = match path {
                "/" | "/index.html" => ("200 OK", "text/html; charset=utf-8", html),
                "/favicon.ico" => ("204 No Content", "text/plain; charset=utf-8", String::new()),
                _ => (
                    "404 Not Found",
                    "text/plain; charset=utf-8",
                    "未找到请求的资源".to_owned(),
                ),
            };

            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
        });
    }

    ExitCode::SUCCESS
}

/// 解析 `.lui` 视图文件并生成同名的静态 HTML 文件。
fn compile_to_html(input: &Path) -> ExitCode {
    if input.extension().and_then(|extension| extension.to_str()) != Some("lui") {
        eprintln!(
            "错误：命令「编译-html」需要 .lui 视图文件，实际为「{}」",
            input.display()
        );
        return ExitCode::FAILURE;
    }

    let html = match compile_view_to_html(input) {
        Ok(html) => html,
        Err(code) => return code,
    };

    let output = input.with_extension("html");

    if let Err(error) = fs::write(&output, html) {
        eprintln!(
            "错误：无法写入文件「{}」：{}",
            output.display(),
            io_error_text(&error)
        );
        return ExitCode::FAILURE;
    }

    println!("已生成 HTML：{}", output.display());
    ExitCode::SUCCESS
}

fn compile_to_bytecode(input: &Path) -> ExitCode {
    let ir = match compile_to_ir(input) {
        Ok(ir) => ir,
        Err(code) => return code,
    };

    let bytecode = li_codegen_bytecode::generate(&ir);

    if let Some(extern_name) = bytecode
        .functions
        .iter()
        .find_map(|function| function.is_extern.then_some(function.name.clone()))
    {
        eprintln!(
            "错误：外部 函数「{extern_name}」无法在 VM 中绑定动态链接库；\
             请改用 `li 编译-原生` 生成原生二进制"
        );
        return ExitCode::FAILURE;
    }

    if let Err(message) = bytecode.validate() {
        eprintln!("错误：生成的字节码校验失败：{message}");
        return ExitCode::FAILURE;
    }

    let text = li_codegen_bytecode::to_text(&bytecode);
    let output = input.with_extension("lic");

    if let Err(error) = fs::write(&output, text) {
        eprintln!(
            "错误：无法写入文件「{}」：{}",
            output.display(),
            io_error_text(&error)
        );
        return ExitCode::FAILURE;
    }

    println!("已生成字节码：{}", output.display());
    ExitCode::SUCCESS
}

fn execute_bytecode(input: &Path) -> ExitCode {
    match input.extension().and_then(|extension| extension.to_str()) {
        Some("lui") => {
            eprintln!(
                "错误：执行命令不能处理 .lui 视图文件，请使用「li 编译-html {}」",
                input.display()
            );
            return ExitCode::FAILURE;
        }
        Some("li") => {
            eprintln!(
                "错误：执行命令需要 .lic 文件，请先运行「li 编译 {}」",
                input.display()
            );
            return ExitCode::FAILURE;
        }
        _ => {}
    }

    let source = match fs::read_to_string(input) {
        Ok(source) => source,
        Err(error) => {
            eprintln!(
                "错误：无法读取文件「{}」：{}",
                input.display(),
                io_error_text(&error)
            );
            return ExitCode::FAILURE;
        }
    };

    let program = match li_codegen_bytecode::from_text(&source) {
        Ok(program) => program,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };

    let result = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => {
            let program = std::sync::Arc::new(program.clone());
            let output = runtime.block_on(async move { li_vm::execute_async(&program).await });
            match output {
                Ok(()) => ExitCode::SUCCESS,
                Err(error) => {
                    eprintln!("{error}");
                    ExitCode::FAILURE
                }
            }
        }
        Err(error) => {
            eprintln!("错误：无法创建 Tokio 运行时：{error}");
            ExitCode::FAILURE
        }
    };
    result
}

/// 判断程序中是否包含视图声明；递归检查模块内部。
fn contains_view(items: &[li_ast::Item]) -> bool {
    items.iter().any(|item| match item {
        li_ast::Item::View(_) => true,
        li_ast::Item::Module(module) => contains_view(&module.items),
        li_ast::Item::Import(_)
        | li_ast::Item::Function(_)
        | li_ast::Item::Type(_)
        | li_ast::Item::Enum(_) => false,
    })
}

/// 把常见的输入输出错误转换为中文说明。
fn io_error_text(error: &std::io::Error) -> &'static str {
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

/// M15 桌面 GUI：解析 `.lui` 文件并加载到原生窗口。
fn run_desktop(input: &Path) -> ExitCode {
    if input.extension().and_then(|extension| extension.to_str()) != Some("lui") {
        eprintln!(
            "错误：命令「桌面」需要 .lui 视图文件，实际为「{}」",
            input.display()
        );
        return ExitCode::FAILURE;
    }

    match desktop::launch(input) {
        Ok(()) => ExitCode::SUCCESS,
        Err(errors) => {
            eprintln!("{}", li_errors::render(&errors, true));
            ExitCode::FAILURE
        }
    }
}
