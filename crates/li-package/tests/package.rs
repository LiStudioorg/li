//! `li-package` 的项目配置、依赖获取与跨包链接测试。

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use li_package::{
    ensure_ready, fetch, prepare_compile, DependencySource, LockedSource, Lockfile, Manifest,
    DEPENDENCY_DIR, LOCK_FILE, MANIFEST_FILE,
};
use li_vm::Vm;

/// 创建唯一的临时目录。
fn temp_directory(prefix: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("系统时间早于 UNIX 纪元")
        .as_nanos();
    let directory = std::env::temp_dir().join(format!("{prefix}-{}-{nanos}", std::process::id()));
    fs::create_dir_all(&directory).expect("无法创建临时目录");
    directory
}

/// 写入文件并自动创建父目录。
fn write(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("无法创建父目录");
    }
    fs::write(path, content).expect("无法写入测试文件");
}

/// 解析入口源码。
fn parse(path: &Path) -> li_ast::Program {
    let source = fs::read_to_string(path).expect("无法读取入口源码");
    li_parser::parse_source(&source).expect("入口源码解析失败")
}

/// 把链接后的程序编译为字节码并执行，返回捕获的输出。
fn compile_and_run(program: &li_ast::Program) -> String {
    li_sema::check(program).expect("语义检查失败");
    let ir = li_ir::lower(program).expect("IR 降级失败");
    let bytecode = li_codegen_bytecode::generate(&ir);
    bytecode.validate().expect("字节码校验失败");

    let mut vm = Vm::with_input(&bytecode, std::iter::empty());
    vm.run().expect("虚拟机执行失败");
    vm.output().to_owned()
}

