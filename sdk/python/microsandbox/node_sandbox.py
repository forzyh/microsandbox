"""
Node.js 沙箱模块 (Node.js Sandbox)

本模块提供了 Node.js 专用的沙箱实现。

主要类：
    NodeSandbox: 用于执行 JavaScript 代码的沙箱类

使用示例：
    from microsandbox import NodeSandbox

    # 使用上下文管理器（推荐）
    async with NodeSandbox.create() as sandbox:
        result = await sandbox.run("console.log('Hello, World!')")
        print(await result.output())

    # 或者手动管理生命周期
    sandbox = NodeSandbox()
    await sandbox.start()
    result = await sandbox.run("console.log('Hello')")
    await sandbox.stop()
"""

import uuid

import aiohttp

from .base_sandbox import BaseSandbox
from .execution import Execution


class NodeSandbox(BaseSandbox):
    """
    Node.js 专用沙箱类 (Node.js Sandbox Class)

    此类继承自 BaseSandbox，提供了在隔离环境中执行 JavaScript 代码的能力。

    主要功能：
        - 执行 JavaScript 代码 (run 方法)
        - 执行系统命令 (command 属性)
        - 获取资源指标 (metrics 属性)

    默认镜像：
        microsandbox/node

    使用示例：
        # 基本使用
        async with NodeSandbox.create() as sandbox:
            # 执行 JavaScript 代码
            result = await sandbox.run("console.log('Hello, World!')")
            print(await result.output())

            # 使用 Node.js API
            result = await sandbox.run("console.log(process.version)")
            print(f"Node.js 版本：{await result.output()}")

            # 执行系统命令
            cmd_result = await sandbox.command.run("ls", ["-la"])
            print(await cmd_result.output())

        # 手动管理生命周期
        sandbox = NodeSandbox()
        await sandbox.start(memory=1024, cpus=2.0)
        result = await sandbox.run("console.log('Hello from Node.js')")
        await sandbox.stop()

    注意事项：
        - 代码在 Node.js REPL 环境中执行
        - 支持状态保持（多次执行间共享变量）
        - 可以使用所有 Node.js 内置模块
    """

    async def get_default_image(self) -> str:
        """
        获取 Node.js 沙箱默认的 Docker 镜像。

        返回：
            str: Docker 镜像名称 "microsandbox/node"

        注意事项：
            - 此方法被 BaseSandbox.start() 调用
            - 如果未指定镜像参数，将使用此默认镜像
        """
        return "microsandbox/node"

    async def run(self, code: str) -> Execution:
        """
        在沙箱中执行 JavaScript 代码。

        此方法通过 JSON-RPC 向 Microsandbox 服务器发送代码执行请求。
        代码在 Node.js REPL 环境中执行，支持状态保持（多次执行间共享变量）。

        参数：
            code (str): 要执行的 JavaScript 代码。
                可以是单行或多行代码。

        返回：
            Execution: 包含代码执行结果的对象。
                - output: 标准输出
                - error: 标准错误输出
                - has_error: 是否包含错误
                - status: 执行状态
                - language: 执行语言（"nodejs"）

        异常：
            RuntimeError: 沙箱未启动或执行失败。

        使用示例：
            # 简单代码执行
            result = await sandbox.run("console.log('Hello, World!')")
            print(await result.output())

            # 多行代码
            code = '''
            const x = 10;
            const y = 20;
            console.log(`Sum: ${x + y}`);
            '''
            result = await sandbox.run(code)

            # 变量保持（多次执行间共享）
            await sandbox.run("const name = 'Node.js';")
            result = await sandbox.run("console.log(`Hello from ${name}`)")
            # 输出：Hello from Node.js

            # 使用 Node.js 内置模块
            result = await sandbox.run("console.log(require('os').platform())")

            # 错误处理
            result = await sandbox.run("throw new Error('Test error')")
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
                "language": "nodejs",
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
