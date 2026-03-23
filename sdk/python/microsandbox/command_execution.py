"""
命令执行结果模块 (Command Execution Results)

本模块提供了表示命令执行结果的类。

主要类：
    CommandExecution: 封装命令执行的输出和状态信息

使用示例：
    result = await cmd.run("ls", ["-la"])
    print(f"退出码：{result.exit_code}")
    print(f"输出：{await result.output()}")
    print(f"是否成功：{result.success}")
"""

from typing import Any, Dict, List, Optional


class CommandExecution:
    """
    命令执行结果类 (Command Execution Result Class)

    此类封装了在沙箱中执行命令后的所有结果信息。

    主要功能：
        - 获取标准输出 (output 方法)
        - 获取错误输出 (error 方法)
        - 获取退出码 (exit_code 属性)
        - 检查是否成功 (success 属性)

    使用示例：
        result = await cmd.run("ls", ["-la"])

        # 获取输出
        output = await result.output()
        print(output)

        # 获取退出码
        print(f"退出码：{result.exit_code}")

        # 检查是否成功
        if result.success:
            print("命令执行成功")
        else:
            print(f"错误：{await result.error()}")
    """

    def __init__(
        self,
        output_data: Optional[Dict[str, Any]] = None,
    ):
        """
        初始化命令执行实例。

        参数：
            output_data (dict, 可选): 来自 sandbox.command.run 响应的输出数据。
                如果提供，将自动解析其中的输出行、退出码等信息。

        实例属性：
            _output_lines (List[dict]): 输出行列表，每行包含 stream 和 text 字段。
            _command (str): 执行的命令名称。
            _args (List[str]): 命令参数列表。
            _exit_code (int): 退出码。
            _success (bool): 是否成功执行。
        """
        self._output_lines: List[Dict[str, str]] = []
        self._command = ""
        self._args: List[str] = []
        self._exit_code = -1
        self._success = False

        # 如果提供了输出数据，进行解析
        if output_data and isinstance(output_data, dict):
            self._process_output_data(output_data)

    def _process_output_data(self, output_data: Dict[str, Any]) -> None:
        """
        处理来自 sandbox.command.run 响应的输出数据。

        此方法解析响应数据并提取以下信息：
        - 输出行列表
        - 命令名称和参数
        - 退出码和成功状态

        参数：
            output_data (dict): 包含输出数据的字典。
                预期包含以下键：
                - "output": 输出行列表
                - "command": 命令名称
                - "args": 参数列表
                - "exit_code": 退出码
                - "success": 是否成功
        """
        # 从响应中提取输出行
        self._output_lines = output_data.get("output", [])

        # 存储命令特定的元数据
        self._command = output_data.get("command", "")
        self._args = output_data.get("args", [])
        self._exit_code = output_data.get("exit_code", -1)
        self._success = output_data.get("success", False)

    async def output(self) -> str:
        """
        获取命令执行的标准输出。

        此方法将所有 stdout 输出行组合成一个字符串。

        返回：
            str: 命令的标准输出内容。
                如果没有任何输出，返回空字符串。

        使用示例：
            result = await cmd.run("echo", ["Hello, World!"])
            output = await result.output()
            print(output)  # 输出：Hello, World!
        """
        # 将 stdout 输出行组合成单个字符串
        output_text = ""
        for line in self._output_lines:
            if isinstance(line, dict) and line.get("stream") == "stdout":
                output_text += line.get("text", "") + "\n"

        # 移除末尾的换行符
        return output_text.rstrip()

    async def error(self) -> str:
        """
        获取命令执行的错误输出。

        此方法将所有 stderr 输出行组合成一个字符串。

        返回：
            str: 命令的标准错误输出内容。
                如果没有任何错误输出，返回空字符串。

        使用示例：
            result = await cmd.run("ls", ["/nonexistent"])
            if not result.success:
                error = await result.error()
                print(f"错误：{error}")
        """
        # 将 stderr 输出行组合成单个字符串
        error_text = ""
        for line in self._output_lines:
            if isinstance(line, dict) and line.get("stream") == "stderr":
                error_text += line.get("text", "") + "\n"

        # 移除末尾的换行符
        return error_text.rstrip()

    @property
    def exit_code(self) -> int:
        """
        获取命令执行的退出码。

        返回：
            int: 命令的退出码。
                - 0: 通常表示成功
                - 非 0: 表示错误，具体含义由命令定义
                - -1: 表示退出码未设置

        使用示例：
            result = await cmd.run("ls", ["/nonexistent"])
            if result.exit_code != 0:
                print("命令执行失败")
        """
        return self._exit_code

    @property
    def success(self) -> bool:
        """
        检查命令是否成功执行。

        返回：
            bool: 如果命令成功执行（退出码为 0）返回 True，否则返回 False。

        使用示例：
            result = await cmd.run("ls", ["/"])
            if result.success:
                print("命令执行成功")
            else:
                print(f"命令执行失败：{await result.error()}")
        """
        return self._success

    @property
    def command(self) -> str:
        """
        获取已执行的命令名称。

        返回：
            str: 执行的命令名称。

        使用示例：
            result = await cmd.run("ls", ["-la"])
            print(f"执行的命令：{result.command}")  # 输出：ls
        """
        return self._command

    @property
    def args(self) -> List[str]:
        """
        获取命令执行的参数列表。

        返回：
            List[str]: 命令参数列表。

        使用示例：
            result = await cmd.run("ls", ["-la", "/"])
            print(f"命令参数：{result.args}")  # 输出：['-la', '/']
        """
        return self._args
