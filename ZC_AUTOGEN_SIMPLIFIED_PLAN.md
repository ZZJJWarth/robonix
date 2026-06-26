# ZC Provider 自动生成简化计划

> 文档性质：简化实施计划  
> 目标：把现有 `ros2_zc` codegen 扩展成可靠的半自动生成流程。  
> 原则：确定性生成优先，AI/人工只填业务逻辑；先支持可验证的最小闭环，不承诺自动理解任意驱动源码。

---

## 目标边界

本计划只支持两条正式路径：

1. **Python bridge**
   - 输入：现有 Python primitive/service/skill provider。
   - 输出：Python ZC provider skeleton。
   - Atlas 注册：由 Python provider 完成。
   - 状态：沿用现有路径，做小幅增强。

2. **C++ sidecar**
   - 输入：C++ ROS2 驱动源码。
   - 输出：C++ data node skeleton + Python sidecar skeleton。
   - Atlas 注册：由 Python sidecar 完成。
   - 共享内存生命周期：由 Python sidecar 负责 `init_shm()` / `shutdown_shm()`，C++ data node 只负责写入和发布。

暂不做：

- 不做正式 `cpp_direct` 模式。
- 不承诺自动迁移任意 SDK 逻辑。
- 不把 Claude skill 作为稳定 CLI 自动化步骤。
- 不支持 `Image` / `PointCloud2` 之外的 ZC 消息类型。
- 不修改 wire format、contract TOML schema、`pylib/robonix-api/` public surface。

---

## Phase 0：收紧现有 ZC 生成边界

目标：先保证 codegen 不生成当前 ZC 运行时不支持的 provider。

实施项：

- [ ] 在 `ros2_zc` codegen 中明确只允许 `sensor_msgs/Image` 和 `sensor_msgs/PointCloud2`。
- [ ] 对其他 `.msg` contract 输出清晰 warning，并跳过 provider skeleton 生成。
- [ ] 把 `SHM_NAME` 唯一性从 AI fill 前移到 codegen：按 contract leaf 生成，例如 `robonix_zc_rgb`、`robonix_zc_lidar3d`。
- [ ] 保持现有 `--fill examples/webots/primitives/` 行为兼容。

验收：

- 现有 Python ZC provider 生成结果仍可用。
- 同时生成多个 ZC provider 时不会共享同一个默认 `SHM_NAME`。
- 不支持的消息类型不会被静默生成成不可运行 provider。

---

## Phase 1：最小 source profile

目标：只提取后续生成必须用到的 metadata，不做复杂源码理解。

新增内部结构建议：

```rust
pub struct SourceMatch {
    pub source_path: PathBuf,
    pub source_language: SourceLanguage,
    pub provider_id: Option<String>,
    pub namespace: Option<String>,
    pub msg_types: Vec<String>,
    pub detected_topics: Vec<String>,
    pub detected_env_vars: Vec<String>,
    pub recommended_deployment: RecommendedDeployment,
    pub warnings: Vec<String>,
}

pub enum SourceLanguage {
    Python,
    Cpp,
}

pub enum RecommendedDeployment {
    PythonBridge,
    CppSidecar,
}
```

Python 识别范围：

- `Primitive(` / `Service(` / `Skill(`
- `id=` / `namespace=`
- `create_subscription(`
- `sensor_msgs.msg import Image` / `PointCloud2`
- `os.environ.get(`

C++ 识别范围：

- `.cpp` / `.cc` / `.cxx` / `.hpp`
- `rclcpp::Node`
- `create_subscription<` / `create_publisher<`
- `sensor_msgs::msg::Image` / `PointCloud2`
- `#include <sensor_msgs/msg/image.hpp>` / `point_cloud2.hpp`

推荐规则：

- Python source -> `python_bridge`
- C++ source -> `cpp_sidecar`
- 无 source -> `python_bridge`，但 manifest 写 warning

验收：

- Python source 可以产出 `source_language = "python"`。
- C++ source 可以产出 `source_language = "cpp"`。
- 识别失败时不报错中断，manifest 中保留 warning。

---

## Phase 2：增强 fill_manifest.json

目标：让后续填充、部署、review 有足够上下文。

在现有 manifest entry 上增加字段：

```json
{
  "source_language": "python",
  "recommended_deployment": "python_bridge",
  "detected_topics": [],
  "detected_env_vars": [],
  "warnings": []
}
```

字段要求：

