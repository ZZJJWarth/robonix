# Robonix 零拷贝驱动源头改造 — 待办清单

> 目标：把 ZC 优化从“ROS topic bridge 后再写共享内存”推进到“驱动产生数据时直接写共享内存”。  
> 核心判断：现有 Python bridge 路径只能优化消费者侧，不能消除驱动发布标准 ROS2 大消息时的序列化、DDS、拷贝成本。真正的 ZC 优化必须修改驱动源码的数据发布点。

---

## 当前结论

现有 `robonix-codegen --lang ros2_zc` 已经能生成 Python ZC provider skeleton，并通过 AI 从已有 provider 中填充逻辑。但当前自动填充出来的典型路径是：

```text
existing ROS node -> ROS Image/PointCloud2 topic -> Python ZC bridge -> shared memory -> object-name topic
```

这条路径的问题是：数据已经经过标准 ROS2 topic 发布和反序列化，ZC 只发生在后半段。它不是完整的驱动源头零拷贝优化。

目标路径应该是：

```text
driver SDK/callback/read loop -> ShmImage/ShmPointCloud2 -> object-name topic
                                    |
                                    v
                              Python sidecar registers ROS2_ZC capability
```

---

## 阶段 0：已有基础设施盘点 ✅

### 0A. Atlas / API 支持 ✅

- [x] **0A.1** `system/atlas/proto/atlas.proto` 已添加 `TRANSPORT_ROS2_ZC = 4`
- [x] **0A.2** proto 已包含 `Ros2ZcParams`
- [x] **0A.3** `pylib/robonix-api/robonix_api/atlas_types.py` 已添加 `Transport.ROS2_ZC` 和 `Ros2ZcParams`
- [x] **0A.4** `pylib/robonix-api/robonix_api/atlas.py` 已支持 ROS2_ZC params 映射
- [x] **0A.5** `pylib/robonix-api/robonix_api/capability.py` 已允许 `topic_out` / `topic_in` 使用 `ros2_zc`

### 0B. ZC 共享库和 Python wrapper ✅

- [x] **0B.1** 外部 ROS 项目已产出 `librobonix_zc.so`
- [x] **0B.2** C++ 侧已有 `ShmImage` / `ShmPointCloud2`
- [x] **0B.3** C++ 侧已有 `ZcPublisher` / `ZcNode`
- [x] **0B.4** `pylib/robonix-zc/src/_zc_bridge.cpp` 已暴露 `init_shm` / `shutdown_shm` / `create_image` / `create_pointcloud2` / `take_image` / `take_pointcloud2`
- [x] **0B.5** `pylib/robonix-zc/robonix_zc/zc_publisher.py` 已实现 Python `ZcPublisher`
- [x] **0B.6** `pylib/robonix-zc/robonix_zc/zc_subscriber.py` 已实现 Python `ZcSubscriber`

### 0C. ZC contract 和演示 ✅

- [x] **0C.1** 已有 `capabilities/primitive/camera/rgb_zc.v1.toml`
- [x] **0C.2** 已有 `capabilities/primitive/camera/depth_zc.v1.toml`
- [x] **0C.3** 已有 `capabilities/primitive/lidar/lidar3d_zc.v1.toml`
- [x] **0C.4** 已有 `examples/zc_demo/zc_camera_provider.py`
- [x] **0C.5** 已有 `examples/zc_demo/zc_camera_consumer.py`
- [x] **0C.6** Webots 示例中已有 `examples/webots/primitives/camera_zc/`

### 0D. 当前 AI 自动生成路径 ✅ 但需要调整

- [x] **0D.1** `tools/codegen/src/codegen/zc_scanner.rs` 已能扫描 contract 并生成 ZC candidate
- [x] **0D.2** `tools/codegen/src/codegen/zc_python_gen.rs` 已能生成 ZC contract TOML + Python provider skeleton
- [x] **0D.3** `tools/codegen/src/main.rs` 已注册 `--lang ros2_zc`
- [x] **0D.4** `--fill` 已能生成 `fill_manifest.json`
- [x] **0D.5** `.claude/skills/fill-zc-providers/SKILL.md` 已能填充 Python bridge skeleton
- [x] **0D.6** 已在 `.claude/skills/fill-zc-providers/SKILL.md` 明确标注：当前 `fill-zc-providers` 填充的是 Python bridge；当 source 本身订阅 ROS topic 时，它不是驱动源头 ZC 优化

