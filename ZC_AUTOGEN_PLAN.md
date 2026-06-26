# AI 自动化驱动源码 → 零拷贝 Provider 生成工作流

> **文档性质**：实施计划 + TODO 清单  
> **目标**：设计一条端到端 AI 自动化流水线，接受任意驱动源码（Python/C++），理解其数据采集模式，自动生成对应的零拷贝（ZC）Provider 版本。  
> **当前状态**：三阶段 ZC codegen 流水线已就绪（Stage 1–3），但仅限 Python 源码 → Python ZC bridge 路径。本计划将其扩展为多语言、多模式的通用工作流。

---

## 现状分析

### 已有资产清单

| 组件 | 路径 | 功能 | 状态 |
|------|------|------|------|
| ZC 扫描器 | `tools/codegen/src/codegen/zc_scanner.rs` (271行) | 扫描 `capabilities/**/*.toml`，提取含 `.msg` IDL 的契约 → `Vec<ZcCandidate>` | ✅ 完成 |
| ZC Python 生成器 | `tools/codegen/src/codegen/zc_python_gen.rs` (341行) | 渲染 ZC 契约 TOML + Python provider 骨架 + `fill_manifest.json` | ✅ 完成 |
| AI 填充技能 | `.claude/skills/fill-zc-providers/SKILL.md` (162行) | 读 manifest → 匹配 source+skeleton → 填充 4 个 TODO | ✅ 完成（仅 Python） |
| 部署工具 | `tools/deploy-zc/deploy_zc.py` (612行) | 复制契约+provider 到 demo 树，生成 build/start 脚本 | ✅ 完成 |
| ZC 共享库 | `/home/warth/Desktop/build/ros/src/cpp_pubsub/` | `librobonix_zc.so`：ShmImage/ShmPointCloud2 + ZcPublisher/ZcNode | ✅ 完成 |
| pybind11 桥接 | `pylib/robonix-zc/src/_zc_bridge.cpp` (274行) | Python ↔ librobonix_zc.so 桥接 | ✅ 完成 |
| Python ZC 封装 | `pylib/robonix-zc/robonix_zc/zc_publisher.py` | `ZcPublisher.publish(msg)` → shm write + String topic | ✅ 完成 |
| Atlas 注册 | `pylib/robonix-api/robonix_api/atlas_types.py` | `Transport.ROS2_ZC = 4`，`Ros2ZcParams` dataclass | ✅ 完成 |

### 当前流水线的限制

1. **源码语言**：`discover_providers()` (`zc_scanner.rs:187`) 仅扫描 `.py` 文件中 `Primitive(` / `namespace=` 模式 — C++ 驱动完全不可见
2. **输出模板**：`render_provider()` (`zc_python_gen.rs:198`) 只生成 Python 骨架 — 无 C++ 模板
3. **AI 填充**：`SKILL.md` 仅处理 Python `robonix_api` 模式（`create_subscription`、`wait_for_topic`）— 无法解析 C++ SDK 调用
4. **部署**：`deploy_zc.py` 只生成 `python3 -m` 启动脚本 — C++ 二进制需要 CMake 构建
5. **fill_manifest.json 结构**：缺少 `source_language`、`driver_pattern`、`sdk_type` 等元数据字段

### 架构约束（不可变）

- ZC transport 仅支持 `Image` 和 `PointCloud2`（硬编码在 `ZcPublisher.__init__`，第81行）
- Atlas 注册是 Python-only（gRPC client 在 `robonix_api`）
- `librobonix_zc.so` 已支持 C++ 直接链接（`ZcNode`、`ZcPublisher` 模板类在 `zc_pubsub.hpp`）
- C++ driver + Python sidecar 是当前可行的混合部署模式

---

## Phase 1：增强型源码分析引擎

**目标**：让 AI 能够理解 Python 和 C++ 两种语言的驱动源码，提取出结构化的「驱动画像」（Driver Profile）。

**预估工期**：5–7 天

### 1.1 定义 Driver Profile 数据结构

```
文件：tools/codegen/src/codegen/driver_profile.rs（新建）
```

