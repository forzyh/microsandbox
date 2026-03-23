"""
命令执行模块 (Command Execution)

本模块提供了在沙箱环境中执行系统命令的接口。

主要类：
    Command: 用于在沙箱中执行 shell 命令的类

使用示例：
    from microsandbox import PythonSandbox

    async with PythonSandbox.create() as sandbox:
        cmd = sandbox.command
        result = await cmd.run("ls", ["-la", "/"])
        print(f"退出码：{result.exit_code}")
        print(f"输出：{await result.output()}")
"""

import uuid
from typing import List, Optional

import aiohttp

from .command_execution import CommandExecution


class Command:
    """
    命令类 (Command Class)

    用于在沙箱环境中执行 shell 命令。

    此类通过 BaseSandbox 的 command 属性访问，不直接实例化。

    主要功能：
        - 执行系统命令 (run 方法)
        - 支持命令参数
        - 支持超时控制

    使用示例：
        sandbox = await PythonSandbox.create()
        cmd = sandbox.command

        # 执行简单命令
        result = await cmd.run("ls", ["-la"])

        # 执行带超时的命令
        result = await cmd.run("sleep", ["10"], timeout=5)
    """

    def __init__(self, sandbox_instance):
        """
        初始化命令实例。

        参数：
            sandbox_instance: 此命令所属的沙箱实例。
                通过沙箱实例访问服务器连接和配置。

        注意事项：
            此构造函数通常不直接调用，而是通过 BaseSandbox.command 属性访问。
        """
        self._sandbox = sandbox_instance

    async def run(
        self,
        command: str,
        args: Optional[List[str]] = None,
        timeout: Optional[int] = None,
    ) -> CommandExecution:
        """
        在沙箱中执行 shell 命令。

        此方法通过 JSON-RPC 向 Microsandbox 服务器发送命令执行请求。

        参数说明：
            command (str): 要执行的命令。
                例如："ls", "echo", "python" 等。
            args (List[str], 可选): 命令参数列表。
                每个参数是独立的字符串。
                例如：["-la", "/"] 或 ["Hello", "World"]
                默认为空列表。
            timeout (int, 可选): 超时时间（秒）。
                如果命令执行时间超过此值，将被终止。
                None 表示不设置超时。

        返回：
            CommandExecution: 包含命令执行结果的对象。
                - exit_code: 退出码（0 表示成功）
                - output: 标准输出
                - error: 标准错误输出
                - success: 是否成功执行

        异常：
            RuntimeError: 沙箱未启动或执行失败。

        使用示例：
            # 基本命令执行
            result = await cmd.run("ls", ["-la", "/"])
            print(f"退出码：{result.exit_code}")
            print(f"输出：{await result.output()}")

            # 带超时的命令
            try:
                result = await cmd.run("sleep", ["10"], timeout=2)
            except RuntimeError as e:
                print(f"命令超时：{e}")

            # 错误处理
            result = await cmd.run("ls", ["/nonexistent"])
            if not result.success:
                print(f"错误：{await result.error()}")
        """
        # 检查沙箱是否已启动
        if not self._sandbox._is_started:
            raise RuntimeError("Sandbox is not started. Call start() first.")

        # 初始化参数列表
        if args is None:
            args = []

        # 设置请求头
        headers = {"Content-Type": "application/json"}
        if self._sandbox._api_key:
            headers["Authorization"] = f"Bearer {self._sandbox._api_key}"

        # 构建 JSON-RPC 请求数据
        request_data = {
            "jsonrpc": "2.0",
            "method": "sandbox.command.run",
            "params": {
                "sandbox": self._sandbox._name,
                "command": command,
                "args": args,
            },
            "id": str(uuid.uuid4()),
        }

        # 如果指定了超时时间，添加到请求参数中
        if timeout is not None:
            request_data["params"]["timeout"] = timeout

        try:
            # 发送 HTTP POST 请求到服务器
            async with self._sandbox._session.post(
                f"{self._sandbox._server_url}/api/v1/rpc",
                json=request_data,
                headers=headers,
            ) as response:
                # 检查 HTTP 状态码
                if response.status != 200:
                    error_text = await response.text()
                    raise RuntimeError(f"Failed to execute command: {error_text}")

                # 解析 JSON 响应
                response_data = await response.json()

                # 检查是否有错误
                if "error" in response_data:
                    raise RuntimeError(
                        f"Failed to execute command: {response_data['error']['message']}"
                    )

                # 获取结果数据
                result = response_data.get("result", {})

                # 创建并返回 CommandExecution 对象
                return CommandExecution(output_data=result)
        except aiohttp.ClientError as e:
            # 网络错误
            raise RuntimeError(f"Failed to execute command: {e}")