---

## 阶段 1：停止把 bridge 当作核心优化路径

目标：保留 bridge 作为兼容/演示路径，但不再把它描述为完整 ZC 优化。

- [x] **1.1** 更新 `tools/codegen/README.md`：说明 Python bridge 只能优化 bridge 之后的数据流
- [x] **1.2** 更新 `.claude/skills/fill-zc-providers/SKILL.md`：当 source 是 ROS subscription provider 时，输出 warning：这是 bridge，不是驱动源头优化
- [x] **1.3** 更新 `ZC_TODO.md` 或补充文档：阶段 4 已完成的是 bridge 生成，不等于驱动源码改造
- [x] **1.4** 在 `fill_manifest.json` 中增加 `generation_mode` 字段，允许值先只用：
  - `python_bridge`
  - `driver_source_patch`
- [x] **1.5** 对 `python_bridge` 生成物保留现有行为，但在文件头写明用途：bridge / compatibility / demo

验收：

- 任何新生成的 Python bridge 文件都不会让 reviewer 误以为它已经消除了驱动侧 ROS2 发布成本。
- 现有 bridge demo 不回归。

---

## 阶段 2：新增 sidecar 模板

目标：生成只负责系统集成的 Python sidecar，不搬运数据。

sidecar 职责：

- 创建共享内存：`init_shm(SHM_NAME, SHM_SIZE)`
- 向 Atlas 注册 ZC capability：`Transport.ROS2_ZC` + `Ros2ZcParams`
- 暴露 lifecycle：`on_init` / `on_activate` / `on_deactivate` / `on_shutdown`
- 关闭共享内存：`shutdown_shm(SHM_NAME)`
- 可选启动或监督已经改造过的驱动进程

sidecar 明确禁止：

- 不订阅原始 ROS Image / PointCloud2 topic
- 不调用 Python `ZcPublisher.publish(msg)` 转发已有 ROS msg
- 不承担数据采集逻辑

实施项：

- [x] **2.1** 在 codegen 中新增 sidecar 渲染函数，输出 `{name}_zc_sidecar.py`
- [x] **2.2** sidecar 生成唯一 `SHM_NAME`，例如 `robonix_zc_rgb`
- [x] **2.3** sidecar 生成 `TOPIC`，该 topic 只发布 object name
- [x] **2.4** sidecar 生成 `declare_capability(...)`，使用对应 `_zc` contract id
- [x] **2.5** sidecar 文件头写明：数据必须由改造后的驱动写入共享内存
- [x] **2.6** 生成 sidecar README 片段，说明它必须和被改造驱动使用同一 `SHM_NAME` / `TOPIC`

验收：

- sidecar 可通过 `py_compile`
- sidecar 只注册 capability 和管理 shm，不包含 ROS topic bridge 逻辑
- 通过 Atlas 可查询到对应 `ROS2_ZC` capability 和 `Ros2ZcParams`

---

## 阶段 3：生成驱动源码修改清单

目标：codegen 不直接改驱动，而是输出一份给 AI skill / reviewer 使用的修改清单。

新增文件建议：

```text
zc_driver_patch_manifest.json
```

最小字段：

```json
{
  "contract_id": "robonix/primitive/camera/rgb_zc",
  "msg_type": "Image",
  "source_dir": "path/to/driver",
  "candidate_files": [],
  "shm_name": "robonix_zc_rgb",
  "shm_size": 67108864,
  "topic": "/camera/rgb_zc",
  "sidecar": "camera_rgb_zc_sidecar.py",
  "rules": {
    "must_patch_driver_publish_point": true,
    "forbid_subscribe_and_republish": true,
    "keep_legacy_ros_publish_optional": true
  },
  "warnings": []
}
```

