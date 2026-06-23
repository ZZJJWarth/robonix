# Robonix 零拷贝传输 — 待办清单

## 阶段 0：基础设施 — Atlas Proto 扩展 ✅

- [x] **0.1** 在 `system/atlas/proto/atlas.proto` 中添加 `TRANSPORT_ROS2_ZC = 4`
- [x] **0.2** 添加 `Ros2ZcParams` 消息（共享内存名称、段大小等参数）
- [x] **0.3** 重新生成 proto 桩代码（`cargo build -p robonix-atlas`）— ✅ 编译通过
- [x] **0.4** 在 `pylib/robonix-api/robonix_api/atlas_types.py` 中添加 `ROS2_ZC = 4` 和 `Ros2ZcParams` 数据类
- [x] **0.5** 更新 `system/atlas/src/service.rs`（TransportParamsState、parse_params、atlas_can_mint）
- [x] **0.6** 更新 `pylib/robonix-api/robonix_api/atlas.py`（transport 解析、pb 映射、params 转换）
- [x] **0.7** 更新 `pylib/robonix-api/robonix_api/capability.py`（mode-transport 兼容矩阵、类型导入）

## 阶段 1：ZC 共享库 — ROS 项目重构产出 `librobonix_zc.so`

> **策略**：不搬源码到 robonix。在 ROS 项目（`/home/warth/Desktop/build/ros`）中将头文件内联实现拆成 `.hpp` 声明 + `.cpp` 编译单元，产出共享库。Robonix 侧只写链接此 `.so` 的 pybind11 桥接层。

### 1A. 重构 `zc_shm.hpp` → `zc_shm.hpp` + `zc_shm.cpp`

- [ ] **1A.1** 将全局变量 `shm`、`manager_` 移到 `zc_shm.cpp`，头文件改为 `extern` 声明
- [ ] **1A.2** 将 `shm_init()` / `shm_shutdown()` 实现移到 `zc_shm.cpp`，头文件保留声明
- [ ] **1A.3** 将 `ShmManager` 成员函数实现移到 `zc_shm.cpp`（`add_subscriber`、`delete_subscriber`、`publish`、`releaseMessage`、`ShmImageGetNew` 等）
- [ ] **1A.4** 头文件保留所有数据结构定义（`ShmImage`、`ShmPointCloud2`、`ShmTopic` 等）——这些是 POD/容器类型，需在编译期可见

### 1B. 重构 `zc_pubsub.hpp` → 拆分非模板函数

- [ ] **1B.1** 将 `zc_interop` 命名空间函数移到新文件 `src/lib/zc_interop.cpp`（`ensure_shm_ready_or_throw`、`prepare_publish`、`erase_from_processing_queue` 等）
- [ ] **1B.2** `ZcNode`、`ZcPublisher` 模板类保留在 `zc_pubsub.hpp`（模板必须 header-only）
- [ ] **1B.3** 显式模板实例化：在 `.cpp` 中为 `Image` 和 `PointCloud2` 实例化 `ZcNode::copy_from_shm`、`ZcPublisher::publish`

### 1C. CMake 添加共享库 target

- [ ] **1C.1** 在 `CMakeLists.txt` 添加 `add_library(robonix_zc SHARED src/lib/zc_shm.cpp src/lib/zc_interop.cpp)`
- [ ] **1C.2** 链接依赖：`iceoryx_posh`、`rclcpp`、`std_msgs`、`sensor_msgs`
- [ ] **1C.3** 安装 target：`install(TARGETS robonix_zc ... DESTINATION lib/${PROJECT_NAME})`
- [ ] **1C.4** 编译验证：`colcon build --packages-select cpp_pubsub`

## 阶段 2：Robonix 侧桥接层 — `pylib/robonix-zc/`

> Robonix 侧**零 C++ 源码搬运**。只有 pybind11 胶水代码 + Python wrapper，链接外部 `librobonix_zc.so`。

### 2A. pybind11 桥接

- [ ] **2A.1** 创建 `pylib/robonix-zc/src/_zc_bridge.cpp`
  - `#include "zc_pubsub.hpp"`（从 ROS 项目的 include 路径）
  - 导出函数：`init_shm`、`shutdown_shm`、`create_image`、`create_pointcloud2`、`take_image`、`take_pointcloud2`、`prepare_publish`、`register_subscriber`、`unregister_subscriber`
- [ ] **2A.2** 编写 `CMakeLists.txt`：`pybind11_add_module` + `target_link_libraries(... robonix_zc)`
- [ ] **2A.3** 编译验证：确保 `import robonix_zc._zc_bridge` 可导入

### 2B. Python 封装

- [ ] **2B.1** 创建 `pylib/robonix-zc/robonix_zc/__init__.py`
- [ ] **2B.2** 实现 `ZcPublisher` 类——封装 `_zc_bridge`，提供 `publish(msg)` 方法
- [ ] **2B.3** 实现 `ZcSubscriber` 类——封装 `_zc_bridge`，提供 callback 模式
- [ ] **2B.4** 编写 `pyproject.toml`，在根 `pyproject.toml` 中注册 workspace member

