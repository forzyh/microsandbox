#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
沙箱指标监控示例 (Sandbox Metrics Examples)

本脚本演示了如何获取沙箱的资源指标。

演示内容：
    1. 获取单个指标（CPU、内存、磁盘）
    2. 一次性获取所有指标
    3. 持续监控指标
    4. 生成 CPU 负载测试指标
    5. 错误处理

运行前准备：
    1. 安装包：pip install -e .
    2. 启动 Microsandbox 服务器 (microsandbox-server)
    3. 运行此脚本：python -m examples.metrics

注意：
    - 如果服务器启用了认证，需要设置 MSB_API_KEY 环境变量
"""

import asyncio
import time

import aiohttp
from microsandbox import PythonSandbox


async def basic_metrics_example():
    """
    获取单个指标的基本示例。

    演示如何获取沙箱的 CPU、内存、磁盘使用情况和运行状态。
    """
    print("\n=== 基本指标示例 ===")

    # 使用上下文管理器创建沙箱
    async with PythonSandbox.create(name="metrics-example") as sandbox:
        # 运行命令生成一些负载
        print("运行命令生成一些沙箱活动...")
        await sandbox.command.run("ls", ["-la", "/"])
        await sandbox.command.run(
            "dd", ["if=/dev/zero", "of=/tmp/testfile", "bs=1M", "count=10"]
        )

        # 等待片刻让指标更新
        time.sleep(1)

        # 获取单个指标
        print("\n获取此沙箱的单个指标：")

        try:
            # 获取 CPU 使用率
            cpu = await sandbox.metrics.cpu()
            # CPU 指标在空闲时可能为 0.0，或者在不可用时为 None
            if cpu is None:
                print("CPU 使用率：不可用")
            else:
                print(f"CPU 使用率：{cpu}%")

            # 获取内存使用量
            memory = await sandbox.metrics.memory()
            print(f"内存使用量：{memory or '不可用'} MiB")

            # 获取磁盘使用量
            disk = await sandbox.metrics.disk()
            print(f"磁盘使用量：{disk or '不可用'} 字节")

            # 检查运行状态
            running = await sandbox.metrics.is_running()
            print(f"运行状态：{running}")
        except RuntimeError as e:
            print(f"获取指标时出错：{e}")


async def all_metrics_example():
    """
    一次性获取所有指标的示例。

    演示如何使用 metrics.all() 方法一次性获取所有指标数据。
    """
    print("\n=== 所有指标示例 ===")

    # 创建沙箱
    async with PythonSandbox.create(name="all-metrics-example") as sandbox:
        try:
            # 运行一些命令生成活动
            print("运行命令生成一些沙箱活动...")
            await sandbox.command.run("cat", ["/etc/os-release"])
            # 使用简单的命令，避免超时或错误
            await sandbox.command.run("ls", ["-la", "/usr"])

            # 等待片刻让指标更新
            time.sleep(1)

            # 一次性获取所有指标
            print("\n以字典形式获取所有指标：")
            all_metrics = await sandbox.metrics.all()

            # 打印格式化的指标
            print(f"沙箱：{all_metrics.get('name')}")
            print(f"  运行状态：{all_metrics.get('running')}")

            # 处理 CPU 指标（可能为 0.0 或 None）
            cpu = all_metrics.get("cpu_usage")
            if cpu is None:
                print("  CPU 使用率：不可用")
            else:
                print(f"  CPU 使用率：{cpu}%")

            print(
                f"  内存使用量：{all_metrics.get('memory_usage') or '不可用'} MiB"
            )
            print(
                f"  磁盘使用量：{all_metrics.get('disk_usage') or '不可用'} 字节"
            )
        except Exception as e:
            print(f"all_metrics_example 错误：{e}")


async def continuous_monitoring_example():
    """
    持续监控沙箱指标的示例。

    演示如何在一段时间内持续监控沙箱的资源使用情况。
    """
    print("\n=== 持续监控示例 ===")

    # 创建沙箱
    async with PythonSandbox.create(name="monitoring-example") as sandbox:
        try:
            print("开始持续监控（5 秒）...")

            # 使用简单安全的命令生成负载
            _ = await sandbox.command.run(
                "sh",
                [
                    "-c",
                    "for i in $(seq 1 5); do ls -la / > /dev/null; sleep 0.2; done &",
                ],
            )

            # 监控 5 秒
            start_time = time.time()
            while time.time() - start_time < 5:
                try:
                    # 获取指标
                    cpu = await sandbox.metrics.cpu()
                    memory = await sandbox.metrics.memory()

                    # 格式化 CPU 使用率（可能为 0.0 或 None）
                    cpu_str = f"{cpu}%" if cpu is not None else "不可用"

                    # 打印当前值
                    print(
                        f"[{time.time() - start_time:.1f}秒] CPU: {cpu_str}, 内存：{memory or '不可用'} MiB"
                    )
                except Exception as e:
                    print(f"获取指标时出错：{e}")

                # 等待下次检查
                await asyncio.sleep(1)

            print("监控完成。")
        except Exception as e:
            print(f"continuous_monitoring_example 错误：{e}")


async def cpu_load_test_example():
    """
    生成 CPU 负载以测试 CPU 指标的示例。

    演示如何运行 CPU 密集型任务并监控 CPU 使用情况。
    """
    print("\n=== CPU 负载测试示例 ===")

    # 创建沙箱
    async with PythonSandbox.create(name="cpu-load-test") as sandbox:
        try:
            # 运行 CPU 密集型 Python 脚本
            print("运行 CPU 密集型任务...")

            # 首先创建一个 Python 脚本用于使用 CPU
            cpu_script = """