- [ ] **1.1.1** 定义 `DriverProfile` 结构体，包含以下字段：
  ```rust
  pub struct DriverProfile {
      pub language: SourceLanguage,           // Python | Cpp
      pub pattern: DriverPattern,             // RosBridge | HardwareSdk | PollingLoop | TimerCallback
      pub sdk_type: Option<SdkType>,          // V4L2 | OpenCV | RealSense | Velodyne | Ouster | Custom
      pub acquisition: AcquisitionInfo,       // 数据采集模式
      pub lifecycle: LifecycleHooks,          // init/activate/deactivate/shutdown 映射
      pub msg_types: Vec<String>,             // ["Image", "PointCloud2"]
      pub topics: Vec<TopicBinding>,          // 输入/输出 topic 名
      pub env_vars: Vec<EnvVarBinding>,       // 环境变量配置
      pub cleanup_actions: Vec<String>,       // 清理动作列表
      pub source_file: PathBuf,
      pub source_lines: std::ops::Range<usize>,  // 关键代码行范围
  }
  
  pub enum SourceLanguage { Python, Cpp }
  pub enum DriverPattern { 
      RosBridge,        // 订阅 ROS topic → 转发（如 tiago_camera）
      HardwareSdk,      // 直接 SDK 调用（如 Realsense、OpenCV）
      PollingLoop,      // 轮询循环读取
      TimerCallback,    // 定时器回调
  }
  pub enum SdkType { 
      V4L2, OpenCV, RealSense, Velodyne, Ouster, 
      PclROS, Custom(String) 
  }
  ```

- [ ] **1.1.2** 定义 `AcquisitionInfo` 子结构：
  ```rust
  pub struct AcquisitionInfo {
      pub method: AcqMethod,              // Subscription | SdkRead | SdkCallback | PollingTimer
      pub callback_name: Option<String>,  // 回调函数名
      pub thread_model: ThreadModel,      // SingleThread | DedicatedThread | ThreadPool
      pub frame_rate_hint: Option<u32>,   // FPS（如可从源码推断）
  }
  ```

- [ ] **1.1.3** 在 `tools/codegen/Cargo.toml` 中无需额外依赖（已有 `serde`、`regex` 可选添加）

### 1.2 Python 源码分析器

```
文件：tools/codegen/src/codegen/py_analyzer.rs（新建）
```

- [ ] **1.2.1** 实现 `analyze_python_driver(path: &Path) -> Result<Option<DriverProfile>>`：
  - 用正则匹配检测以下模式（不引入 tree-sitter，保持 Rust 侧零外部依赖）：
    - `Primitive(id=` / `namespace=` → provider identity
    - `create_subscription(` → RosBridge pattern
    - `create_publisher(` → 发布模式
    - `import cv2` / `import pyrealsense2` / `from v4l2` → SDK 类型
    - `threading.Thread(target=` → 线程模型
    - `os.environ.get(` → 环境变量
    - `on_init` / `on_activate` / `on_shutdown` → lifecycle hooks
    - `sensor_msgs.msg import Image` / `PointCloud2` → 消息类型
    
- [ ] **1.2.2** 参考现有 `extract_provider_info()` (`zc_scanner.rs:215`)，扩展为完整分析：
  - 复用 `extract_string_arg()` 函数提取 id/namespace
  - 新增 `extract_imports()`、`extract_subscriptions()`、`extract_env_vars()`
  
- [ ] **1.2.3** 模式判定逻辑优先级：
  1. 有 `create_subscription` → `RosBridge`
  2. 有 `import pyrealsense2` / `cv2.VideoCapture` → `HardwareSdk`
  3. 有 `while True:` + `time.sleep` → `PollingLoop`
  4. 有 `create_wall_timer` / `Timer(` → `TimerCallback`

### 1.3 C++ 源码分析器

```
文件：tools/codegen/src/codegen/cpp_analyzer.rs（新建）
```

- [ ] **1.3.1** 实现 `analyze_cpp_driver(path: &Path) -> Result<Option<DriverProfile>>`：
  - 正则匹配检测 C++ 模式：
    - `rclcpp::Node` / `class.*: public rclcpp::Node` → ROS node
    - `create_subscription<` → 订阅模式
    - `create_publisher<` → 发布模式
    - `#include <sensor_msgs/msg/image.hpp>` → 消息类型
    - `#include <opencv2` / `#include <librealsense2` / `#include <velodyne_` → SDK
    - `std::thread` / `boost::thread` → 线程模型
    - `shm_init` / `ZcPublisher` / `ZcNode` → 已含 ZC 代码（跳过）
    - `sensor_msgs::msg::Image` / `sensor_msgs::msg::PointCloud2` → 消息类型
    - `timer_->create_wall_timer` → TimerCallback 模式

