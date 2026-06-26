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

### 1A. 重构 `zc_shm.hpp` → `zc_shm.hpp` + `zc_shm.cpp` ✅

- [x] **1A.1** 将全局变量 `shm`、`manager_` 移到 `zc_shm.cpp`，头文件改为 `extern` 声明
- [x] **1A.2** 将 `shm_init()` / `shm_shutdown()` 实现移到 `zc_shm.cpp`，头文件保留声明
- [x] **1A.3** 将 `ShmManager` 非模板成员函数实现移到 `zc_shm.cpp`
- [x] **1A.4** 头文件保留所有数据结构定义 + 模板方法（`releaseMessage`、`ShmMessageGetNew`）
- [x] **1A.5** `ShmSubscriberLiveness` 构造/析构移到 `zc_shm.cpp`
- [x] **1A.6** 更新 `CMakeLists.txt`：talker/listener/pybind11 目标链接 `zc_shm.cpp`
- [x] **1A.7** 编译验证：`g++ -std=c++20` 头文件 + 编译单元均通过

### 1B. 重构 `zc_pubsub.hpp` → 拆分非模板函数 ✅

- [x] **1B.1** 将 `zc_interop` 命名空间 5 个函数移到 `src/lib/zc_interop.cpp`（`ensure_shm_ready_or_throw`、`make_object_name`、`parse_object_name`、`erase_from_processing_queue`、`prepare_publish`）
- [x] **1B.2** `ZcNode`、`ZcPublisher` 模板类保留在 `zc_pubsub.hpp`（模板必须 header-only）；`copy_from_shm` 特化保留在头文件（单行转发函数）
- [x] **1B.3** `CMakeLists.txt`：变量 `ZC_SHM_SRC` → `ZC_SRC`，新增 `src/lib/zc_interop.cpp`

### 1C. CMake 添加共享库 target ✅

- [x] **1C.1** 添加 `add_library(robonix_zc SHARED src/lib/zc_shm.cpp src/lib/zc_interop.cpp)`
- [x] **1C.2** `target_include_directories(robonix_zc PUBLIC src/lib)` — 下游 include 路径
- [x] **1C.3** 链接依赖：`ament_target_dependencies(... rclcpp std_msgs sensor_msgs)` + `iceoryx_posh`
- [x] **1C.4** 导出：`ament_export_include_directories` + `ament_export_libraries`
- [x] **1C.5** talker/listener/`_zc_py_bridge` 改为 `target_link_libraries(... robonix_zc)`，不再直接编译源码
- [x] **1C.6** 安装：`install(TARGETS robonix_zc ...)` LIBRARY → `lib/${PROJECT_NAME}`
- [x] **1C.7** 验证：`zc_shm.o` 链接为 `.so`，`nm -D` 确认 `shm_init`/`ShmManager::publish` 等全部符号导出

## 阶段 2：Robonix 侧桥接层 — `pylib/robonix-zc/`

> Robonix 侧**零 C++ 源码搬运**。只有 pybind11 胶水代码 + Python wrapper，链接外部 `librobonix_zc.so`。

### 2A. pybind11 桥接 ✅

- [x] **2A.1** 创建 `pylib/robonix-zc/package.xml`（colcon 包，依赖 cpp_pubsub + rclcpp）
- [x] **2A.2** 创建 `pylib/robonix-zc/CMakeLists.txt`（`pybind11_add_module` + `find_package(cpp_pubsub)` link `robonix_zc`）
- [x] **2A.3** 创建 `src/_zc_bridge.cpp`（270 行，10 个导出函数：`init_shm`、`shutdown_shm`、`register_subscriber`、`unregister_subscriber`、`create_image`、`create_pointcloud2`、`prepare_publish`、`take_image`、`take_pointcloud2`、`take_pointcloud2`）
- [x] **2A.4** 依赖精简：移除 iceoryx_posh（Boost 的 ZC 不需要）、移除 sensor_msgs_library（Shm* 类型不依赖）

### 2B. Python 封装 ✅

- [x] **2B.1** 创建 `robonix_zc/__init__.py` — 导出 `ZcPublisher`、`ZcSubscriber`、`init_shm`、`shutdown_shm`
- [x] **2B.2** 实现 `ZcPublisher` 类——`publish(msg)` 写入 shm → 发布对象名到 String topic
- [x] **2B.3** 实现 `ZcSubscriber` 类——订阅 String topic → 查找 shm → 解包为 ROS msg → 回调
- [x] **2B.4** 编写 `pyproject.toml`（依赖 rclpy + sensor_msgs + std_msgs）
- [x] **2B.5** 语法验证：3 个 `.py` 文件全部通过 `py_compile`

## 阶段 3：首个 ZC 契约 + 参考实现 ✅

