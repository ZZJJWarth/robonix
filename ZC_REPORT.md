# Robonix 零拷贝传输优化 — 分析总结报告

## 项目概述

本报告分析两个项目的架构，并提出在 Robonix（具身智能操作系统）中通过 Boost 零拷贝传输优化 ROS 框架接口的实施方案。

- **ROS 零拷贝项目** (`/home/warth/Desktop/build/ros`)：基于 Eclipse Iceoryx + Boost.Interprocess 的共享内存传输实现
- **Robonix 项目** (`/home/warth/Desktop/build/robonix`)：具身智能操作系统，通过 Capability 契约向外暴露硬件接口

---

## 1. ROS 零拷贝项目分析

### 核心机制

该项目使用 **共享内存对象名传递** 的方式实现零拷贝：

```
Publisher:  数据 → 写入共享内存 → 仅发布对象名 "ShmObject_42"
Subscriber: 收到对象名字符串 → 共享内存查找 → 读取数据（0 或 1 次拷贝）
```

### 关键组件

| 组件 | 作用 |
|------|------|
| `ShmManager` | 共享内存生命周期管理、订阅者追踪、引用计数 GC |
| `ShmImage` / `ShmPointCloud2` | 共享内存原生消息类型（使用 Boost.Interprocess 容器） |
| `ZcPublisher` | 继承 `rclcpp::Publisher<std_msgs::msg::String>`，发布对象名 |
| `ZcNode` | 继承 `rclcpp::Node`，提供两种模式：一次拷贝（反序列化为 ROS msg）和零拷贝（直接指针 + 清理回调） |
| `zc_py_bridge` | pybind11 桥接，Python 端使用共享内存 |

### 依赖

- **Boost.Interprocess**：`managed_shared_memory`、容器分配器、进程间互斥锁
- **Eclipse Iceoryx**：底层共享内存传输（通过 `iceoryx_posh`）
- **ROS 2 Rolling**：`rclcpp`、`std_msgs`、`sensor_msgs`
- **pybind11**：Python 绑定

### 支持的消息类型

- `sensor_msgs::msg::Image`（~6MB/帧 @ 1080p RGB）
- `sensor_msgs::msg::PointCloud2`（1-10MB/扫描）

---

## 2. Robonix 项目分析

### Capability 体系

Robonix 有三层抽象：
- **Primitive**（原语）：硬件驱动（相机、激光雷达、底盘、IMU、音频）
- **Service**（服务）：组合功能（建图、导航、语音、记忆）
- **Skill**（技能）：模型驱动，按需激活

### 契约系统

每个 Capability 由 TOML 契约文件定义，例如：

```toml
[contract]
id      = "robonix/primitive/camera/rgb"
version = "1"
kind    = "primitive"
idl     = "common_interfaces/sensor_msgs/msg/Image.msg"

[mode]
type = "topic_out"
```

### 传输层

Atlas proto 中定义了三种传输方式：
- `TRANSPORT_GRPC = 1`
- `TRANSPORT_ROS2 = 2`
- `TRANSPORT_MCP = 3`

### 重载荷 Capability（零拷贝优化目标）

| 契约 | IDL 类型 | 典型载荷大小 | 优先级 |
|------|---------|-------------|--------|
| `camera/rgb` | `sensor_msgs/Image` | ~6 MB/帧 | P0 |
| `camera/depth` | `sensor_msgs/Image` | ~2 MB/帧 | P0 |
| `lidar/lidar3d` | `sensor_msgs/PointCloud2` | 1-10 MB/扫描 | P0 |
| `lidar/lidar` | `sensor_msgs/LaserScan` | ~10 KB/扫描 | P1 |
| `imu/imu` | `sensor_msgs/Imu` | ~100 B/采样 | P1 |

### 代码生成工具

`robonix-codegen` 是核心工具链，支持多种输出目标：
- `--lang proto`：生成 protobuf
- `--lang ros2`：生成 ROS2 colcon 包
- `--lang mcp`：生成 MCP Python 类型

---

## 3. 关键设计决策