- [ ] **1.3.2** 处理 C++ 头文件+源文件配对：
  - 扫描目录时查找 `.hpp` + `.cpp` 对
  - 优先分析 `.cpp` 文件（实现细节更多）
  - 合并 `.hpp` 中的 class 声明信息

- [ ] **1.3.3** 识别 CMakeLists.txt 依赖：
  - 同目录下找 `CMakeLists.txt`
  - 提取 `find_package(` 调用 → 推断 SDK 类型
  - 提取 `target_link_libraries(` → 确认依赖链

### 1.4 统一发现入口

```
修改：tools/codegen/src/codegen/zc_scanner.rs
```

- [ ] **1.4.1** 重构 `discover_providers()` → `discover_drivers()`：
  ```rust
  pub fn discover_drivers(dirs: &[PathBuf]) -> Result<Vec<DriverProfile>> {
      let mut profiles = Vec::new();
      for d in dirs {
          for entry in walk_files(d)? {
              match entry.extension() {
                  "py" => { if let Some(p) = py_analyzer::analyze(&entry)? { profiles.push(p); } }
                  "cpp" | "cc" | "cxx" => { if let Some(p) = cpp_analyzer::analyze(&entry)? { profiles.push(p); } }
                  _ => {}
              }
          }
      }
      Ok(profiles)
  }
  ```

- [ ] **1.4.2** 保留向后兼容：`ProviderInfo` 作为 `DriverProfile` 的投影
  ```rust
  impl From<&DriverProfile> for ProviderInfo { ... }
  ```

### 1.5 fill_manifest.json 扩展

```
修改：tools/codegen/src/codegen/zc_python_gen.rs（FillManifestEntry 结构体）
```

- [ ] **1.5.1** 扩展 `FillManifestEntry`：
  ```rust
  struct FillManifestEntry {
      skeleton: String,
      source: Option<String>,
      contract_id: String,
      namespace: String,
      msg_type: String,
      todos: Vec<&'static str>,
      // ── 新增字段 ──
      source_language: Option<String>,     // "python" | "cpp" | null
      driver_pattern: Option<String>,      // "ros_bridge" | "hardware_sdk" | "polling_loop" | "timer_callback"
      sdk_type: Option<String>,            // "opencv" | "realsense" | "v4l2" | ...
      recommended_deployment: String,      // "python_bridge" | "cpp_direct" | "cpp_sidecar"
      detected_topics: Vec<String>,        // ["/head_front_camera/rgb/image_raw"]
      detected_env_vars: Vec<String>,      // ["TIAGO_RGB_TOPIC"]
  }
  ```

- [ ] **1.5.2** 根据 `DriverProfile` 自动设置 `recommended_deployment`：
  - Python source → `"python_bridge"`（现有路径）
  - C++ source + Image/PointCloud2 → `"cpp_sidecar"`（C++ 做数据，Python sidecar 注册 Atlas）
  - C++ source + 已 link `librobonix_zc` → `"cpp_direct"`（纯 C++ 方案）

---

## Phase 2：多语言模板生成系统

**目标**：扩展 codegen 支持三种部署模式的代码生成。

**预估工期**：7–10 天

### 2.1 Python Bridge 模板增强（现有路径改进）

```
修改：tools/codegen/src/codegen/zc_python_gen.rs → render_provider()
```

- [ ] **2.1.1** 当前模板已经有 4 个 TODO，对于 `RosBridge` 模式已经够用
- [ ] **2.1.2** 增加对 `HardwareSdk` 模式的差异化 TODO 描述：
  - `init` TODO: `"# TODO: initialise hardware SDK (e.g. cv2.VideoCapture, pyrealsense2.pipeline)"`
  - `activate` TODO: `"# TODO: start acquisition loop; on each frame call _pub.publish(msg)"`
  - `deactivate` TODO: `"# TODO: stop acquisition loop / release camera handle"`
  - `shutdown` TODO: `"# TODO: close SDK handles + shutdown_shm()"`

### 2.2 C++ Direct 模板（新增）

```
文件：tools/codegen/src/codegen/zc_cpp_gen.rs（新建）
```