实施项：

- [x] **3.1** 扩展 `--lang ros2_zc`：当用户传入 driver source 目录时，生成 `zc_driver_patch_manifest.json`
- [x] **3.2** 扫描候选 C++ 文件：`.cpp` / `.cc` / `.cxx` / `.hpp`
- [x] **3.3** 候选识别只做最小规则：
  - `sensor_msgs::msg::Image`
  - `sensor_msgs::msg::PointCloud2`
  - `create_publisher<`
  - `publisher_->publish`
  - camera / lidar SDK callback 或 read loop 的明显函数名
- [x] **3.4** 不确定时写入 `warnings`，不静默跳过
- [x] **3.5** manifest 明确禁止“订阅原 ROS topic 再转发 ZC”

验收：

- 给定 C++ 驱动目录时，可以生成 sidecar + driver patch manifest
- manifest 能告诉 AI/reviewer：应该改哪些候选文件、目标 topic/shm 是什么、禁止做什么

---

## 阶段 4：新增驱动源码修改 skill

目标：创建一个类似现有 `.claude/skills/fill-zc-providers` 的 skill，用于读取驱动源码并添加 ZC 写入逻辑。

新增文件：

```text
.claude/skills/patch-zc-driver/SKILL.md
```

skill 输入：

- `zc_driver_patch_manifest.json`
- 驱动源码目录
- C++ ZC API 参考：`zc_pubsub.hpp` / `zc_shm.hpp`
- sidecar 文件

skill 必须做：

- [x] **4.1** 读取 manifest 和候选源码文件
- [x] **4.2** 找到真实数据产生点：
  - SDK callback
  - frame read loop
  - lidar packet conversion
  - `sensor_msgs::msg::Image` / `PointCloud2` 填充完成但尚未 publish 的位置
- [x] **4.3** 在驱动中接入 ZC C++ API：
  - 初始化或连接共享内存
  - 创建 ZC publisher
  - 写入 `ShmImage` 或 `ShmPointCloud2`
  - 发布 object name topic
- [x] **4.4** 修改构建文件：
  - `CMakeLists.txt`
  - 必要时修改 `package.xml`
  - 链接 `robonix_zc`
- [x] **4.5** 保留原 ROS publisher 作为可选 fallback，除非用户明确要求移除
- [x] **4.6** 每个修改块添加来源标记：
  - `// @ai-filled source: <file>:Lx-Ly`
- [x] **4.7** 输出修改摘要、未确认点、验证命令

skill 禁止做：

- [x] **4.8** 禁止新增订阅原始 ROS topic 再转发到 ZC 的逻辑
- [x] **4.9** 禁止修改 wire format、contract TOML schema、`pylib/robonix-api/` public surface
- [x] **4.10** 禁止无依据重写驱动生命周期
- [x] **4.11** 禁止删除原驱动逻辑，除非用户明确确认

验收：

- skill 能对一个简单 C++ Image publisher demo 生成可 review 的 patch
- patch 后源码中出现共享内存写入点，而不是 ROS subscription bridge
- patch 后构建文件包含 ZC 库依赖

---

## 阶段 5：C++ 驱动接入 API 样例

目标：给 AI skill 和人工 reviewer 一个最小正确参考，避免每次临场猜 API。

新增文件建议：

```text
examples/zc_driver_patch/
├── README.md
├── image_driver_before.cpp
├── image_driver_after.cpp
├── pointcloud_driver_before.cpp
└── pointcloud_driver_after.cpp
```

实施项：

- [ ] **5.1** 编写 `Image` 驱动改造前后样例
- [ ] **5.2** 编写 `PointCloud2` 驱动改造前后样例
- [ ] **5.3** 样例展示如何使用同一 `SHM_NAME` / `TOPIC`
- [ ] **5.4** 样例展示 fallback：保留或关闭原 ROS publisher
- [ ] **5.5** 样例展示必要 CMake 修改

验收：

- AI skill 可引用样例作为改造规则
- reviewer 可以用样例判断 patch 是否把 ZC 放到了驱动源头

---

## 阶段 6：部署工具调整