/// 检测本机是否提供 git。
fn git_available() -> bool {
    Command::new("git")
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

/// 在指定目录执行 git 命令，失败时输出中文断言信息。
fn git(directory: &Path, arguments: &[&str]) {
    let output = Command::new("git")
        .current_dir(directory)
        .args(arguments)
        .output()
        .expect("无法启动 git");
    assert!(
        output.status.success(),
        "git 命令失败：{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// 提交仓库中的全部改动，返回新提交哈希。
fn git_commit(directory: &Path, message: &str) -> String {
    git(directory, &["add", "."]);
    git(
        directory,
        &[
            "-c",
            "user.name=li 测试",
            "-c",
            "user.email=li@example.com",
            "commit",
            "--quiet",
            "-m",
            message,
        ],
    );
    String::from_utf8_lossy(
        &Command::new("git")
            .current_dir(directory)
            .args(["rev-parse", "HEAD"])
            .output()
            .expect("无法读取 git 提交")
            .stdout,
    )
    .trim()
    .to_owned()
}

/// 创建一个带本地 `数学库` 依赖的示例项目。
fn local_math_project() -> (PathBuf, PathBuf) {
    let base = temp_directory("li-package-本地依赖");
    let library = base.join("数学库");
    write(
        &library.join("主.li"),
        "函数 加法(甲: 整数, 乙: 整数) -> 整数 {\n    返回 甲 加 乙;\n}\n",
    );
    write(&library.join("文档/说明.txt"), "这是一个示例数学库。\n");

    let application = base.join("应用");
    write(
        &application.join(MANIFEST_FILE),
        "[项目]\n名称 = \"应用\"\n版本 = \"0.1.0\"\n入口 = \"主.li\"\n\n[依赖]\n数学库 = { 路径 = \"../数学库\" }\n",
    );
    write(
        &application.join("主.li"),
        "导入 数学库;\n\n函数 主() -> 整数 {\n    打印(数学库.加法(1, 2));\n    返回 0;\n}\n",
    );

    (base, application)
}

#[test]
fn parses_manifest_with_chinese_keys() {
    let root = temp_directory("li-package-清单");
    let source = "\
[项目]
名称 = \"网络应用\"
版本 = \"1.2.3\"
入口 = \"src/主.li\"

[依赖]
本地工具 = { 路径 = \"../我的工具\" }
网络库 = { git = \"https://example.com/net.git\", 版本 = \"v1.0\" }
";

    let manifest =
        Manifest::parse(source, &root.join(MANIFEST_FILE), &root).expect("项目配置应当解析成功");

    assert_eq!(manifest.project.name, "网络应用");
    assert_eq!(manifest.project.version.as_deref(), Some("1.2.3"));
    assert_eq!(manifest.project.entry, Some(PathBuf::from("src/主.li")));
    assert_eq!(manifest.dependencies.len(), 2);

    let local = manifest.dependency("本地工具").expect("缺少本地依赖");
    assert_eq!(
        local.source,
        DependencySource::Path {
            path: PathBuf::from("../我的工具")
        }
    );

    let network = manifest.dependency("网络库").expect("缺少 Git 依赖");
    assert_eq!(
        network.source,
        DependencySource::Git {
            url: "https://example.com/net.git".to_owned(),
            version: Some("v1.0".to_owned()),
        }
    );

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn reports_manifest_errors_with_position_and_chinese_text() {
    let root = temp_directory("li-package-清单错误");
    let source = "[依赖]\n网络库 = { 版本 = \"v1.0\" }\n";

    let errors = Manifest::parse(source, &root.join(MANIFEST_FILE), &root)
        .expect_err("缺少来源的依赖应当报错")
        .to_string();

    assert!(errors.contains("第 2 行"), "诊断缺少定位：{errors}");
    assert!(errors.contains("缺少来源"), "诊断缺少说明：{errors}");
    assert!(
        errors.contains("`路径` 或 `git`"),
        "诊断缺少期望值：{errors}"
    );

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn fetches_local_dependency_and_writes_lockfile() {
    let (base, application) = local_math_project();

    let result = fetch(&application).expect("获取本地依赖失败");
    assert_eq!(result.fetched.len(), 1);
    assert_eq!(result.fetched[0].name, "数学库");
    assert_eq!(result.fetched[0].source, "路径 ../数学库");

    let installed = application.join(DEPENDENCY_DIR).join("数学库");
    assert!(installed.join("主.li").is_file(), "未安装依赖入口");
    assert!(
        installed.join("文档/说明.txt").is_file(),
        "未递归复制依赖文件"
    );

    let lock = Lockfile::load(&application)
        .expect("读取锁文件失败")
        .expect("未生成 li.lock");
    let package = lock.packages.get("数学库").expect("锁文件缺少依赖");
    match &package.source {
        LockedSource::Path { path, .. } => assert_eq!(path, "../数学库"),
        other => panic!("依赖来源应为路径，实际为 {other:?}"),
    }

    let manifest = Manifest::load(&application).expect("读取项目配置失败");
    assert!(ensure_ready(&application, &manifest)
        .expect("依赖应当就绪")
        .is_some());
    assert!(application.join(LOCK_FILE).is_file());

    let _ = fs::remove_dir_all(&base);
}

#[test]
fn links_and_runs_local_dependency() {
    let (base, application) = local_math_project();
    fetch(&application).expect("获取本地依赖失败");

    let entry = application.join("主.li");
    let program = parse(&entry);
    let linked = prepare_compile(&entry, &program).expect("跨包链接失败");

    assert_eq!(compile_and_run(&linked), "3\n");

    let _ = fs::remove_dir_all(&base);
}

#[test]
fn links_package_with_module_wrapper() {
    let base = temp_directory("li-package-模块包裹");
    let library = base.join("数学库");
    write(
        &library.join("主.li"),
        "模块 数学库 {\n    函数 加法(甲: 整数, 乙: 整数) -> 整数 {\n        返回 甲 加 乙;\n    }\n}\n",
    );

    let application = base.join("应用");
    write(
        &application.join(MANIFEST_FILE),
        "[项目]\n名称 = \"应用\"\n入口 = \"主.li\"\n\n[依赖]\n数学库 = { 路径 = \"../数学库\" }\n",
    );
    write(
        &application.join("主.li"),
        "导入 数学库;\n\n函数 主() -> 整数 {\n    打印(数学库.加法(2, 3));\n    返回 0;\n}\n",
    );

    fetch(&application).expect("获取本地依赖失败");
    let entry = application.join("主.li");
    let program = parse(&entry);
    let linked = prepare_compile(&entry, &program).expect("跨包链接失败");

    assert_eq!(compile_and_run(&linked), "5\n");

    let _ = fs::remove_dir_all(&base);
}

#[test]
fn requires_lockfile_before_compiling_dependencies() {
    let (base, application) = local_math_project();
    let entry = application.join("主.li");
    let program = parse(&entry);

    let errors = prepare_compile(&entry, &program)
        .expect_err("缺少锁文件时应当报错")
        .to_string();

    assert!(errors.contains("缺少依赖锁文件"), "诊断不正确：{errors}");
    assert!(errors.contains("li 获取"), "诊断缺少修复建议：{errors}");

    let _ = fs::remove_dir_all(&base);
}

#[test]
fn reports_missing_local_module_with_chinese_diagnostic() {
    let directory = temp_directory("li-package-缺少模块");
    let entry = directory.join("主.li");
    write(
        &entry,
        "导入 幽灵;\n\n函数 主() -> 整数 {\n    返回 0;\n}\n",
    );

    let program = parse(&entry);
    let errors = prepare_compile(&entry, &program)
        .expect_err("不存在的模块应当报错")
        .to_string();

    assert!(
        errors.contains("未找到模块「幽灵」"),
        "诊断不正确：{errors}"
    );
    assert!(errors.contains("已查找"), "诊断缺少查找位置：{errors}");

    let _ = fs::remove_dir_all(&directory);
}

#[test]
fn fetches_git_dependency_and_pins_exact_commit() {
    if !git_available() {
        eprintln!("未找到可用的 git，跳过 Git 依赖测试。");
        return;
    }

    let base = temp_directory("li-package-Git依赖");
    let library = base.join("网络库");
    write(
        &library.join("主.li"),
        "函数 加法(甲: 整数, 乙: 整数) -> 整数 {\n    返回 甲 加 乙;\n}\n",
    );

    git(&library, &["init", "--quiet"]);
    let expected = git_commit(&library, "初始提交");
    git(&library, &["tag", "v1.0"]);

    let application = base.join("应用");
    write(
        &application.join(MANIFEST_FILE),
        "[项目]\n名称 = \"应用\"\n入口 = \"主.li\"\n\n[依赖]\n网络库 = { git = \"../网络库\", 版本 = \"v1.0\" }\n",
    );
    write(
        &application.join("主.li"),
        "导入 网络库;\n\n函数 主() -> 整数 {\n    打印(网络库.加法(4, 5));\n    返回 0;\n}\n",
    );

    fetch(&application).expect("获取 Git 依赖失败");

    let lock = Lockfile::load(&application)
        .expect("读取锁文件失败")
        .expect("未生成 li.lock");
    let package = lock.packages.get("网络库").expect("锁文件缺少依赖");
    match &package.source {
        LockedSource::Git {
            commit, version, ..
        } => {
            assert_eq!(commit, &expected, "锁文件未记录精确提交");
            assert_eq!(version.as_deref(), Some("v1.0"));
        }
        other => panic!("依赖来源应为 Git，实际为 {other:?}"),
    }

    let manifest = Manifest::load(&application).expect("读取项目配置失败");
    ensure_ready(&application, &manifest).expect("Git 依赖应当就绪");

    let entry = application.join("主.li");
    let program = parse(&entry);
    let linked = prepare_compile(&entry, &program).expect("跨包链接失败");
    assert_eq!(compile_and_run(&linked), "9\n");

    let _ = fs::remove_dir_all(&base);
}

#[test]
fn reuses_locked_git_commit_on_second_fetch() {
    if !git_available() {
        eprintln!("未找到可用的 git，跳过 Git 锁文件复用测试。");
        return;
    }

    let base = temp_directory("li-package-Git锁定");
    let library = base.join("仓库");
    write(
        &library.join("主.li"),
        "函数 加法(甲: 整数, 乙: 整数) -> 整数 {\n    返回 甲 加 乙;\n}\n",
    );
    git(&library, &["init", "--quiet"]);
    let first = git_commit(&library, "第一版");

    let application = base.join("应用");
    write(
        &application.join(MANIFEST_FILE),
        "[项目]\n名称 = \"应用\"\n入口 = \"主.li\"\n\n[依赖]\n仓库 = { git = \"../仓库\" }\n",
    );
    write(
        &application.join("主.li"),
        "导入 仓库;\n\n函数 主() -> 整数 {\n    打印(仓库.加法(1, 1));\n    返回 0;\n}\n",
    );

    fetch(&application).expect("首次获取 Git 依赖失败");

    // 第一次获取后仓库继续前进，第二次获取必须仍然使用锁文件中的提交。
    write(
        &library.join("主.li"),
        "函数 加法(甲: 整数, 乙: 整数) -> 整数 {\n    返回 甲 加 乙 加 100;\n}\n",
    );
    let second = git_commit(&library, "第二版");
    assert_ne!(first, second, "测试仓库应产生新的提交");

    fetch(&application).expect("再次获取 Git 依赖失败");

    let lock = Lockfile::load(&application)
        .expect("读取锁文件失败")
        .expect("未生成 li.lock");
    let package = lock.packages.get("仓库").expect("锁文件缺少依赖");
    match &package.source {
        LockedSource::Git { commit, .. } => assert_eq!(commit, &first, "锁文件提交被意外更新"),
        other => panic!("依赖来源应为 Git，实际为 {other:?}"),
    }

    let installed = application.join(DEPENDENCY_DIR).join("仓库");
    let head = String::from_utf8_lossy(
        &Command::new("git")
            .arg("-C")
            .arg(&installed)
            .args(["rev-parse", "HEAD"])
            .output()
            .expect("无法读取已安装依赖的提交")
            .stdout,
    )
    .trim()
    .to_owned();
    assert_eq!(head, first, "再次获取没有检出锁定的提交");

    let _ = fs::remove_dir_all(&base);
}

#[test]
fn links_nested_imports_inside_package() {
    let base = temp_directory("li-package-嵌套导入");
    let library = base.join("数学库");
    write(
        &library.join("辅助.li"),
        "函数 加倍(值: 整数) -> 整数 {\n    返回 值 乘 2;\n}\n",
    );
    write(
        &library.join("主.li"),
        "导入 辅助;\n\n函数 加法(甲: 整数, 乙: 整数) -> 整数 {\n    返回 辅助.加倍(甲) 加 乙;\n}\n",
    );

    let application = base.join("应用");
    write(
        &application.join(MANIFEST_FILE),
        "[项目]\n名称 = \"应用\"\n入口 = \"主.li\"\n\n[依赖]\n数学库 = { 路径 = \"../数学库\" }\n",
    );
    write(
        &application.join("主.li"),
        "导入 数学库;\n\n函数 主() -> 整数 {\n    打印(数学库.加法(2, 3));\n    返回 0;\n}\n",
    );

    fetch(&application).expect("获取本地依赖失败");
    let entry = application.join("主.li");
    let program = parse(&entry);
    let linked = prepare_compile(&entry, &program).expect("嵌套导入链接失败");

    assert_eq!(compile_and_run(&linked), "7\n");

    let _ = fs::remove_dir_all(&base);
}