- [ ] **2.2.1** 创建 C++ ZC provider 骨架生成器：
  - 生成 `.hpp` 头文件 + `.cpp` 实现文件 + `CMakeLists.txt` 片段
  - 使用 `ZcNode`（继承 `rclcpp::Node`）而非 `Primitive`（Python-only）
  - 模板核心结构：
    ```cpp
    // @generated by robonix-codegen --lang ros2_zc --target cpp_direct
    #include "zc_pubsub.hpp"
    #include "zc_shm.hpp"
    
    class {ClassName}Node : public ZcNode {
    public:
        {ClassName}Node() : ZcNode("{node_name}") {
            shm_init("{shm_name}", {shm_size});
            publisher_ = create_zc_publisher<{RosMsgT}>("{topic}");
            // TODO: initialize hardware SDK
        }
        
        ~{ClassName}Node() {
            // TODO: close hardware
            shm_shutdown("{shm_name}");
        }
        
    private:
        void timer_callback() {
            auto* data = manager_->Shm{MsgName}GetNew(shm);
            // TODO: fill data from hardware SDK
            publisher_->publish(data);
        }
        
        std::shared_ptr<ZcPublisher> publisher_;
        // TODO: SDK handles
    };
    ```

- [ ] **2.2.2** 生成配套 `CMakeLists.txt` 片段：
  ```cmake
  # @generated by robonix-codegen --lang ros2_zc --target cpp_direct
  find_package(cpp_pubsub REQUIRED)
  add_executable({name}_zc_node src/{name}_zc_node.cpp)
  target_link_libraries({name}_zc_node robonix_zc)
  ```

- [ ] **2.2.3** 注意：C++ direct 模式**不需要 Atlas 注册**（Atlas 是 Python gRPC client），需要在生成的 README 中标注手动注册步骤或配合 sidecar

### 2.3 C++ Sidecar 模板（新增）

```
文件：tools/codegen/src/codegen/zc_sidecar_gen.rs（新建）
```

- [ ] **2.3.1** 生成两个文件：
  - `{name}_zc_data_node.cpp`：C++ 节点，负责数据采集 → ZC publish（不含 Atlas）
  - `{name}_zc_sidecar.py`：Python sidecar，负责 Atlas capability 注册 + lifecycle 代理

- [ ] **2.3.2** Python sidecar 模板：
  ```python
  # @generated by robonix-codegen --lang ros2_zc --target cpp_sidecar
  # Sidecar: registers ZC capability on Atlas while C++ node handles data.
  from robonix_api import Primitive, Ok
  from robonix_api.atlas_types import Transport, Ros2ZcParams
  
  sidecar = Primitive(
      id="{name}_zc_sidecar",
      namespace="{namespace}",
  )
  
  @sidecar.on_init
  def init(cfg: dict) -> Ok | str:
      sidecar.declare_capability(
          contract_id="{zc_contract_id}",
          endpoint="{topic}",
          transport=Transport.ROS2_ZC,
          params=Ros2ZcParams(shm_name="{shm_name}", shm_size={shm_size}),
          description="{label} ZC stream (C++ data node)",
      )
      return Ok()
  
  if __name__ == "__main__":
      sidecar.run()
  ```

- [ ] **2.3.3** C++ data node 模板与 2.2 类似，但：
  - 不调用 `shm_init`/`shm_shutdown`（由 sidecar 或外部管理）
  - 只关注数据采集 → `ShmImageGetNew` → `publish`

### 2.4 CLI 扩展

```
修改：tools/codegen/src/main.rs
```

- [ ] **2.4.1** 添加 `--target` 参数（仅 `--lang ros2_zc` 时有效）：
  ```rust
  #[arg(long = "target", default_value = "auto")]
  target: String,  // "auto" | "python_bridge" | "cpp_direct" | "cpp_sidecar"
  ```

- [ ] **2.4.2** `"auto"` 模式下，根据 `--fill` 发现的 `DriverProfile.language` 自动选择：
  - Python source → `python_bridge`
  - C++ source → `cpp_sidecar`（默认安全选项）
  - 无 source → `python_bridge`（降级为手动填写）

- [ ] **2.4.3** 更新帮助文本，说明三种 target 的适用场景

### 2.5 mod.rs 注册

```
修改：tools/codegen/src/codegen/mod.rs
```

- [ ] **2.5.1** 添加新模块声明：
  ```rust
  pub mod driver_profile;
  pub mod py_analyzer;
  pub mod cpp_analyzer;
  pub mod zc_cpp_gen;
  pub mod zc_sidecar_gen;
  ```