- [x] **3.1** 创建 `capabilities/primitive/camera/rgb_zc.v1.toml`（同 IDL，新 contract id，`topic_out`）
- [x] **3.2** 创建 `examples/zc_demo/zc_camera_provider.py`（Primitive + `ZcPublisher` + 完整生命周期）
- [x] **3.3** 创建 `examples/zc_demo/zc_camera_consumer.py`（独立 consumer，无需 Atlas）
- [x] **3.4** 创建 `examples/zc_demo/README.md`
- [x] **3.5** 语法验证：两个 `.py` 文件通过 `py_compile`

## 阶段 4：AI 代码生成工作流 ✅

> 在 `robonix-codegen` 中添加 `--lang ros2_zc` 目标。分两层：
> **A 层（确定性模板）** 生成 ZC 契约 + provider 骨架；**B 层（AI agent）**
> 读取现有非 ZC provider，自动填充业务逻辑到骨架中。
> 全部代码开发完成；4C 为开发者使用指南。
>
> 注意：本阶段完成的是 Python bridge 生成路径。若 source provider
> 本身订阅已有 ROS2 topic，则生成物只优化 bridge 之后的数据流；
> 完整源头 ZC 优化仍需要改造驱动源码的数据产生/发布点。

### 4A. 确定性模板生成 — `robonix-codegen --lang ros2_zc` ✅

- [x] **4A.1** 实现 `zc_scanner.rs`（扫描 contracts/ 目录，匹配 Image/PointCloud2/LaserScan IDL）
- [x] **4A.2** 实现 `zc_python_gen.rs`（ZC 契约 TOML 生成 + Python provider 骨架模板）
- [x] **4A.3** 在 `mod.rs` + `main.rs` 中注册 `--lang ros2_zc` 目标
- [x] **4A.4** 验证：`cargo build` 零警告，5 个候选全部正确生成，`.py` 全部通过 `py_compile`

### 4B. AI Agent 业务逻辑填充 — `--fill <source_providers_dir>` ✅

- [x] **4B.1** 添加 `--fill` CLI 选项 + `discover_providers()` 扫描源码目录
- [x] **4B.2** `zc_python_gen::generate()` 生成 `fill_manifest.json`（skeleton↔source 映射 + TODO 清单）
- [x] **4B.3** 创建 `.claude/skills/fill-zc-providers/SKILL.md` — 读取 manifest、提取业务逻辑、填入 TODO、标记 `@ai-filled`
- [x] **4B.4** 实际演示：4 个骨架全部填充，3 个 4/4，1 个 3/4（PointCloud2 源仅有 LaserScan），全部通过 `py_compile`
- [x] **4B.5** 依赖：`serde_json`（manifest 序列化）

### 4C. 开发者使用方式（运营任务，非开发任务）

开发者按需使用，每次只生成需要的 contract：

```bash
# 生成单个
robonix-codegen --lang ros2_zc --contracts capabilities/ \
  --filter robonix/primitive/camera/rgb \
  --fill examples/webots/primitives/ -o gen/zc/

# AI 填充（如有匹配源码）
/fill-zc-providers gen/zc/fill_manifest.json
```

输出（以 `--filter robonix/primitive/camera/rgb` 为例）：

```
gen/zc/
├── fill_manifest.json                       # skeleton↔source 映射
├── contracts/camera/rgb_zc.v1.toml          # ZC 契约
└── providers/camera_rgb_zc_provider.py      # provider（骨架或 @ai-filled）
```

## 阶段 5：集成测试

- [ ] **5.1** 在 Webots 示例中部署 ZC 相机（与标准相机并存）
- [ ] **5.2** Executor 集成测试——通过 Atlas 发现 ZC 契约，验证 `ConnectCapability(ROS2_ZC)`
- [ ] **5.3** 端到端延迟测量——对比 ZC vs 标准 ROS2 全链路延迟
- [ ] **5.4** 压力测试——多路 ZC 相机同时运行，验证 shm GC 不泄漏

## 阶段 6：文档

- [ ] **6.1** 在 `docs/src/developer-guide.md` 中添加 ZC 传输章节
- [ ] **6.2** 更新 `pylib/robonix-zc/README.md`（Python API 文档）
- [ ] **6.3** 更新 `tools/codegen/README.md`（`--lang ros2_zc` 使用说明）

## 阶段 7：CI 和发布

- [ ] **7.1** CI 中添加 `robonix_zc` 库编译 + `robonix-codegen --lang ros2_zc` 冒烟测试
- [ ] **7.2** `make check`（`cargo fmt --all -- --check` + clippy）+ `cargo test --workspace --all-targets`
- [ ] **7.3** Pre-commit review（`/pre-commit-review`）
