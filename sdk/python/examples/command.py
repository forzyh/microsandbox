#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
命令执行示例 (Command Execution Examples)

本脚本演示了如何使用 sandbox.command.run 执行 shell 命令。

演示内容：
    1. 基本命令执行
    2. 错误处理
    3. 命令超时控制
    4. 高级命令用法（文件操作、管道等）
    5. 显式生命周期管理

运行前准备：
    1. 安装 microsandbox 包
    2. 启动 Microsandbox 服务器 (microsandbox-server)
    3. 运行此脚本：python examples/command.py

注意：
    - 如果服务器启用了认证，需要设置 MSB_API_KEY 环境变量
"""

import asyncio

import aiohttp
from microsandbox import PythonSandbox


async def basic_example():
    """
    基本命令执行示例。

    演示如何使用上下文管理器执行简单的 shell 命令。
    上下文管理器自动处理沙箱的启动和停止。
    """
    print("\n=== 基本命令示例 ===")

    # 使用上下文管理器创建沙箱（自动处理启动/停止）
    async with PythonSandbox.create(name="command-example") as sandbox:
        # 运行简单命令
        ls_execution = await sandbox.command.run("ls", ["-la", "/"])
        print("$ ls -la /")
        print(f"退出码：{ls_execution.exit_code}")
        print("输出：")
        print(await ls_execution.output())

        # 执行带字符串参数的命令
        echo_execution = await sandbox.command.run(
            "echo", ["Hello from", "sandbox command!"]
        )
        print("\n$ echo Hello from sandbox command!")
        print(f"输出：{await echo_execution.output()}")

        # 获取系统信息
        uname_execution = await sandbox.command.run("uname", ["-a"])
        print("\n$ uname -a")
        print(f"输出：{await uname_execution.output()}")


async def error_handling_example():
    """
    错误处理示例。

    演示如何处理命令执行过程中的错误：
    1. 访问不存在的路径
    2. 执行不存在的命令
    """
    print("\n=== 错误处理示例 ===")

    async with PythonSandbox.create(name="error-example") as sandbox:
        # 运行会产生错误的命令
        error_execution = await sandbox.command.run("ls", ["/nonexistent"])

        print("$ ls /nonexistent")
        print(f"退出码：{error_execution.exit_code}")
        print(f"成功：{error_execution.success}")
        print("错误输出：")
        print(await error_execution.error())

        # 故意执行不存在的命令
        try:
            _nonexistent_cmd = await sandbox.command.run("nonexistentcommand", [])
            # 如果命令失败，这里不应该执行
            print("命令意外成功")
        except RuntimeError as e:
            print(f"\n捕获到不存在的命令异常：{e}")


async def timeout_example():
    """
    命令超时示例。

    演示如何设置命令超时时间，防止命令执行过长。
    """
    print("\n=== 超时示例 ===")

    async with PythonSandbox.create(name="timeout-example") as sandbox:
        print("运行带超时的命令...")
        try:
            # 运行一个执行时间超过指定超时的命令
            await sandbox.command.run("sleep", ["10"], timeout=2)
            print("命令完成（意外！）")
        except RuntimeError as e:
            print(f"命令按预期超时：{e}")

        # 显示超时后沙箱仍可使用
        echo_execution = await sandbox.command.run("echo", ["Still working!"])
        print(f"\n沙箱仍可工作：{await echo_execution.output()}")


async def advanced_example():
    """
    高级命令用法示例。

    演示更复杂的命令操作：
    1. 文件写入和读取
    2. 复杂管道命令
    3. 创建并执行 Python 脚本
    """
    print("\n=== 高级示例 ===")

    async with PythonSandbox.create(name="advanced-example") as sandbox:
        # 写入文件
        write_cmd = await sandbox.command.run(
            "bash", ["-c", "echo 'Hello, file content!' > /tmp/test.txt"]
        )
        print(f"创建文件，退出码：{write_cmd.exit_code}")

        # 读取文件
        read_cmd = await sandbox.command.run("cat", ["/tmp/test.txt"])
        print(f"文件内容：{await read_cmd.output()}")

        # 运行更复杂的管道命令
        pipeline_cmd = await sandbox.command.run(
            "bash",
            [
                "-c",
                "mkdir -p /tmp/test_dir && "
                "echo 'Line 1' > /tmp/test_dir/data.txt && "
                "echo 'Line 2' >> /tmp/test_dir/data.txt && "
                "cat /tmp/test_dir/data.txt | grep 'Line' | wc -l",
            ],
        )
        print(f"\n管道输出（应该是 2）：{await pipeline_cmd.output()}")

        # 创建并运行 Python 脚本
        create_script = await sandbox.command.run(
            "bash",
            [
                "-c",
                """cat > /tmp/test.py << 'EOF'
import sys
print("Python script executed!")
print(f"Arguments: {sys.argv[1:]}")
EOF""",
            ],
        )

        if create_script.success:
            # 带参数运行脚本
            script_cmd = await sandbox.command.run(
                "python", ["/tmp/test.py", "arg1", "arg2", "arg3"]
            )
            print("\nPython 脚本输出：")
            print(await script_cmd.output())


async def explicit_lifecycle_example():
    """
    显式生命周期管理示例。

    演示如何不使用上下文管理器，而是手动管理沙箱的启动和停止。
    这种方式提供更多控制权，但需要手动清理资源。
    """
    print("\n=== 显式生命周期示例 ===")

    # 不使用上下文管理器创建沙箱
    sandbox = PythonSandbox(name="explicit-lifecycle")
    sandbox._session = aiohttp.ClientSession()

    try:
        # 手动启动沙箱
        print("启动沙箱...")
        await sandbox.start()

        # 执行命令
        hostname_cmd = await sandbox.command.run("hostname")
        print(f"主机名：{await hostname_cmd.output()}")

        date_cmd = await sandbox.command.run("date")
        print(f"日期：{await date_cmd.output()}")

    finally:
        # 手动停止沙箱并关闭会话
        # 这部分在 finally 块中，确保总是执行
        print("停止沙箱...")
        await sandbox.stop()
        await sandbox._session.close()


async def main():
    """主函数，运行所有示例。"""
    print("命令执行示例")
    print("=========================")

    await basic_example()
    await error_handling_example()
    await timeout_example()
    await advanced_example()
    await explicit_lifecycle_example()

    print("\n所有示例完成！")


if __name__ == "__main__":
    asyncio.run(main())