目标：部署的不再是 Python bridge provider，而是 sidecar + 已改造驱动。

需要修改：

- `tools/deploy-zc/deploy_zc.py`
- Webots 示例启动脚本
- package manifest 生成逻辑

实施项：

- [ ] **6.1** `deploy_zc.py` 支持部署 sidecar 文件
- [ ] **6.2** `deploy_zc.py` 支持复制 `zc_driver_patch_manifest.json`
- [ ] **6.3** `start.sh` 支持先启动改造后的驱动，再启动 sidecar，或由 sidecar 监督驱动
- [ ] **6.4** 对未完成驱动 patch 的部署打印 blocking warning
- [ ] **6.5** 保留 Python bridge 部署路径，但标注为 bridge/demo

验收：

- 未改造驱动时，部署工具不会假装 ZC 源头优化已经完成
- 改造驱动 + sidecar 可以作为一个 primitive package 启动

---

## 阶段 7：端到端验证

目标：证明优化发生在驱动源头，而不是 bridge 后半段。

必须验证：

- [ ] **7.1** Atlas 可发现 `_zc` capability
- [ ] **7.2** `ConnectCapability(ROS2_ZC)` 返回正确 `Ros2ZcParams`
- [ ] **7.3** 消费端通过 object-name topic 读取共享内存数据
- [ ] **7.4** 改造后的驱动源码中没有新增“订阅原始 Image/PointCloud2 topic 再转发”的路径
- [ ] **7.5** 标准 ROS publisher 可以作为 fallback 关闭，关闭后 ZC 消费仍能收到数据
- [ ] **7.6** 对比 bridge 路径和驱动源头路径的延迟 / CPU / 内存

推荐测试顺序：

1. 最小 C++ mock Image driver
2. Webots camera RGB
3. Webots camera depth
4. Webots lidar3d / PointCloud2

---

## 阶段 8：文档和边界说明

- [ ] **8.1** 更新 `ZC_TODO.md`：把已完成 bridge 工作和待完成驱动源头工作区分开
- [ ] **8.2** 更新 `tools/codegen/README.md`：说明 `ros2_zc` 的两种生成物：bridge skeleton / sidecar + driver patch manifest
- [ ] **8.3** 更新 `pylib/robonix-zc/README.md`：补充 C++ 驱动源头写入共享内存的推荐模式
- [ ] **8.4** 补充 reviewer checklist：
  - 是否改到了真实数据产生点
  - 是否禁止了 subscribe-and-republish
  - sidecar 是否只注册和管理 shm
  - driver 和 sidecar 是否使用相同 `SHM_NAME` / `TOPIC`

---

## P0 优先级

- [ ] **P0.1** 新增 sidecar 模板，不包含 bridge 逻辑
- [ ] **P0.2** 生成 `zc_driver_patch_manifest.json`
- [ ] **P0.3** 新增 `.claude/skills/patch-zc-driver/SKILL.md`
- [ ] **P0.4** 写一个最小 C++ Image driver 改造样例
- [ ] **P0.5** 更新文档，明确当前 Python bridge 不是完整 ZC 优化

## P1 优先级

- [ ] **P1.1** codegen 支持 sidecar + patch manifest 输出
- [ ] **P1.2** skill 支持 C++ Image driver patch
- [ ] **P1.3** CMake/package.xml patch 规则稳定下来
- [ ] **P1.4** mock Image driver 端到端验证通过

## P2 优先级

- [ ] **P2.1** 支持 PointCloud2 driver patch
- [ ] **P2.2** Webots camera RGB/depth 源头改造
- [ ] **P2.3** Webots lidar3d 源头改造
- [ ] **P2.4** `deploy_zc.py` 支持 sidecar + 改造驱动部署

## P3 优先级

- [ ] **P3.1** 性能 benchmark
- [ ] **P3.2** CI 中加入 sidecar 生成和 mock driver patch smoke test
- [ ] **P3.3** 更复杂 SDK 驱动的 AI patch 规则
- [ ] **P3.4** 评估是否需要实验性 C++ direct 模式