---

## Phase 3：AI 驱动的填充工作流

**目标**：扩展 `/fill-zc-providers` 技能，使其能处理 C++ 源码并适配多种部署模式。

**预估工期**：5–7 天

### 3.1 扩展现有 fill-zc-providers 技能

```
修改：.claude/skills/fill-zc-providers/SKILL.md
```

- [ ] **3.1.1** 新增 `source_language` 分支处理逻辑：
  - 当 `fill_manifest.json` 条目包含 `"source_language": "cpp"` 时：
    - 读取 C++ 源文件而非 Python
    - 识别 `create_subscription<>` / `create_wall_timer` / SDK API 调用
    - 提取 C++ lambda / member function 作为业务逻辑

- [ ] **3.1.2** 新增 C++ 源码分析指令（给 AI agent 的引导）：
  ```markdown
  ### C++ Source Analysis Checklist

  1. Find the main Node class (inherits rclcpp::Node or ZcNode)
  2. Identify data acquisition:
     - `create_subscription<Xxx>(...)` → extract callback body
     - `create_wall_timer(...)` → extract timer callback body  
     - SDK function calls (e.g., `rs2::pipeline::poll_for_frame()`)
  3. Extract configuration: constructor params, launch args, `declare_parameter()`
  4. Identify cleanup: destructor body, `on_shutdown()` equivalent
  5. Check if already links librobonix_zc.so (→ cpp_direct possible)
  ```

- [ ] **3.1.3** 新增 `driver_pattern` 对应填充策略表：

  | Pattern | Python Bridge | C++ Direct | C++ Sidecar |
  |---------|--------------|------------|-------------|
  | RosBridge | ✅ 现有 | N/A | C++ republish + Python register |
  | HardwareSdk | Python SDK wrapper | 直接 SDK→ZC | SDK→ZC + sidecar register |
  | PollingLoop | Thread + polling | Timer callback | Same + sidecar |
  | TimerCallback | rclpy Timer | Wall timer | Same + sidecar |

### 3.2 新增 C++ 专用 Claude Code 技能

```
文件：.claude/skills/fill-zc-cpp/SKILL.md（新建）
```

- [ ] **3.2.1** 专门处理 `fill_manifest.json` 中 `"source_language": "cpp"` 的条目
- [ ] **3.2.2** 核心流程：
  1. 读取 C++ 源文件和生成的骨架 `.cpp`/`.hpp`
  2. 识别 `class XxxNode : public rclcpp::Node` → 提取数据采集逻辑
  3. 将 SDK 调用逻辑迁移到 `ZcNode` 子类中
  4. 替换标准 `rclcpp::Publisher` → `ZcPublisher`（使用 `ShmImageGetNew` 代替 `msg.data = ...`）
  5. 标记 `// @ai-filled — source: xxx.cpp:L42-L78`

- [ ] **3.2.3** C++ 特有的填充规则：
  - `sensor_msgs::msg::Image` → `ShmImage`（通过 `manager_->ShmImageGetNew(shm)`）
  - `sensor_msgs::msg::PointCloud2` → `ShmPointCloud2`
  - `data.resize(N)` 保留（ShmVector 支持 resize）
  - `encoding = "rgb8"` → 字符串赋值改为 `encoding = "rgb8".c_str()`（BIP allocator）
  - 移除所有 DDS 相关的 QoS 设置（ZC 用 best_effort + String topic）

### 3.3 新增 Sidecar 生成技能

```
文件：.claude/skills/generate-zc-sidecar/SKILL.md（新建）
```

- [ ] **3.3.1** 当 `recommended_deployment` = `"cpp_sidecar"` 时触发
- [ ] **3.3.2** 生成 Python sidecar 文件，填入正确的 Atlas 注册参数
- [ ] **3.3.3** 确保 sidecar 和 C++ data node 使用相同的 `SHM_NAME`
- [ ] **3.3.4** 生成配套的 `start.sh`：先启动 C++ node，再启动 Python sidecar

### 3.4 fill_manifest.json 消费逻辑升级

```
修改：.claude/skills/fill-zc-providers/SKILL.md 顶部判断逻辑
```

