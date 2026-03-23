"""
代码执行结果模块 (Code Execution Results)

本模块提供了表示代码执行结果的类。

主要类：
    Execution: 封装代码执行的输出和状态信息

使用示例：
    sandbox = await PythonSandbox.create()
    result = await sandbox.run("print('Hello, World!')")

    # 获取输出
    output = await result.output()
    print(output)

    # 检查是否有错误
    if result.has_error():
        print(f"执行错误：{await result.error()}")
"""

from typing import Any, Dict, List, Optional


class Execution:
    """
    代码执行结果类 (Execution Result Class)

    此类封装了在沙箱中执行代码后的所有结果信息。

    主要功能：
        - 获取标准输出 (output 方法)
        - 获取错误输出 (error 方法)
        - 检查是否有错误 (has_error 方法)
        - 获取执行状态 (status 属性)
        - 获取执行语言 (language 属性)

    使用示例：
        sandbox = await PythonSandbox.create()
        result = await sandbox.run("print('Hello')")

        # 获取输出
        print(await result.output())

        # 检查状态
        print(f"状态：{result.status}")
        print(f"语言：{result.language}")

        # 错误检查
        if result.has_error():
            print(f"错误：{await result.error()}")
    """

    def __init__(
        self,
        output_data: Optional[Dict[str, Any]] = None,
    ):
        """
        初始化代码执行实例。

        参数：
            output_data (dict, 可选): 来自 sandbox.repl.run 响应的输出数据。
                如果提供，将自动解析其中的输出行、状态等信息。

        实例属性：
            _output_lines (List[dict]): 输出行列表，每行包含 stream 和 text 字段。
            _status (str): 执行状态（如 "success"、"error"）。
            _language (str): 执行使用的编程语言（如 "python"、"nodejs"）。
            _has_error (bool): 是否包含错误。
        """
        self._output_lines: List[Dict[str, str]] = []
        self._status = "unknown"
        self._language = "unknown"
        self._has_error = False

        # 如果提供了输出数据，进行解析
        if output_data and isinstance(output_data, dict):
            self._process_output_data(output_data)

    def _process_output_data(self, output_data: Dict[str, Any]) -> None:
        """
        处理来自 sandbox.repl.run 响应的输出数据。

        此方法解析响应数据并提取以下信息：
        - 输出行列表
        - 执行状态
        - 执行语言
        - 错误标志

        参数：
            output_data (dict): 包含输出数据的字典。
                预期包含以下键：
                - "output": 输出行列表
                - "status": 执行状态
                - "language": 执行语言
        """
        # 从响应中提取输出行
        self._output_lines = output_data.get("output", [])

        # 存储元数据
        self._status = output_data.get("status", "unknown")
        self._language = output_data.get("language", "unknown")

        # 检查是否有错误
        # 首先检查状态字段
        if self._status == "error" or self._status == "exception":
            self._has_error = True
        else:
            # 检查是否有 stderr 输出
            for line in self._output_lines:
                if (
                    isinstance(line, dict)
                    and line.get("stream") == "stderr"
                    and line.get("text")
                ):
                    self._has_error = True
                    break

    async def output(self) -> str:
        """
        获取代码执行的标准输出。

        此方法将所有 stdout 输出行组合成一个字符串。

        返回：
            str: 代码执行的标准输出内容。
                如果没有任何输出，返回空字符串。

        使用示例：
            result = await sandbox.run("print('Hello, World!')")
            print(await result.output())  # 输出：Hello, World!
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
        获取代码执行的错误输出。

        此方法将所有 stderr 输出行组合成一个字符串。

        返回：
            str: 代码执行的标准错误输出内容。
                如果没有任何错误输出，返回空字符串。

        使用示例：
            result = await sandbox.run("1/0")  # 除零错误
            if result.has_error():
                print(f"错误：{await result.error()}")
        """
        # 将 stderr 输出行组合成单个字符串
        error_text = ""
        for line in self._output_lines:
            if isinstance(line, dict) and line.get("stream") == "stderr":
                error_text += line.get("text", "") + "\n"

        # 移除末尾的换行符
        return error_text.rstrip()

    def has_error(self) -> bool:
        """
        检查代码执行是否包含错误。

        返回：
            bool: 如果执行过程中遇到错误返回 True，否则返回 False。

        使用示例：
            result = await sandbox.run("1/0")
            if result.has_error():
                print("执行出错")
            else:
                print("执行成功")
        """
        return self._has_error

    @property
    def status(self) -> str:
        """
        获取代码执行的状态。

        返回：
            str: 执行状态。
                常见值：
                - "success": 执行成功
                - "error": 执行出错
                - "exception": 发生异常
                - "unknown": 未知状态

        使用示例：
            result = await sandbox.run("print('hello')")
            print(f"执行状态：{result.status}")
        """
        return self._status

    @property
    def language(self) -> str:
        """
        获取代码执行使用的语言。

        返回：
            str: 执行语言。
                常见值：
                - "python": Python 语言
                - "nodejs": JavaScript/Node.js
                - "unknown": 未知语言

        使用示例：
            result = await sandbox.run("print('hello')")
            print(f"执行语言：{result.language}")  # 输出：python
        """
        return self._language
