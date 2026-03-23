"""
指标监控模块 (Metrics)

本模块提供了获取沙箱资源使用情况的接口。

主要类：
    Metrics: 用于检索沙箱的资源指标

支持的指标类型：
    - CPU 使用率 (cpu 方法)
    - 内存使用量 (memory 方法)
    - 磁盘使用量 (disk 方法)
    - 运行状态 (is_running 方法)
    - 所有指标 (all 方法)

使用示例：
    from microsandbox import PythonSandbox

    async with PythonSandbox.create() as sandbox:
        metrics = sandbox.metrics

        # 获取单个指标
        cpu = await metrics.cpu()
        memory = await metrics.memory()

        # 获取所有指标
        all_metrics = await metrics.all()
        print(f"CPU: {all_metrics.get('cpu_usage')}%")
        print(f"内存：{all_metrics.get('memory_usage')} MiB")
"""

import uuid
from typing import Optional


class Metrics:
    """
    指标监控类 (Metrics Class)

    此类提供了获取沙箱资源使用情况的方法。

    此类通过 BaseSandbox 的 metrics 属性访问，不直接实例化。

    主要功能：
        - 获取 CPU 使用率 (cpu 方法)
        - 获取内存使用量 (memory 方法)
        - 获取磁盘使用量 (disk 方法)
        - 获取所有指标 (all 方法)
        - 检查运行状态 (is_running 方法)

    使用示例：
        sandbox = await PythonSandbox.create()
        metrics = sandbox.metrics

        # 获取 CPU 使用率
        cpu = await metrics.cpu()
        print(f"CPU 使用率：{cpu}%")

        # 持续监控
        while True:
            cpu = await metrics.cpu()
            memory = await metrics.memory()
            print(f"CPU: {cpu}%, Memory: {memory} MiB")
    """

    def __init__(self, sandbox_instance):
        """
        初始化指标实例。

        参数：
            sandbox_instance: 此指标对象所属的沙箱实例。
                通过沙箱实例访问服务器连接和配置。

        注意事项：
            此构造函数通常不直接调用，而是通过 BaseSandbox.metrics 属性访问。
        """
        self._sandbox = sandbox_instance

    async def _get_metrics(self) -> dict:
        """
        内部方法，从服务器获取当前指标。

        此方法发送 JSON-RPC 请求到 Microsandbox 服务器，
        获取指定沙箱的资源使用情况。

        返回：
            dict: 包含沙箱指标数据的字典。
                可能的键：
                - "name": 沙箱名称
                - "running": 是否正在运行
                - "cpu_usage": CPU 使用率（百分比）
                - "memory_usage": 内存使用量（MiB）
                - "disk_usage": 磁盘使用量（字节）

        异常：
            RuntimeError: 服务器请求失败。

        注意事项：
            - 这是内部方法，通常不直接调用
            - 沙箱必须先启动才能获取指标
        """
        # 检查沙箱是否已启动
        if not self._sandbox._is_started:
            raise RuntimeError("Sandbox is not started. Call start() first.")

        # 设置请求头
        headers = {"Content-Type": "application/json"}
        if self._sandbox._api_key:
            headers["Authorization"] = f"Bearer {self._sandbox._api_key}"

        # 构建 JSON-RPC 请求数据
        request_data = {
            "jsonrpc": "2.0",
            "method": "sandbox.metrics.get",
            "params": {
                "sandbox": self._sandbox._name,
            },
            "id": str(uuid.uuid4()),
        }

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
                    raise RuntimeError(f"Failed to get sandbox metrics: {error_text}")

                # 解析 JSON 响应
                response_data = await response.json()

                # 检查是否有错误
                if "error" in response_data:
                    raise RuntimeError(
                        f"Failed to get sandbox metrics: {response_data['error']['message']}"
                    )

                # 获取结果数据
                result = response_data.get("result", {})
                sandboxes = result.get("sandboxes", [])

                # 期望响应中正好有一个沙箱（我们自己的）
                if not sandboxes:
                    return {}

                # 返回第一个（也是唯一一个）沙箱的数据
                return sandboxes[0]
        except Exception as e:
            raise RuntimeError(f"Failed to get sandbox metrics: {e}")

    async def all(self) -> dict:
        """
        获取当前沙箱的所有指标。

        返回：
            dict: 包含所有沙箱指标的字典：
                {
                    "name": str,              # 沙箱名称
                    "running": bool,          # 是否正在运行
                    "cpu_usage": float,       # CPU 使用率（百分比，0-100）
                    "memory_usage": int,      # 内存使用量（MiB）
                    "disk_usage": int         # 磁盘使用量（字节）
                }
                如果指标不可用，对应字段可能为 null 或不存在。

        异常：
            RuntimeError: 沙箱未启动或请求失败。

        使用示例：
            metrics = await sandbox.metrics.all()
            print(f"沙箱：{metrics['name']}")
            print(f"运行状态：{metrics['running']}")
            print(f"CPU: {metrics['cpu_usage']}%")
            print(f"内存：{metrics['memory_usage']} MiB")
        """
        return await self._get_metrics()

    async def cpu(self) -> Optional[float]:
        """
        获取当前沙箱的 CPU 使用率。

        返回：
            float, optional: CPU 使用率百分比（0-100）。
                - None: 当指标不可用时
                - 0.0: 沙箱空闲或指标不精确时可能返回

        异常：
            RuntimeError: 沙箱未启动或请求失败。

        使用示例：
            cpu = await metrics.cpu()
            if cpu is not None:
                print(f"CPU 使用率：{cpu}%")
            else:
                print("CPU 指标不可用")
        """
        metrics = await self._get_metrics()
        return metrics.get("cpu_usage")

    async def memory(self) -> Optional[int]:
        """
        获取当前沙箱的内存使用量。

        返回：
            int, optional: 内存使用量，单位 MiB。
                - None: 当指标不可用时

        异常：
            RuntimeError: 沙箱未启动或请求失败。

        使用示例：
            memory = await metrics.memory()
            if memory is not None:
                print(f"内存使用：{memory} MiB")
            else:
                print("内存指标不可用")
        """
        metrics = await self._get_metrics()
        return metrics.get("memory_usage")

    async def disk(self) -> Optional[int]:
        """
        获取当前沙箱的磁盘使用量。

        返回：
            int, optional: 磁盘使用量，单位字节。
                - None: 当指标不可用时

        异常：
            RuntimeError: 沙箱未启动或请求失败。

        使用示例：
            disk = await metrics.disk()
            if disk is not None:
                print(f"磁盘使用：{disk} 字节 ({disk / 1024 / 1024:.2f} MB)")
            else:
                print("磁盘指标不可用")
        """
        metrics = await self._get_metrics()
        return metrics.get("disk_usage")

    async def is_running(self) -> bool:
        """
        检查沙箱是否正在运行。

        返回：
            bool: True 表示沙箱正在运行，False 表示未运行。

        异常：
            RuntimeError: 请求失败。

        使用示例：
            if await metrics.is_running():
                print("沙箱正在运行")
            else:
                print("沙箱已停止")
        """
        metrics = await self._get_metrics()
        return metrics.get("running", False)