import time
start = time.time()
duration = 10  # 秒

# CPU 密集型计算
while time.time() - start < duration:
    # 计算质数 - CPU 密集型
    for i in range(1, 100000):
        is_prime = True
        for j in range(2, int(i ** 0.5) + 1):
            if i % j == 0:
                is_prime = False
                break

    # 每秒打印进度
    elapsed = time.time() - start
    if int(elapsed) == elapsed:
        print(f"已运行 {int(elapsed)} 秒...")

print("CPU 负载测试完成")
"""
            # 将脚本写入文件
            await sandbox.command.run(
                "bash", ["-c", f"cat > /tmp/cpu_test.py << 'EOF'\n{cpu_script}\nEOF"]
            )

            # 在后台运行脚本
            print("开始 CPU 测试（运行 10 秒）...")
            await sandbox.command.run("python", ["/tmp/cpu_test.py", "&"])

            # 在脚本运行时监控 CPU 使用情况
            print("\n监控 CPU 使用情况...")
            for i in range(5):
                # 等待片刻
                await asyncio.sleep(2)

                # 获取指标
                cpu = await sandbox.metrics.cpu()
                memory = await sandbox.metrics.memory()

                # 格式化 CPU 使用率（可能为 0.0 或 None）
                cpu_str = f"{cpu}%" if cpu is not None else "不可用"

                # 打印当前值
                print(
                    f"[{i * 2} 秒] CPU: {cpu_str}, 内存：{memory or '不可用'} MiB"
                )

            print("CPU 负载测试完成。")
        except Exception as e:
            print(f"cpu_load_test_example 错误：{e}")


async def error_handling_example():
    """
    指标监控的错误处理示例。

    演示如何在未启动沙箱时正确处理指标请求错误。
    """
    print("\n=== 错误处理示例 ===")

    # 创建沙箱但不立即启动
    sandbox = PythonSandbox(name="error-example")

    try:
        # 尝试在启动沙箱之前获取指标
        print("尝试在启动沙箱之前获取指标...")
        cpu = await sandbox.metrics.cpu()
        print(f"CPU: {cpu}%")  # 这不应该被执行到
    except RuntimeError as e:
        print(f"预期错误：{e}")

    try:
        # 正确启动沙箱
        print("\n正确启动沙箱...")
        sandbox._session = aiohttp.ClientSession()
        await sandbox.start()

        # 启动后获取指标
        cpu = await sandbox.metrics.cpu()
        # 格式化 CPU 使用率（可能为 0.0 或 None）
        cpu_str = f"{cpu}%" if cpu is not None else "不可用"
        print(f"启动后的 CPU 使用率：{cpu_str}")
    except Exception as e:
        print(f"错误：{e}")
    finally:
        # 清理
        if sandbox._is_started:
            await sandbox.stop()
        if sandbox._session and not sandbox._session.closed:
            await sandbox._session.close()


async def main():
    """运行所有示例的主函数。"""
    print("沙箱指标示例")
    print("=======================")

    try:
        await basic_metrics_example()
        await all_metrics_example()
        await continuous_monitoring_example()
        await cpu_load_test_example()
        await error_handling_example()
    except Exception as e:
        print(f"main 错误：{e}")

    print("\n所有示例完成！")


if __name__ == "__main__":
    asyncio.run(main())
