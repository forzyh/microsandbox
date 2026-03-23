"""
基础沙箱模块 (Base Sandbox)

本模块提供了 Microsandbox Python SDK 的基础沙箱实现。
它处理与 Microsandbox 服务器的通信，包括沙箱创建、管理和通信等公共功能。

主要类：
    BaseSandbox: 抽象基类，提供沙箱环境的基础接口

使用方式：
    此类通常不直接使用，而是通过其子类 PythonSandbox 或 NodeSandbox 来访问。
"""

import asyncio
import os
import uuid
from abc import ABC, abstractmethod
from contextlib import asynccontextmanager
from typing import Dict, List, Optional

import aiohttp
from dotenv import load_dotenv

from .command import Command
from .metrics import Metrics


class BaseSandbox(ABC):
    """
    基础沙箱环境类 (Base Sandbox Class)

    此类提供了与 Microsandbox 服务器交互的基础接口。
    它处理沙箱创建、启动、停止和通信等核心功能。

    这是一个抽象基类，具体的语言实现需要继承此类并实现抽象方法。

    主要功能：
        - 沙箱生命周期管理 (start/stop)
        - HTTP 会话管理
        - 命令执行接口
        - 资源监控接口

    使用示例：
        # 通过子类创建沙箱
        async with PythonSandbox.create() as sandbox:
            # 执行代码
            result = await sandbox.run("print('hello')")
    """

    def __init__(
        self,
        server_url: str = None,
        name: Optional[str] = None,
        api_key: Optional[str] = None,
    ):
        """
        初始化基础沙箱实例。

        参数说明：
            server_url (str, 可选): Microsandbox 服务器的 URL。
                - 如果未提供，先检查 MSB_SERVER_URL 环境变量
                - 如果环境变量也不存在，使用默认值 "http://127.0.0.1:5555"
            name (str, 可选): 沙箱名称。
                - 如果未提供，将生成一个随机的唯一名称
            api_key (str, 可选): API 密钥，用于服务器认证。
                - 如果未提供，将从 MSB_API_KEY 环境变量读取

        实例属性：
            _server_url: 服务器 URL
            _name: 沙箱名称
            _api_key: API 密钥
            _session: HTTP 会话对象
            _is_started: 沙箱是否已启动的标志
        """
        # 仅在 MSB_API_KEY 未设置时尝试加载 .env 文件
        if "MSB_API_KEY" not in os.environ:
            # 忽略 .env 文件不存在的错误
            try:
                load_dotenv()
            except Exception:
                pass

        self._server_url = server_url or os.environ.get(
            "MSB_SERVER_URL", "http://127.0.0.1:5555"
        )
        # 使用 UUID 的前 8 个字符生成随机名称
        self._name = name or f"sandbox-{uuid.uuid4().hex[:8]}"
        self._api_key = api_key or os.environ.get("MSB_API_KEY")
        self._session = None
        self._is_started = False

    @abstractmethod
    async def get_default_image(self) -> str:
        """
        获取此沙箱类型默认的 Docker 镜像。

        这是一个抽象方法，必须由子类实现。

        返回：
            str: Docker 镜像名称和标签，例如 "microsandbox/python"

        实现示例：
            PythonSandbox 返回 "microsandbox/python"
            NodeSandbox 返回 "microsandbox/node"
        """
        pass

    @classmethod
    @asynccontextmanager
    async def create(
        cls,
        server_url: str = None,
        name: Optional[str] = None,
        api_key: Optional[str] = None,
    ):
        """
        创建并初始化一个新的沙箱，作为异步上下文管理器。

        这是一个便捷的工厂方法，使用 async with 语法自动管理沙箱的生命周期。

        参数说明：
            server_url (str, 可选): Microsandbox 服务器的 URL。
            name (str, 可选): 沙箱名称。
            api_key (str, 可选): API 密钥用于认证。

        返回：
            一个初始化完成的沙箱实例

        使用示例：
            async with PythonSandbox.create() as sandbox:
                # 沙箱已自动启动
                result = await sandbox.run("print('hello')")
                print(await result.output())
            # 退出上下文后，沙箱自动停止并关闭连接

        注意事项：
            - 沙箱在退出上下文时会自动停止
            - HTTP 会话会自动关闭
        """
        # 仅在 MSB_API_KEY 未设置时尝试加载 .env 文件
        if "MSB_API_KEY" not in os.environ:
            # 忽略 .env 文件不存在的错误
            try:
                load_dotenv()
            except Exception:
                pass

        sandbox = cls(
            server_url=server_url,
            name=name,
            api_key=api_key,
        )
        try:
            # 创建 HTTP 会话
            sandbox._session = aiohttp.ClientSession()
            # 启动沙箱
            await sandbox.start()
            yield sandbox
        finally:
            # 停止沙箱
            await sandbox.stop()
            # 关闭 HTTP 会话
            if sandbox._session:
                await sandbox._session.close()
                sandbox._session = None

    async def start(
        self,
        image: Optional[str] = None,
        memory: int = 512,
        cpus: float = 1.0,
        volumes: Optional[List[str]] = None,
        ports: Optional[List[str]] = None,
        envs: Optional[List[str]] = None,
        depends_on: Optional[List[str]] = None,
        workdir: Optional[str] = None,
        shell: Optional[str] = None,
        scripts: Optional[Dict[str, str]] = None,
        exec: Optional[str] = None,
        timeout: float = 180.0,
    ) -> None:
        """
        启动沙箱容器。

        此方法向 Microsandbox 服务器发送请求，创建并启动一个新的沙箱容器。

        参数说明：
            image (str, 可选): Docker 镜像名称。
                - 如果未指定，使用语言特定的默认镜像
                - 例如："microsandbox/python:latest"
            memory (int): 内存限制，单位 MB。默认 512MB。
            cpus (float): CPU 限制。默认 1.0 核。
                - 会被四舍五入到最接近的整数
            volumes (List[str], 可选): 要挂载的卷列表。
            ports (List[str], 可选): 要暴露的端口列表。
            envs (List[str], 可选): 环境变量列表。
            depends_on (List[str], 可选): 依赖的其他沙箱名称列表。
            workdir (str, 可选): 工作目录路径。
            shell (str, 可选): 使用的 Shell 程序。
            scripts (Dict[str, str], 可选): 可执行的脚本字典。
            exec (str, 可选): 要执行的命令。
            timeout (float): 等待沙箱启动的超时时间（秒）。
                - 默认 180 秒
                - 客户端超时比服务器超时多 30 秒，以考虑网络延迟

        异常：
            RuntimeError: 沙箱启动失败或服务器通信失败。
            TimeoutError: 沙箱在指定时间内未启动。

        使用示例：
            sandbox = PythonSandbox()
            await sandbox.start(memory=1024, cpus=2.0)
            # 使用沙箱...
            await sandbox.stop()
        """
        # 如果已经启动，直接返回
        if self._is_started:
            return

        # 确定使用的镜像
        sandbox_image = image or await self.get_default_image()

        # 构建配置对象
        config = {
            "image": sandbox_image,
            "memory": memory,
            "cpus": int(round(cpus)),
        }
        # 仅当参数被显式提供时才添加到配置中
        if volumes is not None:
            config["volumes"] = volumes
        if ports is not None:
            config["ports"] = ports
        if envs is not None:
            config["envs"] = envs
        if depends_on is not None:
            config["depends_on"] = depends_on
        if workdir is not None:
            config["workdir"] = workdir
        if shell is not None:
            config["shell"] = shell
        if scripts is not None:
            config["scripts"] = scripts
        if exec is not None:
            config["exec"] = exec

        # 构建 JSON-RPC 请求
        request_data = {
            "jsonrpc": "2.0",
            "method": "sandbox.start",
            "params": {
                "sandbox": self._name,
                "config": config,
            },
        }

        # 设置请求头
        headers = {"Content-Type": "application/json"}
        if self._api_key:
            headers["Authorization"] = f"Bearer {self._api_key}"

        try:
            # 设置客户端超时时间（比服务器超时多 30 秒）
            # 以考虑网络延迟和处理时间
            client_timeout = aiohttp.ClientTimeout(total=timeout + 30)

            async with self._session.post(
                f"{self._server_url}/api/v1/sandbox/start",
                json=request_data,
                headers=headers,
                timeout=client_timeout,
            ) as response:
                if response.status != 200:
                    error_text = await response.text()
                    raise RuntimeError(f"Failed to start sandbox: {error_text}")

                response_data = await response.json()

                # 检查响应消息
                # 服务器可能返回超时但沙箱仍在启动中
                message = response_data.get("message", "")
                if isinstance(message, str) and "timed out waiting" in message:
                    # 服务器超时但仍启动了沙箱
                    # 发出警告但仍认为已启动
                    import warnings

                    warnings.warn(f"Sandbox start warning: {message}")

                self._is_started = True
        except aiohttp.ClientError as e:
            if isinstance(e, asyncio.TimeoutError):
                raise TimeoutError(
                    f"Timed out waiting for sandbox to start after {timeout} seconds"
                ) from e
            raise RuntimeError(f"Failed to communicate with Microsandbox server: {e}")

    async def stop(self) -> None:
        """
        停止沙箱容器。

        此方法向 Microsandbox 服务器发送请求，停止并清理沙箱容器。

        异常：
            RuntimeError: 沙箱停止失败或服务器通信失败。

        使用示例：
            sandbox = PythonSandbox()
            await sandbox.start()
            # 使用沙箱...
            await sandbox.stop()  # 清理资源

        注意事项：
            - 如果沙箱未启动，此方法不执行任何操作
            - 停止后如需再次使用，必须重新调用 start()
        """
        # 如果未启动，直接返回
        if not self._is_started:
            return

        # 构建 JSON-RPC 停止请求
        request_data = {
            "jsonrpc": "2.0",
            "method": "sandbox.stop",
            "params": {"sandbox": self._name},
            "id": str(uuid.uuid4()),
        }

        headers = {"Content-Type": "application/json"}
        if self._api_key:
            headers["Authorization"] = f"Bearer {self._api_key}"

        try:
            async with self._session.post(
                f"{self._server_url}/api/v1/sandbox/stop",
                json=request_data,
                headers=headers,
            ) as response:
                if response.status != 200:
                    error_text = await response.text()
                    raise RuntimeError(f"Failed to stop sandbox: {error_text}")

                self._is_started = False
        except aiohttp.ClientError as e:
            raise RuntimeError(f"Failed to communicate with Microsandbox server: {e}")

    @abstractmethod
    async def run(self, code: str):
        """
        在沙箱中执行代码。

        这是一个抽象方法，必须由子类实现。

        参数：
            code (str): 要执行的代码。

        返回：
            Execution: 表示已执行代码的对象，包含输出和状态信息。

        异常：
            RuntimeError: 执行失败或沙箱未启动。

        实现示例：
            PythonSandbox.run() - 执行 Python 代码
            NodeSandbox.run() - 执行 JavaScript 代码
        """
        pass

    @property
    def command(self):
        """
        访问命令命名空间，用于在沙箱中执行系统命令。

        返回：
            Command: 绑定到此沙箱的 Command 实例。

        使用示例：
            sandbox = await PythonSandbox.create()
            cmd = sandbox.command
            result = await cmd.run("ls", ["-la"])
            print(await result.output())
        """
        return Command(self)

    @property
    def metrics(self):
        """
        访问指标命名空间，用于获取沙箱的资源指标。

        返回：
            Metrics: 绑定到此沙箱的 Metrics 实例。

        使用示例：
            sandbox = await PythonSandbox.create()
            metrics = sandbox.metrics
            cpu = await metrics.cpu()
            memory = await metrics.memory()
            print(f"CPU: {cpu}%, Memory: {memory} MiB")
        """
        return Metrics(self)
