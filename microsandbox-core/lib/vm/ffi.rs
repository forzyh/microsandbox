//! libkrun FFI 绑定
//!
//! 本模块提供了与 libkrun 库的 FFI（Foreign Function Interface）绑定。
//! libkrun 是用于创建和管理微虚拟机的底层库。

use std::ffi::c_char;

//--------------------------------------------------------------------------------------------------
// FFI 绑定
//--------------------------------------------------------------------------------------------------

/// 链接 libkrun 库
///
/// 以下所有函数都是 libkrun 库提供的 C API 的 Rust 绑定
#[link(name = "krun")]
unsafe extern "C" {
    /// 设置库的日志级别
    ///
    /// ## 参数
    /// * `level` - 要设置的日志级别：
    ///   - `0` - Off（关闭）
    ///   - `1` - Error（错误）
    ///   - `2` - Warn（警告）
    ///   - `3` - Info（信息）
    ///   - `4` - Debug（调试）
    ///   - `5` - Trace（追踪）
    ///
    /// ## 返回值
    /// 成功返回 0，失败返回负的错误码
    pub(crate) fn krun_set_log_level(level: u32) -> i32;

    /// 创建配置上下文
    ///
    /// ## 返回值
    /// 成功返回上下文 ID，失败返回负的错误码
    pub(crate) fn krun_create_ctx() -> i32;

    /// 释放配置上下文
    ///
    /// ## 参数
    /// * `ctx_id` - 要释放的配置上下文 ID
    ///
    /// ## 返回值
    /// 成功返回 0，失败返回负的错误码
    pub(crate) fn krun_free_ctx(ctx_id: u32) -> i32;

    /// 设置 MicroVm 的基本配置参数
    ///
    /// ## 参数
    /// * `ctx_id` - 配置上下文 ID
    /// * `num_vcpus` - vCPU 数量
    /// * `ram_mib` - 内存大小（MiB）
    ///
    /// ## 返回值
    /// 成功返回 0，失败返回负的错误码
    pub(crate) fn krun_set_vm_config(ctx_id: u32, num_vcpus: u8, ram_mib: u32) -> i32;

    /// 设置 MicroVm 的根文件系统路径
    ///
    /// 不适用于 libkrun-SEV 版本。
    ///
    /// ## 参数
    /// * `ctx_id` - 配置上下文 ID
    /// * `root_path` - 要用作根的路径
    ///
    /// ## 返回值
    /// 成功返回 0，失败返回负的错误码
    ///
    /// ## 错误情况
    /// * `-EEXIST` - 已经设置了根设备
    ///
    /// ## 注意事项
    /// 此函数与 `krun_set_overlayfs_root` 互斥，不能同时使用。
    pub(crate) fn krun_set_root(ctx_id: u32, root_path: *const c_char) -> i32;

    /// 设置使用 OverlayFS 作为 MicroVm 的根文件系统
    ///
    /// 不适用于 libkrun-SEV 版本。
    ///
    /// ## 参数
    /// * `ctx_id` - 配置上下文 ID
    /// * `root_layers` - 空终止字符串指针数组，表示用作 OverlayFS 层的路径
    ///   必须至少包含一个层
    ///
    /// ## 返回值
    /// 成功返回 0，失败返回负的错误码
    ///
    /// ## 错误情况
    /// * `-EINVAL` - 未提供任何层
    /// * `-EEXIST` - 已经设置了根设备
    ///
    /// ## 注意事项
    /// 此函数与 `krun_set_root` 互斥，不能同时使用。
    pub(crate) fn krun_set_overlayfs_root(ctx_id: u32, root_layers: *const *const c_char) -> i32;

    /// 添加磁盘映像作为 MicroVm 的通用分区
    ///
    /// 此 API 与已弃用的 `krun_set_root_disk` 和 `krun_set_data_disk` 方法互斥，
    /// 不得一起使用。
    ///
    /// ## 参数
    /// * `ctx_id` - 配置上下文 ID
    /// * `block_id` - 空终止字符串，表示分区
    /// * `disk_path` - 空终止字符串，表示包含根文件系统的路径
    /// * `read_only` - 挂载是否为只读（当调用者对 /usr/share 中的磁盘映像
    ///   没有写权限时需要）
    #[allow(dead_code)]
    pub(crate) fn krun_add_disk(
        ctx_id: u32,
        block_id: *const c_char,
        disk_path: *const c_char,
        read_only: bool,
    ) -> i32;

    /// 添加独立的 virtio-fs 设备，指向主机目录并带有标签
    ///
    /// virtio-fs 是一种高性能的虚拟化文件系统协议，用于在主机和访客
    /// 之间共享目录。
    ///
    /// ## 参数
    /// * `ctx_id` - 配置上下文 ID
    /// * `c_tag` - 用于在访客中标识文件系统的标签
    /// * `c_path` - 主机上要向访客暴露的目录的完整路径
    ///
    /// ## 返回值
    /// 成功返回 0，失败返回负的错误码
    pub(crate) fn krun_add_virtiofs(
        ctx_id: u32,
        c_tag: *const c_char,
        c_path: *const c_char,
    ) -> i32;

    /// 添加独立的 virtio-fs 设备，指向主机目录并带有标签
    ///
    /// 此变体允许指定 DAX 窗口的大小。DAX（Direct Access）允许
    /// 直接访问持久内存，提高性能。
    ///
    /// ## 参数
    /// * `ctx_id` - 配置上下文 ID
    /// * `c_tag` - 用于在访客中标识文件系统的标签
    /// * `c_path` - 主机上要向访客暴露的目录的完整路径
    /// * `shm_size` - DAX 共享内存窗口的大小（字节）
    #[allow(dead_code)]
    pub(crate) fn krun_add_virtiofs2(
        ctx_id: u32,
        c_tag: *const c_char,
        c_path: *const c_char,
        shm_size: u64,
    ) -> i32;

    /// 配置网络使用 passt
    ///
    /// 调用此函数会禁用 TSI 后端，改用 passt。
    /// passt 是一种用户态的网络代理，提供安全的网络连接。
    ///
    /// ## 参数
    /// * `ctx_id` - 配置上下文 ID
    /// * `fd` - 用于与 passt 通信的文件描述符
    #[allow(dead_code)]
    pub(crate) fn krun_set_passt_fd(ctx_id: u32, fd: i32) -> i32;

    /// 配置网络使用 gvproxy（vfkit 模式）
    ///
    /// 调用此函数会禁用 TSI 后端，改用 gvproxy。
    /// gvproxy 是 gVisor 的网络代理实现。
    ///
    /// ## 参数
    /// * `ctx_id` - 配置上下文 ID
    /// * `c_path` - gvproxy 二进制文件的路径
    ///
    /// ## 注意事项
    /// 如果从不调用此函数，网络将使用 TSI 后端。
    /// 此函数应在 `krun_set_port_map` 之前调用。
    #[allow(dead_code)]
    pub(crate) fn krun_set_gvproxy_path(ctx_id: u32, c_path: *const c_char) -> i32;

    /// 设置 virtio-net 设备的 MAC 地址（使用 passt 后端时）
    ///
    /// ## 参数
    /// * `ctx_id` - 配置上下文 ID
    /// * `c_mac` - MAC 地址，6 字节的 u8 数组
    #[allow(dead_code)]
    pub(crate) fn krun_set_net_mac(ctx_id: u32, c_mac: *const u8) -> i32;

    /// 配置 MicroVm 的主机到访客 TCP 端口映射
    ///
    /// ## 参数
    /// * `ctx_id` - 配置上下文 ID
    /// * `c_port_map` - **空终止**字符串指针数组，格式为 "host_port:guest_port"
    ///
    /// ## 注意事项
    /// 传递 NULL（或不调用此函数）作为 "port_map" 与传递空数组有不同的含义：
    /// - NULL：指示 libkrun 尝试将访客中所有监听的端口暴露给主机
    /// - 空数组：表示访客中的端口都不会暴露给主机
    ///
    /// 暴露的端口在访客中也只能通过 "host_port" 访问。这意味着对于
    /// "8080:80" 这样的映射，访客内的应用程序也需要通过 "8080" 端口访问服务。
    ///
    /// 如果使用 passt 网络模式（调用了 `krun_set_passt_fd`），端口映射不作为
    /// libkrun 的 API 支持（但仍可以使用 passt 的命令行参数进行端口映射）。
    pub(crate) fn krun_set_port_map(ctx_id: u32, c_port_map: *const *const c_char) -> i32;

    /// 为 TSI 网络后端配置静态 IP、子网和范围
    ///
    /// ## 参数
    /// * `ctx_id` - 配置上下文 ID
    /// * `c_ip` - 可选的空终止字符串，表示访客的静态 IPv4 地址
    /// * `c_subnet` - 可选的空终止字符串，表示 CIDR 格式的访客子网
    ///   （如 "192.168.1.0/24"）
    /// * `scope` - 整数，指定范围（0-3）：
    ///   - `0` - None（无） - 阻止所有 IP 通信
    ///   - `1` - Group（组） - 允许子网内通信（如果指定；否则像范围 0 一样阻止所有）
    ///   - `2` - Public（公共） - 允许公共 IP
    ///   - `3` - Any（任意） - 允许任何 IP
    ///
    /// ## 返回值
    /// 成功返回 0，失败返回负的错误码
    ///
    /// ## 错误情况
    /// * `-EINVAL` - 范围值 > 3 或 IP/子网字符串无效
    /// * `-ENOTSUP` - 网络模式不是 TSI
    ///
    /// ## 注意事项
    /// 此函数仅在使用默认 TSI 网络后端时有效（即未调用
    /// `krun_set_passt_fd` 或 `krun_set_gvproxy_path`）。
    pub(crate) fn krun_set_tsi_scope(
        ctx_id: u32,
        c_ip: *const c_char,
        c_subnet: *const c_char,
        scope: u8,
    ) -> i32;

    /// 启用并配置 virtio-gpu 设备
    ///
    /// virtio-gpu 是虚拟化的 GPU 设备，用于提供图形加速功能。
    ///
    /// ## 参数
    /// * `ctx_id` - 配置上下文 ID
    /// * `virgl_flags` - 要传递给 virglrenderer 的标志
    #[allow(dead_code)]
    pub(crate) fn krun_set_gpu_options(ctx_id: u32, virgl_flags: u32) -> i32;

    /// 启用并配置 virtio-gpu 设备
    ///
    /// 此变体允许指定主机窗口的大小（在访客中充当显存）。
    ///
    /// ## 参数
    /// * `ctx_id` - 配置上下文 ID
    /// * `virgl_flags` - 要传递给 virglrenderer 的标志
    /// * `shm_size` - 共享内存主机窗口的大小（字节）
    #[allow(dead_code)]
    pub(crate) fn krun_set_gpu_options2(ctx_id: u32, virgl_flags: u32, shm_size: u64) -> i32;

    /// 启用或禁用 virtio-snd 设备
    ///
    /// virtio-snd 是虚拟化的音频设备。
    ///
    /// ## 参数
    /// * `ctx_id` - 配置上下文 ID
    /// * `enable` - 是否启用音频设备
    #[allow(dead_code)]
    pub(crate) fn krun_set_snd_device(ctx_id: u32, enable: bool) -> i32;

    /// 配置要在访客中设置的 rlimits 映射
    ///
    /// rlimits（resource limits）用于限制进程可使用的系统资源。
    ///
    /// ## 参数
    /// * `ctx_id` - 配置上下文 ID
    /// * `c_rlimits` - **空终止**字符串指针数组，格式为
    ///   "<RESOURCE_NUMBER>=RLIM_CUR:RLIM_MAX"（如 "6=1024:1024"）
    pub(crate) fn krun_set_rlimits(ctx_id: u32, c_rlimits: *const *const c_char) -> i32;

    /// 设置 MicroVm 的 SMBIOS OEM 字符串
    ///
    /// SMBIOS OEM 字符串可用于向访客传递自定义信息。
    ///
    /// ## 参数
    /// * `ctx_id` - 配置上下文 ID
    /// * `c_oem_strings` - 字符串指针数组，必须用额外的 NULL 指针终止
    #[allow(dead_code)]
    pub(crate) fn krun_set_smbios_oem_strings(
        ctx_id: u32,
        c_oem_strings: *const *const c_char,
    ) -> i32;

    /// 设置 MicroVm 内要运行的可执行文件的工作目录
    ///
    /// ## 参数
    /// * `ctx_id` - 配置上下文 ID
    /// * `c_workdir_path` - 工作目录路径，相对于用 "krun_set_root" 配置的根
    pub(crate) fn krun_set_workdir(ctx_id: u32, c_workdir_path: *const c_char) -> i32;

    /// 设置 MicroVm 内要运行的可执行文件的路径、传递给可执行文件的参数
    /// 以及要在可执行文件上下文中配置的环境变量
    ///
    /// ## 参数
    /// * `ctx_id` - 配置上下文 ID
    /// * `c_exec_path` - 可执行文件路径，相对于用 "krun_set_root" 配置的根
    /// * `c_argv` - **空终止**字符串指针数组，要作为参数传递
    /// * `c_envp` - **空终止**字符串指针数组，要注入到可执行文件上下文中的
    ///   环境变量
    ///
    /// ## 注意事项
    /// 为 `c_envp` 传递 NULL 将自动生成一个数组，收集当前环境中存在的变量。
    pub(crate) fn krun_set_exec(
        ctx_id: u32,
        c_exec_path: *const c_char,
        c_argv: *const *const c_char,
        c_envp: *const *const c_char,
    ) -> i32;

    /// 设置要在可执行文件上下文中配置的环境变量
    ///
    /// ## 参数
    /// * `ctx_id` - 配置上下文 ID
    /// * `c_envp` - **空终止**字符串指针数组，要注入到可执行文件上下文中的
    ///   环境变量
    ///
    /// ## 注意事项
    /// 为 `c_envp` 传递 NULL 将自动生成一个数组，收集当前环境中存在的变量。
    #[allow(dead_code)]
    pub(crate) fn krun_set_env(ctx_id: u32, c_envp: *const *const c_char) -> i32;

    /// 设置 MicroVm 的 TEE 配置文件路径
    ///
    /// 仅适用于 libkrun-sev 版本。TEE（Trusted Execution Environment）
    /// 用于提供硬件级别的安全隔离。
    ///
    /// ## 参数
    /// * `ctx_id` - 配置上下文 ID
    /// * `c_filepath` - TEE 配置文件的路径
    #[allow(dead_code)]
    pub(crate) fn krun_set_tee_config_file(ctx_id: u32, c_filepath: *const c_char) -> i32;

    /// 添加端口 - 路径配对，用于访客与主机进程进行 IPC
    ///
    /// vsock 是一种用于 VM 和主机之间通信的套接字类型。
    ///
    /// ## 参数
    /// * `ctx_id` - 配置上下文 ID
    /// * `port` - 访客用于 IPC 连接的端口
    /// * `c_filepath` - 主机上 Unix 套接字的路径
    #[allow(dead_code)]
    pub(crate) fn krun_add_vsock_port(ctx_id: u32, port: u32, c_filepath: *const c_char) -> i32;

    /// 获取用于有序关闭访客的 eventfd 文件描述符
    ///
    /// 必须在用 "krun_start_enter" 启动 MicroVm 之前调用。
    /// 仅适用于 libkrun-efi 版本。
    ///
    /// ## 参数
    /// * `ctx_id` - 配置上下文 ID
    ///
    /// ## 返回值
    /// 成功返回 eventfd 文件描述符，失败返回负的错误码
    #[allow(dead_code)]
    pub(crate) fn krun_get_shutdown_eventfd(ctx_id: u32) -> i32;

    /// 设置 MicroVm 控制台输出要写入的文件路径
    ///
    /// ## 参数
    /// * `ctx_id` - 配置上下文 ID
    /// * `c_filepath` - 要写入控制台输出的文件路径
    pub(crate) fn krun_set_console_output(ctx_id: u32, c_filepath: *const c_char) -> i32;

    /// 使用配置的参数启动并进入 MicroVm
    ///
    /// VMM（Virtual Machine Monitor）将尝试接管 stdin/stdout，
    /// 代表隔离环境中运行的进程管理它们，模拟后者直接控制终端的行为。
    ///
    /// 此函数会消耗上下文 ID 指向的配置。
    ///
    /// ## 参数
    /// * `ctx_id` - 配置上下文 ID
    ///
    /// ## 返回值
    /// 此函数仅在 MicroVm 启动前发生错误时返回。否则，VMM 假定它拥有
    /// 进程的完全控制权，并将在 MicroVm 关闭时调用 exit()。
    pub(crate) fn krun_start_enter(ctx_id: u32) -> i32;
}
