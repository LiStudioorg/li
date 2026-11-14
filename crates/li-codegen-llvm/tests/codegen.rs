//! `li-codegen-llvm` 的中立 IR → 文本 LLVM IR 测试。

/// 解析、检查并降级为中立 IR。
fn lower_source(source: &str) -> li_ir::Program {
    let program =
        li_parser::parse_source(source).unwrap_or_else(|errors| panic!("语法分析失败：\n{errors}"));
    li_sema::check(&program).unwrap_or_else(|errors| panic!("语义分析失败：\n{errors}"));
    li_ir::lower(&program).unwrap_or_else(|errors| panic!("IR 降级失败：\n{errors}"))
}

/// 生成文本 LLVM IR。
fn compile(source: &str) -> String {
    let ir = lower_source(source);
    li_codegen_llvm::generate(&ir).unwrap_or_else(|errors| panic!("LLVM 生成失败：\n{errors}"))
}

#[test]
fn generates_llvm_ir_for_fibonacci() {
    let source = include_str!("../../li-parser/tests/fixtures/fibonacci.li");
    let llvm = compile(source);

    assert!(llvm.contains("@\"斐波那契\""), "缺少函数定义：\n{llvm}");
    assert!(llvm.contains("define i32 @main()"), "缺少 C 入口：\n{llvm}");
    assert!(
        llvm.contains("call i64 @\"斐波那契\""),
        "入口应调用 li 主函数：\n{llvm}"
    );
    assert!(llvm.contains("icmp slt"), "缺少小于比较：\n{llvm}");
    assert!(llvm.contains("br i1"), "缺少条件分支：\n{llvm}");
    assert!(llvm.contains("add i64"), "缺少整数加法：\n{llvm}");
    assert!(llvm.contains("@printf"), "缺少打印声明：\n{llvm}");
}

#[test]
fn uses_unique_register_names_across_blocks() {
    let source = "\
函数 分类(值: 整数) -> 整数 {
    如果 值 小于 0 则 {
        返回 -1;
    }
    如果 值 大于 0 则 {
        返回 1;
    }
    返回 0;
}

函数 主() -> 整数 {
    返回 分类(3);
}";
    let llvm = compile(source);

    assert!(llvm.contains("icmp slt"), "缺少小于比较：\n{llvm}");
    assert!(llvm.contains("icmp sgt"), "缺少大于比较：\n{llvm}");

    // 同一函数内不同基本块的临时寄存器必须使用不同前缀。
    for function in llvm.split("define ").skip(1) {
        let mut registers = std::collections::HashSet::new();
        for line in function.lines() {
            let Some((left, _)) = line.split_once(" = ") else {
                continue;
            };
            let name = left.trim();
            if name.starts_with("%b") {
                assert!(
                    registers.insert(name.to_owned()),
                    "寄存器「{name}」在同一函数内重复定义：\n{llvm}"
                );
            }
        }
    }
}

#[test]
fn prints_strings_floats_booleans_and_null() {
    let source = "\
函数 主() -> 整数 {
    打印(「你好，LLVM」);
    打印(1.5);
    打印(真);
    返回 0;
}";
    let llvm = compile(source);

    assert!(llvm.contains("%s\\0A"), "缺少字符串格式：\n{llvm}");
    assert!(llvm.contains("%f\\0A"), "缺少小数格式：\n{llvm}");
    assert!(llvm.contains("select i1"), "缺少布尔选择：\n{llvm}");
    assert!(
        llvm.contains("\\E7\\9C\\9F") && llvm.contains("\\E5\\81\\87"),
        "缺少中文布尔字面量：\n{llvm}"
    );
}

#[test]
fn supports_raw_pointer_equality() {
    let source = include_str!("../../li-parser/tests/fixtures/unsafe_raw_pointer.li");
    let llvm = compile(source);

    assert!(
        llvm.contains("define ptr @\"空指针\""),
        "原生指针应映射为 ptr：\n{llvm}"
    );
    assert!(llvm.contains("icmp eq ptr"), "缺少指针比较：\n{llvm}");
}

#[test]
fn reports_unsupported_runtime_objects_in_chinese() {
    let source = "\
函数 主() -> 整数 {
    让 数据: 列表<整数> = [1, 2];
    打印(数据);
    返回 0;
}";
    let ir = lower_source(source);
    let errors =
        li_codegen_llvm::generate(&ir).expect_err("LLVM 骨架应明确拒绝需要运行时对象的列表");

    let message = errors.to_string();
    assert!(
        message.contains("LLVM 后端暂不支持"),
        "错误信息不是中文诊断：\n{message}"
    );
    assert!(message.contains("第 "), "缺少行号定位：\n{message}");
}