- [ ] **3.4.1** 读取 manifest 后按 `source_language` + `recommended_deployment` 分派：
  ```
  python + python_bridge → 走现有填充流程（不变）
  cpp + cpp_sidecar      → 调用 /fill-zc-cpp 填 C++ data node + /generate-zc-sidecar 生成 sidecar
  cpp + cpp_direct       → 调用 /fill-zc-cpp 填 C++ provider（含 Atlas 桩）
  null + python_bridge   → 走现有流程（无 source match，留 TODO）
  ```

---

## Phase 4：验证和测试框架

**目标**：建立自动化验证机制，确保生成的 ZC provider 功能正确。

**预估工期**：4–5 天

### 4.1 静态验证层

```
文件：tools/codegen/src/codegen/zc_validator.rs（新建）
```

- [ ] **4.1.1** Python 语法检查：
  - 对所有生成的 `.py` 文件执行 `py_compile`（现有 `cargo build` 后的 CI step）
  - 检查 `import` 语句完整性（每个 import 必须有对应的使用点）

- [ ] **4.1.2** C++ 编译检查：
  - 生成 `CMakeLists.txt` 片段后，尝试 `cmake --configure`（dry-run）
  - 验证 `#include` 路径正确性

- [ ] **4.1.3** SHM_NAME 唯一性检查：
  - 扫描所有生成的 provider，确认 `SHM_NAME` 不重复
  - 逻辑：从 contract_id leaf 提取后缀（现有 `SKILL.md` 的规则）

- [ ] **4.1.4** Lifecycle 完整性检查：
  - 验证 `on_init` / `on_activate` / `on_deactivate` / `on_shutdown` 四个 hook 均存在
  - 验证 `shutdown()` 中包含 `shutdown_shm()` 调用
  - 验证 `activate()` 中创建的 subscription/timer 在 `deactivate()` 或 `shutdown()` 中被销毁

### 4.2 运行时集成测试

```
文件：tools/codegen/tests/zc_integration_test.rs（新建）
```

- [ ] **4.2.1** Mock ZC provider 冒烟测试：
  - 用 `zc_camera_provider.py`（已有的 mock provider）验证 ZC pipeline 可用
  - 启动 Atlas → boot → provider → consumer → 验证帧到达

- [ ] **4.2.2** 性能基准测试脚本：
  ```
  文件：examples/zc_demo/zc_benchmark.py
  ```
  - 同时走标准 ROS2 和 ZC 两条路径
  - 测量指标：吞吐量（msg/s）、P50/P99 延迟、CPU 占用、内存峰值
  - 对比报告输出到 stdout

- [ ] **4.2.3** Atlas 注册验证：
  - 启动 generated provider → 查询 Atlas → 验证 `ConnectCapability(ROS2_ZC)` 返回正确的 `Ros2ZcParams`

### 4.3 AI 填充质量评估

- [ ] **4.3.1** 定义「填充覆盖率」指标：
  - `filled_todos / total_todos` × 100%
  - 目标：RosBridge pattern ≥ 90%，HardwareSdk ≥ 70%

- [ ] **4.3.2** 在 `fill_manifest.json` 中添加 `fill_result` 字段（AI 填充后回填）：
  ```json
  "fill_result": {
      "filled": 4,
      "total": 4,
      "unfilled_todos": [],
      "warnings": []
  }
  ```

---

## Phase 5：工作流编排与一键命令

**目标**：将所有步骤串联成单条命令可执行的端到端工作流。

**预估工期**：3–4 天

### 5.1 一键生成脚本

```
文件：scripts/gen_zc_provider.sh（新建）
```

- [ ] **5.1.1** 顶层编排脚本，接收参数后顺序执行：
  ```bash
  #!/usr/bin/env bash
  # Usage: scripts/gen_zc_provider.sh \
  #   --contracts capabilities/ \
  #   --filter robonix/primitive/camera/rgb \
  #   --fill examples/webots/primitives/ \
  #   --fill ~/my_cpp_drivers/ \
  #   --target auto \
  #   -o gen/zc/
  
  set -euo pipefail
  
  # Step 1: codegen（确定性模板生成）
  cargo run -p robonix-codegen -- \
    --lang ros2_zc \
    --contracts "$CONTRACTS_DIR" \
    --filter "$FILTER" \
    --fill "${FILL_DIRS[@]}" \
    --target "$TARGET" \
    -o "$OUT_DIR"
  
  # Step 2: AI fill（Claude Code skill invocation）
  echo "[gen_zc] Running AI fill via Claude Code..."
  claude --skill fill-zc-providers "$OUT_DIR/fill_manifest.json"
  
  # Step 3: validate
  echo "[gen_zc] Validating generated providers..."
  python scripts/validate_zc_output.py "$OUT_DIR"
  
  # Step 4: deploy（可选）
  if [ "$DEPLOY" = "true" ]; then
    python tools/deploy-zc/deploy_zc.py "$OUT_DIR" --all --pkg "$PKG_NAME"
  fi
  ```