| 决策点 | 方案 A | 方案 B | 推荐 |
|--------|--------|--------|------|
| 传输粒度 | 新增 `ROS2_ZC` 传输枚举 | 在 `Ros2Params` 中添加 `zero_copy: bool` | **方案 A**（独立传输类型，变更面最小） |
| 契约命名 | 独立 `rgb_zc` 契约 | 同一 `rgb` 契约，不同 provider | **独立契约**（零破坏性，消费者显式选择） |
| C++ 依赖 | 保留 C++ Iceoryx/BIP 原生库 | 纯 Python `multiprocessing.shared_memory` | **保留 C++**（性能最优，已有成熟实现） |
| 代码生成范围 | 生成完整可运行的 provider | 生成骨架代码 | **完整生成**（ZC 模式足够公式化） |

---

## 4. 实施方案

### 架构概览

```
┌──────────────────────────────────────────────────────────┐
│              robonix-codegen --lang ros2_zc               │
├──────────────────────────────────────────────────────────┤
│  1. 契约扫描器                                            │
│     capabilities/**/*.toml → 按 IDL 类型过滤              │
│     (Image, PointCloud2, LaserScan)                      │
│                    │                                     │
│                    ▼                                     │
│  2. ZC 契约生成器                                         │
│     输出: capabilities/**/<name>_zc.v1.toml               │
│     (相同 IDL，新契约 ID，transport=ros2_zc)               │
│                    │                                     │
│                    ▼                                     │
│  3. Provider 代码生成器                                   │
│     模板 + 契约元数据 → 完整的 Python Primitive            │
│     内置 ZcPublisher/ZcSubscriber                        │
│                    │                                     │
│                    ▼                                     │
│  4. 构建集成                                              │
│     输出 CMakeLists.txt 片段                             │
│     用于 system/robonix_zc/ 库链接                        │
└──────────────────────────────────────────────────────────┘
```

### 零拷贝数据流

```
Provider (C++):
  sensor_msgs::Image → ShmImage (共享内存写入)
                    → 发布 "ShmObject_N" 到 ROS topic

Consumer:
  收到 "ShmObject_N" → 共享内存查找 ShmImage
                     → 一次拷贝模式: copy_image() → sensor_msgs::Image
                     → 零拷贝模式: 直接指针 + 清理回调
```

### 与现有系统的关系

- **不修改**任何现有 TOML 契约文件
- **不修改** `RosBackend` 或现有 ROS2 传输路径
- 新增 `system/robonix_zc/` 作为独立 C++ 库
- 新增 `pylib/robonix-zc/` 作为独立 Python 包
- 在 `robonix-codegen` 中添加 `--lang ros2_zc` 目标

---

## 5. 文件变更清单

| 文件 | 操作 | 说明 |
|------|------|------|
| `system/atlas/proto/atlas.proto` | 修改 | 添加 `TRANSPORT_ROS2_ZC = 4` |
| `system/robonix_zc/` | **新建** | C++ ZC 库 + pybind11 桥接 |
| `pylib/robonix-zc/` | **新建** | Python ZC 包 |
| `pylib/robonix-api/robonix_api/atlas_types.py` | 修改 | 添加 `ROS2_ZC` 传输类型 |
| `capabilities/primitive/camera/rgb_zc.v1.toml` | **新建** | ZC 相机契约 |
| `capabilities/primitive/camera/depth_zc.v1.toml` | **新建** | ZC 深度相机契约 |
| `capabilities/primitive/lidar/lidar3d_zc.v1.toml` | **新建** | ZC 激光雷达 3D 契约 |
| `capabilities/primitive/lidar/lidar_zc.v1.toml` | **新建** | ZC 激光雷达契约 |
| `capabilities/primitive/imu/imu_zc.v1.toml` | **新建** | ZC IMU 契约 |
| `tools/codegen/src/codegen/zc_scanner.rs` | **新建** | 重载荷契约检测 |
| `tools/codegen/src/codegen/zc_python_gen.rs` | **新建** | ZC provider 代码生成 |
| `tools/codegen/src/codegen/mod.rs` | 修改 | 注册新模块 |
| `tools/codegen/src/main.rs` | 修改 | 添加 `--lang ros2_zc` |
| `system/atlas/src/service.rs` | 修改 | 处理新传输类型 |
| `system/atlas/src/contract_registry.rs` | 修改 | 解析 TOML 中可选的 `[transport]` 节 |

---

## 6. 预期收益

- **延迟**：消除大消息（6MB 图像）的序列化/反序列化开销，预计减少 30-50% 端到端延迟
- **吞吐量**：共享内存绕过 DDS 层，释放 CPU 用于算法处理
- **内存**：避免多次数据拷贝，减少内存峰值
- **兼容性**：现有 ROS2 传输路径完整保留，ZC 为可选增强