## 阶段 3：首个 ZC 契约 + 参考实现

- [ ] **3.1** 创建 `capabilities/primitive/camera/rgb_zc.v1.toml`
  ```toml
  [contract]
  id      = "robonix/primitive/camera/rgb_zc"
  version = "1"
  kind    = "primitive"
  idl     = "common_interfaces/sensor_msgs/msg/Image.msg"
  description = "RGB 相机流，通过零拷贝共享内存传输。"

  [mode]
  type = "topic_out"
  ```
- [ ] **3.2** 编写参考 provider：`examples/zc_camera_provider.py`
  - 使用 `Primitive` + `ZcPublisher`
  - 完整的生命周期钩子（`@on_init`、`@on_shutdown`）管理共享内存
- [ ] **3.3** 编写参考 consumer：`examples/zc_camera_consumer.py`
  - 使用 `ZcSubscriber` 订阅
  - 对比标准 ROS2 订阅的延迟
- [ ] **3.4** 基准测试：1080p RGB @ 30fps，对比标准 ROS2 vs ZC
  - 端到端延迟（publish → callback）
  - CPU 使用率
  - 内存峰值

## 阶段 4：AI 代码生成工作流

- [ ] **4.1** 实现契约扫描器 `zc_scanner.rs`
  - 递归扫描 `capabilities/` 目录
  - 解析每个 TOML 的 `[contract].idl`
  - 标记重载荷类型：`Image.msg`、`PointCloud2.msg`、`LaserScan.msg`
  - 输出 JSON manifest
- [ ] **4.2** 实现 ZC 契约生成器（扩展 `contract_gen.rs`）
  - 为每个候选契约生成 `*_zc.v1.toml`
  - 自动跳过已存在的 ZC 变体
  - 生成 `description` 字段
- [ ] **4.3** 实现 Python provider 代码生成器 `zc_python_gen.rs`
  - 使用模板引擎（`minijinja` 或字符串模板）
  - 模板变量：类名、命名空间、topic、消息类型
  - 生成完整的 `.py` 文件（包括 import、lifecycle 钩子）
- [ ] **4.4** 在 `robonix-codegen` 中注册 `--lang ros2_zc` 目标
  - 修改 `tools/codegen/src/main.rs`
  - 修改 `tools/codegen/src/codegen/mod.rs`
- [ ] **4.5** 验证生成结果
  ```bash
  robonix-codegen --lang ros2_zc \
    --contracts capabilities/ \
    -I capabilities/lib/ \
    -o gen/zc/
  ```

## 阶段 5：全量覆盖

- [ ] **5.1** 自动生成 `camera/depth_zc.v1.toml` + provider
- [ ] **5.2** 自动生成 `lidar/lidar3d_zc.v1.toml` + provider
- [ ] **5.3** 自动生成 `lidar/lidar_zc.v1.toml` + provider
- [ ] **5.4** 自动生成 `imu/imu_zc.v1.toml` + provider
- [ ] **5.5** 审查所有自动生成的代码，确保符合 CLAUDE.md 规范

## 阶段 6：集成测试

- [ ] **6.1** 在 Webots 示例中部署 ZC 相机
  - 与现有标准相机并存运行
  - 验证两种传输路径同时工作
- [ ] **6.2** Executor 集成测试
  - 通过 Atlas 发现 ZC 契约
  - 验证 `ConnectCapability` 对 `ROS2_ZC` 传输
- [ ] **6.3** 端到端延迟测量
  - 使用 `examples/webots/` 环境
  - 对比 ZC vs 标准 ROS2 的 Plan → Execute → Result 全链路延迟
- [ ] **6.4** 压力测试
  - 多路 ZC 相机同时运行
  - 验证共享内存不会泄漏（引用计数 GC 正确性）

## 阶段 7：文档

- [ ] **7.1** 在 `docs/src/developer-guide.md` 中添加 ZC 传输章节
- [ ] **7.2** 更新 `system/robonix_zc/README.md`（库使用说明）
- [ ] **7.3** 更新 `pylib/robonix-zc/README.md`（Python API 文档）
- [ ] **7.4** 更新 `tools/codegen/README.md`（`--lang ros2_zc` 使用说明）
- [ ] **7.5** 更新 `AGENTS.md` 或 `CLAUDE.md`（概念命名稳定性）

## 阶段 8：CI 和发布

- [ ] **8.1** 在 CI 中添加 `robonix_zc` 库的编译
- [ ] **8.2** 添加 `robonix-codegen --lang ros2_zc` 的冒烟测试
- [ ] **8.3** 运行 `make check`（`cargo fmt --all -- --check` + clippy）
- [ ] **8.4** 运行 `cargo test --workspace --all-targets`
- [ ] **8.5** Pre-commit review（`/pre-commit-review`）