- [ ] **5.1.2** 错误处理和回退：
  - codegen 失败 → 报错退出（模板问题必须修复）
  - AI fill 部分失败 → 继续，打印未填充的 TODO 列表
  - validate 失败 → 警告但不阻断（非阻塞式质量门控）

### 5.2 Claude Code 自定义 Slash Command

```
文件：.claude/skills/gen-zc-workflow/SKILL.md（新建）
```

- [ ] **5.2.1** 创建高层编排 skill，整合所有步骤：
  ```markdown
  ---
  name: gen-zc-workflow
  description: End-to-end ZC provider generation from driver source code.
  ---
  
  # gen-zc-workflow
  
  Accepts a driver source directory and a contract filter, runs the full
  ZC generation pipeline: codegen → AI fill → validate → deploy.
  
  ## Arguments
  - `--source <dir>`: driver source code directory (Python or C++)
  - `--filter <contract_id>`: which contract(s) to generate ZC for
  - `--deploy`: optionally deploy into the demo tree
  
  ## Steps
  1. Run `robonix-codegen --lang ros2_zc --fill <source> --filter <id> -o gen/zc/<id>/`
  2. If `fill_manifest.json` has source matches → invoke /fill-zc-providers
  3. If any entries have `source_language: cpp` → invoke /fill-zc-cpp
  4. Validate all generated files
  5. Print summary report
  ```

### 5.3 文档自动生成

- [ ] **5.3.1** 每次成功生成后，在 `gen/zc/<id>/README.md` 中输出：
  - 契约 ID + 消息类型
  - 部署模式（python_bridge / cpp_direct / cpp_sidecar）
  - 环境变量清单
  - 启动命令示例
  - AI 填充覆盖率统计

### 5.4 deploy_zc.py 扩展

```
修改：tools/deploy-zc/deploy_zc.py
```

- [ ] **5.4.1** 支持 C++ provider 部署：
  - 复制 `.cpp` / `.hpp` 文件到 colcon workspace
  - 生成 `colcon_build.sh`（替代纯 Python 的 `build.sh`）
  - `start.sh` 中区分 Python `exec python3 -m` vs C++ `exec ros2 run`

- [ ] **5.4.2** 支持 sidecar 双进程启动：
  - `start.sh` 中先启动 C++ data node，等待 shm_init，再启动 Python sidecar

---

## 实施路线总览

| Phase | 内容 | 工期 | 前置依赖 | 交付物 |
|-------|------|------|----------|--------|
| **Phase 1** | 增强型源码分析 | 5–7天 | 无 | `driver_profile.rs`, `py_analyzer.rs`, `cpp_analyzer.rs` |
| **Phase 2** | 多语言模板生成 | 7–10天 | Phase 1 | `zc_cpp_gen.rs`, `zc_sidecar_gen.rs`, CLI `--target` |
| **Phase 3** | AI 填充工作流 | 5–7天 | Phase 2 | 扩展 `SKILL.md`, 新增 `fill-zc-cpp` skill |
| **Phase 4** | 验证和测试 | 4–5天 | Phase 3 | `zc_validator.rs`, benchmark 脚本, CI integration |
| **Phase 5** | 工作流编排 | 3–4天 | Phase 4 | `gen_zc_provider.sh`, `gen-zc-workflow` skill |
| **总计** | | **24–33天** | | |

### 关键里程碑

```
Week 1-2:  Phase 1 完成 → 可以分析 Python+C++ 驱动源码
Week 2-3:  Phase 2 完成 → 可以生成三种部署模式的骨架
Week 3-4:  Phase 3 完成 → AI 可以自动填充 C++ 和 Python 驱动
Week 4-5:  Phase 4 完成 → 自动化验证 + 性能基准
Week 5:    Phase 5 完成 → 一键命令完成全流程
```

---

## 具体实施 TODO（按优先级排序）

### 🔴 P0 — 必须首先完成

