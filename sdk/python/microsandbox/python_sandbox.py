"""
Python 沙箱模块 (Python Sandbox)

本模块提供了 Python 专用的沙箱实现。

主要类：
    PythonSandbox: 用于执行 Python 代码的沙箱类

使用示例：
    from microsandbox import PythonSandbox

    # 使用上下文管理器（推荐）
    async with PythonSandbox.create() as sandbox:
        result = await sandbox.run("print('Hello, World!')")
        print(await result.output())

    # 或者手动管理生命周期
    sandbox = PythonSandbox()
    await sandbox.start()
    result = await sandbox.run("print('Hello')")
    await sandbox.stop()
"""

import uuid

import aiohttp

from .base_sandbox import BaseSandbox
from .execution import Execution


class PythonSandbox(BaseSandbox):
    """
    Python 专用沙箱类 (Python Sandbox Class)

    此类继承自 BaseSandbox，提供了在隔离环境中执行 Python 代码的能力。

    主要功能：
        - 执行 Python 代码 (run 方法)
        - 执行系统命令 (command 属性)
        - 获取资源指标 (metrics 属性)

    默认镜像：
        microsandbox/python

    使用示例：
        # 基本使用
        async with PythonSandbox.create() as sandbox:
            # 执行 Python 代码
            result = await sandbox.run("print('Hello, World!')")
            print(await result.output())

            # 执行系统命令
            cmd_result = await sandbox.command.run("ls", ["-la"])
            print(await cmd_result.output())

            # 获取资源指标
            cpu = await sandbox.metrics.cpu()
            memory = await sandbox.metrics.memory()
            print(f"CPU: {cpu}%, Memory: {memory} MiB")

        # 手动管理生命周期
        sandbox = PythonSandbox()
        await sandbox.start(memory=1024, cpus=2.0)
        result = await sandbox.run("import sys; print(sys.version)")
        await sandbox.stop()
    """

    async def get_default_image(self) -> str:
        """
        获取 Python 沙箱默认的 Docker 镜像。

        返回：
            str: Docker 镜像名称 "microsandbox/python"

        注意事项：
            - 此方法被 BaseSandbox.start() 调用
            - 如果未指定镜像参数，将使用此默认镜像
        """
        return "microsandbox/python"

    async def run(self, code: str) -> Execution:
        """
        在沙箱中执行 Python 代码。

        此方法通过 JSON-RPC 向 Microsandbox 服务器发送代码执行请求。
        代码在 Python REPL 环境中执行，支持状态保持（多次执行间共享变量）。

        参数：
            code (str): 要执行的 Python 代码。
                可以是单行或多行代码。

        返回：
            Execution: 包含代码执行结果的对象。
                - output: 标准输出
                - error: 标准错误输出
                - has_error: 是否包含错误
                - status: 执行状态
                - language: 执行语言（"python"）

        异常：
            RuntimeError: 沙箱未启动或执行失败。

        使用示例：
            # 简单代码执行
            result = await sandbox.run("print('Hello, World!')")
            print(await result.output())

            # 多行代码
            code = '''
            x = 10
            y = 20
            print(f"Sum: {x + y}")
            '''
            result = await sandbox.run(code)

            # 变量保持（多次执行间共享）
            await sandbox.run("x = 42")
            result = await sandbox.run("print(x)")  # 输出：42

            # 错误处理
            result = await sandbox.run("1 / 0")
            if result.has_error():
                print(f"错误：{await result.error()}")
        """
        # 检查沙箱是否已启动
        if not self._is_started:
            raise RuntimeError("Sandbox is not started. Call start() first.")

        # 设置请求头
        headers = {"Content-Type": "application/json"}
        if self._api_key:
            headers["Authorization"] = f"Bearer {self._api_key}"

        # 构建 JSON-RPC 请求数据
        request_data = {
            "jsonrpc": "2.0",
            "method": "sandbox.repl.run",
            "params": {
                "sandbox": self._name,
                "language": "python",
                "code": code,
            },
            "id": str(uuid.uuid4()),
        }

        try:
            # 发送 HTTP POST 请求到服务器
            async with self._session.post(
                f"{self._server_url}/api/v1/rpc",
                json=request_data,
                headers=headers,
            ) as response:
                # 检查 HTTP 状态码
                if response.status != 200:
                    error_text = await response.text()
                    raise RuntimeError(f"Failed to execute code: {error_text}")

                # 解析 JSON 响应
                response_data = await response.json()

                # 检查是否有错误
                if "error" in response_data:
                    raise RuntimeError(
                        f"Failed to execute code: {response_data['error']['message']}"
                    )

                # 获取结果数据
                result = response_data.get("result", {})

                # 创建并返回 Execution 对象
                return Execution(output_data=result)
        except aiohttp.ClientError as e:
            # 网络错误
            raise RuntimeError(f"Failed to execute code: {e}")
