#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
Node.js 沙箱示例 (Node.js Sandbox Examples)

本脚本演示了如何使用 NodeSandbox 执行 JavaScript 代码。

演示内容：
    1. 基本 Node.js 代码执行
    2. 不同的沙箱管理模式
    3. JavaScript 模块使用
    4. 错误输出处理

运行前准备：
    1. 安装包：pip install -e .
    2. 启动 Microsandbox 服务器 (microsandbox-server)
    3. 运行此脚本：python -m examples.node

注意：
    - 如果服务器启用了认证，需要设置 MSB_API_KEY 环境变量
"""

import asyncio

from microsandbox import NodeSandbox


async def basic_example():
    """
    基本 JavaScript 代码执行示例。

    演示如何使用上下文管理器执行简单的 Node.js 代码。
    """
    print("\n=== 基本 Node.js 示例 ===")

    # 使用上下文管理器创建沙箱（自动处理启动/停止）
    async with NodeSandbox.create(name="node-basic") as sandbox:
        # 运行简单的 JavaScript 代码
        execution = await sandbox.run("console.log('Hello from Node.js!');")
        output = await execution.output()
        print("输出：", output)

        # 运行使用 Node.js 功能的代码
        version_code = """
const version = process.version;
const platform = process.platform;
console.log(`Node.js ${version} running on ${platform}`);
"""
        version_execution = await sandbox.run(version_code)
        print("Node.js 信息：", await version_execution.output())


async def error_handling_example():
    """
    JavaScript 错误处理示例。

    演示如何处理 JavaScript 代码执行过程中的错误。
    """
    print("\n=== 错误处理示例 ===")

    async with NodeSandbox.create(name="node-error") as sandbox:
        # 运行包含错误处理的代码
        caught_error_code = """
try {
    // 这将导致 ReferenceError
    console.log(undefinedVariable);
} catch (error) {
    console.error('Caught error:', error.message);
}
"""
        caught_execution = await sandbox.run(caught_error_code)
        print("标准输出：", await caught_execution.output())
        print("错误输出：", await caught_execution.error())
        print("包含错误：", caught_execution.has_error())


async def module_example():
    """
    Node.js 模块使用示例。

    演示如何使用 Node.js 内置模块（fs 和 os）。
    """
    print("\n=== 模块使用示例 ===")

    async with NodeSandbox.create(name="node-module") as sandbox:
        # 使用 Node.js 内置模块
        fs_code = """
const fs = require('fs');
const os = require('os');

// 写入文件
fs.writeFileSync('/tmp/hello.txt', 'Hello from Node.js!');
console.log('文件写入成功');

// 读取文件
const content = fs.readFileSync('/tmp/hello.txt', 'utf8');
console.log('文件内容：', content);

// 获取系统信息
console.log('主机名：', os.hostname());
console.log('平台：', os.platform());
console.log('架构：', os.arch());
"""
        fs_execution = await sandbox.run(fs_code)
        print(await fs_execution.output())


async def execution_chaining_example():
    """
    执行链示例，演示变量状态保持。

    演示如何在多次执行之间共享变量状态。
    """
    print("\n=== 执行链示例 ===")

    async with NodeSandbox.create(name="node-chain") as sandbox:
        # 执行一系列保持状态的代码块
        await sandbox.run("const name = 'Node.js';")
        await sandbox.run("const version = process.version;")
        await sandbox.run("const numbers = [1, 2, 3, 4, 5];")

        # 使用之前执行中定义的变量
        final_execution = await sandbox.run("""
console.log(`Hello from ${name} ${version}!`);
const sum = numbers.reduce((a, b) => a + b, 0);
console.log(`Sum of numbers: ${sum}`);
""")

        print(await final_execution.output())


async def main():
    """运行所有示例的主函数。"""
    print("Node.js 沙箱示例")
    print("=======================")

    try:
        await basic_example()
        await error_handling_example()
        await module_example()
        await execution_chaining_example()

        print("\n所有 Node.js 示例完成！")
    except Exception as e:
        print(f"运行示例时出错：{e}")


if __name__ == "__main__":
    asyncio.run(main())