- [ ] 创建 `tools/codegen/src/codegen/driver_profile.rs`，定义 `DriverProfile` / `SourceLanguage` / `DriverPattern` / `SdkType` 等核心枚举和结构体
- [ ] 实现 `py_analyzer.rs`，覆盖现有 tiago_camera/tiago_lidar/tiago_chassis 三个驱动的准确识别
- [ ] 扩展 `FillManifestEntry`，添加 `source_language` / `driver_pattern` / `recommended_deployment` 字段
- [ ] 验证向后兼容：现有 `--fill examples/webots/primitives/` 调用仍然工作

### 🟡 P1 — 核心价值

- [ ] 实现 `cpp_analyzer.rs`，至少能识别 `publisher_example.cpp`（`/home/warth/Desktop/build/ros/src/cpp_pubsub/src/publisher_example.cpp`）的模式
- [ ] 创建 `zc_cpp_gen.rs`，生成可用的 C++ ZC node 骨架
- [ ] 创建 `zc_sidecar_gen.rs`，生成 Python sidecar 模板
- [ ] 添加 `--target` CLI 参数 + auto 选择逻辑

### 🟢 P2 — 完善体验

- [ ] 扩展 `fill-zc-providers` SKILL.md，支持 C++ source 分支
- [ ] 创建 `fill-zc-cpp` skill
- [ ] 创建 `gen-zc-workflow` 编排 skill
- [ ] 编写 `scripts/gen_zc_provider.sh` 顶层脚本

### ⚪ P3 — 质量保障

- [ ] 实现 `zc_validator.rs`（静态检查）
- [ ] 集成测试：Webots ZC 相机 + lidar 端到端
- [ ] benchmark 脚本实现
- [ ] CI pipeline 添加 `--lang ros2_zc` 冒烟测试

---

## 风险与缓解

| 风险 | 影响 | 概率 | 缓解措施 |
|------|------|------|----------|
| C++ 源码格式多样性大，正则匹配不准 | 高 | 中 | 限定支持的 coding style（参照现有 `publisher_example.cpp`），对无法识别的发出 WARNING 而非静默跳过 |
| C++ direct 模式缺少 Atlas 注册能力 | 中 | 高 | 默认推荐 `cpp_sidecar` 模式，`cpp_direct` 标注为 advanced |
| pybind11 桥接不支持新的 Shm 消息类型 | 低 | 低 | ZC transport 限定 Image+PointCloud2，不新增类型 |
| AI 填充质量不稳定 | 中 | 中 | 强制 `@ai-filled` provenance marker + 人工 review 步骤 |
| C++ CMake 构建环境差异 | 中 | 中 | `deploy_zc.py` 只做文件复制，实际编译留给 colcon workspace |

---

## 复用的现有代码索引

| 复用点 | 来源文件 | 行号 | 用途 |
|--------|----------|------|------|
| `extract_string_arg()` | `zc_scanner.rs:226` | 提取 Python `id="..."` 值 | `py_analyzer.rs` 可直接复用 |
| `extract_provider_info()` | `zc_scanner.rs:215` | Provider identity 提取 | 扩展为 `extract_driver_profile()` |
| `walk_for_providers()` | `zc_scanner.rs:197` | 目录递归遍历 | 扩展为 `walk_for_drivers()` 支持 .cpp |
| `parse_msg_info()` | `zc_scanner.rs:145` | IDL 路径 → MsgInfo | 新模板同样需要 |
| `render_contract_toml()` | `zc_python_gen.rs:162` | ZC 契约 TOML 渲染 | C++/sidecar 模式共用同一契约 |
| `FillManifestEntry` | `zc_python_gen.rs:14` | manifest JSON 结构 | 扩展字段 |
| `provider_id()` / `topic_name()` | `zc_python_gen.rs:317,331` | 命名约定 | C++ node name 遵循同一规则 |
| `ZcPublisher` type check | `zc_publisher.py:81` | `msg_type in (Image, PointCloud2)` | C++ 模板特化同理 |
| `SHM_NAME` 唯一性规则 | `SKILL.md:86` | contract leaf suffix | C++ 代码也需遵守 |
| `_deploy_contract()` | `deploy_zc.py:176` | 契约部署 | C++/sidecar 共用 |
| `_render_start_sh()` | `deploy_zc.py:305` | 启动脚本生成 | 需扩展支持 C++ binary |
