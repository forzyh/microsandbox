#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
Python 沙箱高级示例 (Python Sandbox Advanced Examples)

本脚本演示了 Python 沙箱的各种高级功能。

演示内容：
    1. 使用异步上下文管理器创建和管理沙箱
    2. 资源配置（内存、CPU）
    3. 错误处理
    4. 多种代码执行模式
    5. 输出处理
    6. 超时和长时间启动的处理

运行前准备：
    1. 安装包：pip install -e .
    2. 启动 Microsandbox 服务器 (microsandbox-server)
    3. 运行此脚本：python -m examples.repl

注意：
    - 如果服务器启用了认证，需要设置 MSB_API_KEY 环境变量
"""

import asyncio

import aiohttp
from microsandbox import PythonSandbox


async def example_context_manager():
    """
    使用异步上下文管理器的示例。

    演示如何使用 async with 语法自动管理沙箱的生命周期。
    这是推荐的沙箱使用方式，因为它会自动处理启动、停止和资源清理。
    """
    print("\n=== 上下文管理器示例 ===")

    async with PythonSandbox.create(name="sandbox-cm") as sandbox:
        # 运行一些计算
        code = """
print("Hello, world!")
"""
        execution = await sandbox.run(code)
        output = await execution.output()
        print("输出：", output)


async def example_explicit_lifecycle():
    """
    使用显式生命周期管理的示例。

    演示如何手动控制沙箱的启动和停止。
    这种方式提供更多的控制权，但需要手动清理资源。
    """
    print("\n=== 显式生命周期示例 ===")

    # 使用自定义配置创建沙箱
    sandbox = PythonSandbox(
        server_url="http://127.0.0.1:5555", name="sandbox-explicit"
    )

    # 创建 HTTP 会话
    sandbox._session = aiohttp.ClientSession()

    try:
        # 使用资源限制启动沙箱
        await sandbox.start(
            memory=1024,  # 1GB 内存
            cpus=2.0,  # 2 核 CPU
        )

        # 运行多个带有变量赋值的代码块
        await sandbox.run("x = 42")
        await sandbox.run("y = [i**2 for i in range(10)]")
        execution3 = await sandbox.run("print(f'x = {x}')\nprint(f'y = {y}')")

        print("输出：", await execution3.output())

        # 演示错误处理
        try:
            # 这将导致 ZeroDivisionError
            error_execution = await sandbox.run("1/0")
            print("错误：", await error_execution.error())
        except RuntimeError as e:
            print(f"捕获错误：{e}")

    finally:
        # 清理资源
        # finally 块确保即使发生错误也会执行清理
        await sandbox.stop()
        await sandbox._session.close()


async def example_execution_chaining():
    """
    执行链示例，演示变量状态保持。

    演示如何在多次执行之间共享变量状态。
    REPL 环境的特点就是保持状态，后续执行可以使用之前定义的变量。
    """
    print("\n=== 执行链示例 ===")

    async with PythonSandbox.create(name="sandbox-chain") as sandbox:
        # 执行一系列相关的代码块
        await sandbox.run("name = 'Python'")
        await sandbox.run("import sys")
        await sandbox.run("version = sys.version")
        exec = await sandbox.run("print(f'Hello from {name} {version}!')")

        # 只获取最后一次执行的输出
        print("输出：", await exec.output())


async def main():
    """运行所有示例的主函数。"""
    try:
        await example_context_manager()
        await example_explicit_lifecycle()
        await example_execution_chaining()
    except Exception as e:
        print(f"运行示例时出错：{e}")


if __name__ == "__main__":
    asyncio.run(main())