- `source_language`: `"python"` / `"cpp"` / `null`
- `recommended_deployment`: `"python_bridge"` / `"cpp_sidecar"`
- `detected_topics`: 仅放源码中明确识别到的 topic
- `detected_env_vars`: 仅放源码中明确识别到的 env var
- `warnings`: 记录不确定点，例如消息类型缺失、多个 source 匹配、source 和 contract 类型不一致

验收：

- 旧版 fill skill 仍能读取原有字段。
- 新字段缺失时部署工具不崩溃。
- manifest 足以解释为什么某个 source 被推荐成 `python_bridge` 或 `cpp_sidecar`。

---

## Phase 3：新增 C++ sidecar skeleton

目标：为 C++ 驱动提供可 review、可手工填充的 ZC skeleton。

生成文件：

- `{name}_zc_data_node.cpp`
- `{name}_zc_sidecar.py`
- `README.md` 或构建说明片段

Python sidecar 负责：

- `init_shm(SHM_NAME, SHM_SIZE)`
- `declare_capability(...)`
- `shutdown_shm(SHM_NAME)`
- 运行 `Primitive.run()`

C++ data node 负责：

- 创建 ROS2 node
- 创建 ZC publisher
- 在 TODO 区填入采集逻辑
- 写入 `ShmImage` 或 `ShmPointCloud2`
- 发布 object name

明确不做：

- 不自动改写原始 C++ driver。
- 不自动迁移 SDK 初始化和清理逻辑。
- 不生成正式 `cpp_direct` provider。

验收：

- 生成的 sidecar Python 文件可以通过 `py_compile`。
- C++ skeleton 有明确 TODO 和 message-type-specific 填充位置。
- sidecar 和 C++ data node 使用同一个 `SHM_NAME`。

---

## Phase 4：最小验证

目标：先验证生成物不会明显不可用。

实现一个轻量 validator，可以作为独立脚本或 codegen 子步骤：

- [ ] 检查所有生成的 `.py` 文件可 `py_compile`。
- [ ] 检查 manifest 必要字段存在。
- [ ] 检查每个 provider 的 `SHM_NAME` 唯一。
- [ ] 检查 unsupported msg type 已跳过或带 warning。
- [ ] 检查 Python provider / sidecar 包含 Atlas 注册。

运行时 smoke test 先保持小范围：

- 用现有 mock ZC camera provider 验证 `ROS2_ZC` 注册和消费。
- C++ sidecar 阶段先验证文件生成和 sidecar 注册，不强求真实硬件数据。

---

## Phase 5：部署工具小步扩展

目标：保留现有 Python 部署稳定性，同时支持 sidecar 文件复制和启动脚本生成。

`tools/deploy-zc/deploy_zc.py` 增强：

- [ ] 继续支持现有 Python provider 部署。
- [ ] 当 `recommended_deployment = "cpp_sidecar"`：
  - 复制 sidecar `.py`。
  - 复制 C++ data node `.cpp`。
  - 生成或提示 C++ 构建步骤。
  - `start.sh` 先启动 C++ data node，再启动 Python sidecar。
- [ ] 对无法自动构建的 C++ 项目打印 warning，不静默假设成功。

暂不做：

- 不自动生成完整 colcon package。
- 不自动调用 Claude skill。
- 不把 benchmark 放进部署流程。

---

## 优先级

### P0

- [ ] 限定 ZC 消息类型为 `Image` / `PointCloud2`。
- [ ] codegen 生成唯一 `SHM_NAME`。
- [ ] manifest 增加 `source_language`、`recommended_deployment`、`warnings`。
- [ ] 保证现有 Python ZC 生成不回归。

### P1

- [ ] 最小 Python source profile。
- [ ] 最小 C++ source profile。
- [ ] C++ sidecar skeleton。
- [ ] sidecar 负责 Atlas 注册和共享内存生命周期。

### P2

- [ ] 轻量 validator。
- [ ] `deploy_zc.py` 支持 `cpp_sidecar` 文件复制和双进程启动。
- [ ] 更新 `tools/codegen/README.md`。

### P3

- [ ] AI fill skill 支持 C++ skeleton。
- [ ] benchmark。
- [ ] 更复杂 SDK 识别。
- [ ] 实验性 `cpp_direct`。

---

## 推荐实施顺序

1. 先改 Python 现有路径：消息类型过滤、唯一 `SHM_NAME`、manifest 扩展。
2. 再做 source profile，但只输出 metadata 和 warning。
3. 然后生成 `cpp_sidecar` skeleton。
4. 最后扩展部署和验证。

这样每一步都可以独立验收，也不会把不稳定的 AI 自动迁移放进核心路径。
