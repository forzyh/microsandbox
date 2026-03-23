"""
Microsandbox Python SDK

Microsandbox Python SDK 是一个用于与 Microsandbox 服务器交互的客户端库。
它提供了在隔离的沙箱环境中执行代码和系统命令的能力。

主要功能：
- Python 代码执行 (PythonSandbox)
- Node.js 代码执行 (NodeSandbox)
- 系统命令执行 (Command)
- 资源监控 (Metrics)

使用示例：
    from microsandbox import PythonSandbox

    async with PythonSandbox.create() as sandbox:
        result = await sandbox.run("print('Hello, World!')")
        print(await result.output())
"""

__version__ = "0.1.0"

# 导入基类和各种沙箱实现
from .base_sandbox import BaseSandbox
from .command import Command
from .command_execution import CommandExecution
from .execution import Execution
from .metrics import Metrics
from .node_sandbox import NodeSandbox
from .python_sandbox import PythonSandbox

# 定义模块的公共 API
# 这些是使用 `from microsandbox import *` 时会导入的内容
__all__ = [
    "PythonSandbox",      # Python 沙箱
    "NodeSandbox",        # Node.js 沙箱
    "BaseSandbox",        # 沙箱基类
    "Execution",          # 代码执行结果
    "CommandExecution",   # 命令执行结果
    "Command",            # 命令执行接口
    "Metrics",            # 资源监控接口
]
