# RealSense 驱动接入 Robonix ZC 流程

本文说明如何使用当前 Robonix ZC 自动化工具，为下面这个 RealSense 相机驱动生成并接入一个 ZC 版本：

```text
/home/warth/Desktop/build/test/ros2_driver_test/ros2_pkgs/realsense_drv
```

当前自动化工具可以生成：

- ZC contract
- ZC sidecar
- Python bridge provider 模板
- 驱动源码修改提示文件 `zc_driver_patch_manifest.json`

但它还不能完全自动、安全地修改 RealSense 驱动源码。真正的零拷贝优化必须改驱动的数据生产点，让驱动直接把图像写入共享内存，而不是从已有 ROS topic 订阅图像后再写共享内存。

## 目标

为 RealSense 驱动加入两个 ZC capability：

```text
robonix/primitive/camera/rgb_zc
robonix/primitive/camera/depth_zc
```

RealSense 驱动中优先修改的位置是：

```text
/home/warth/Desktop/build/test/ros2_driver_test/ros2_pkgs/realsense_drv/src/realsense-ros/realsense2_camera/src/base_realsense_node.cpp
```

关键函数是：

```text
BaseRealSenseNode::publishFrame(...)
```

这个函数会填充 `sensor_msgs::msg::Image`，然后调用 `image_publisher->publish(...)`。ZC 代码应该加在这个数据生产点附近，而不是新增一个节点去订阅 `/camera_435i/.../image_raw`。

## 1. 设置路径变量

```bash
cd /home/warth/Desktop/build/robonix

export RBNX_REPO=/home/warth/Desktop/build/robonix
export RS_DRV=/home/warth/Desktop/build/test/ros2_driver_test/ros2_pkgs/realsense_drv
export RS_CAMERA_SRC=$RS_DRV/src/realsense-ros/realsense2_camera
export ZC_OUT=$RBNX_REPO/gen/zc/realsense
```

命令作用：

- `cd /home/warth/Desktop/build/robonix`：进入 Robonix 仓库根目录，后续从这里运行 codegen。
- `RBNX_REPO`：记录 Robonix 仓库路径。
- `RS_DRV`：记录 RealSense 驱动工作区路径。
- `RS_CAMERA_SRC`：记录 RealSense 相机源码包路径，codegen 会扫描这个目录里的 C++ 源码。
- `ZC_OUT`：记录生成物输出目录。

## 2. 生成 ZC contract、sidecar 和驱动补丁提示文件

```bash
cargo run -p robonix-codegen -- \
  --lang ros2_zc \
  --contracts capabilities \
  --filter robonix/primitive/camera/rgb \
  --filter robonix/primitive/camera/depth \
  --fill "$RS_CAMERA_SRC" \
  --out "$ZC_OUT"
```

命令作用：

- `cargo run -p robonix-codegen --`：运行 Robonix 本地的代码生成工具。
- `--lang ros2_zc`：选择生成 ROS2 ZC 相关文件。
- `--contracts capabilities`：从 `capabilities/` 目录读取 Robonix contract。
- `--filter robonix/primitive/camera/rgb`：生成 RGB 相机的 ZC 版本。
- `--filter robonix/primitive/camera/depth`：生成 depth 相机的 ZC 版本。
- `--fill "$RS_CAMERA_SRC"`：扫描 RealSense C++ 源码，生成驱动源码修改提示文件。
- `--out "$ZC_OUT"`：把生成物写入 `gen/zc/realsense`。

预期生成结果：

```text
gen/zc/realsense/
├── contracts/camera/rgb_zc.v1.toml
├── contracts/camera/depth_zc.v1.toml
├── providers/camera_rgb_zc_provider.py
├── providers/camera_depth_zc_provider.py
├── sidecars/camera_rgb_zc_sidecar.py
├── sidecars/camera_depth_zc_sidecar.py
├── sidecars/README.md
└── zc_driver_patch_manifest.json
```

注意：

- `providers/` 目录里的 Python 文件是兼容性 bridge 模板，不能解决驱动侧拷贝问题。
- 真正用于驱动侧 ZC 的是 `sidecars/` 和 `zc_driver_patch_manifest.json`。

## 3. 查看 sidecar 参数

```bash
sed -n '1,120p' "$ZC_OUT/sidecars/README.md"
```

命令作用：

- 查看生成的 sidecar 使用的共享内存名和 ROS topic 名。
- 后面修改 RealSense 驱动源码时，必须使用这里列出的同一组参数。

预期值类似：

```text
rgb_zc:   SHM_NAME=robonix_zc_rgb    TOPIC=/camera/rgb_zc
depth_zc: SHM_NAME=robonix_zc_depth  TOPIC=/camera/depth_zc
```

## 4. 查看驱动补丁提示文件

```bash
python3 -m json.tool "$ZC_OUT/zc_driver_patch_manifest.json"
```

命令作用：

- 格式化打印 `zc_driver_patch_manifest.json`。
- 查看 codegen 扫描出的候选源码文件、共享内存名、ZC topic 和修改约束。

对这个 RealSense 驱动来说，候选文件里通常会包含：

```text
$RS_CAMERA_SRC/src/base_realsense_node.cpp
$RS_CAMERA_SRC/src/image_publisher.cpp
$RS_CAMERA_SRC/src/rs_node_setup.cpp
```

修改优先级：

1. 优先修改 `src/base_realsense_node.cpp`，尤其是 `BaseRealSenseNode::publishFrame(...)`。
2. 只有在你决定封装现有 publish 抽象时，才考虑修改 `src/image_publisher.cpp`。
3. 不要优先修改工具、测试或非主路径文件。

## 5. 修改 RealSense 驱动源码

先阅读生成的 manifest 和示例：

```bash
sed -n '1,240p' "$ZC_OUT/zc_driver_patch_manifest.json"
sed -n '1,220p' examples/zc_driver_patch/README.md
sed -n '1,180p' examples/zc_driver_patch/image_driver_after.cpp
```

命令作用：

- 第一条命令查看本次 RealSense 驱动对应的 patch 参数。
- 第二条命令查看通用的驱动源码修改说明。
- 第三条命令查看图像驱动接入 ZC 后的大致代码形态。

源码修改要求：

- 在 RealSense 驱动的数据生产点修改，也就是 `BaseRealSenseNode::publishFrame(...)`。
- RGB 图像写入 `SHM_NAME=robonix_zc_rgb`，并在 `/camera/rgb_zc` 发布对象名。
- depth 图像写入 `SHM_NAME=robonix_zc_depth`，并在 `/camera/depth_zc` 发布对象名。
- 建议保留原始 ROS image publisher 作为 fallback，除非你明确决定关闭它。
- 不要新增节点去订阅 `/camera_435i/color/image_raw` 或 `/camera_435i/aligned_depth_to_color/image_raw`，那样只是 bridge，不是驱动侧 ZC。

如果使用 Claude Code，可以用这个 skill 辅助修改：

```text
/patch-zc-driver gen/zc/realsense/zc_driver_patch_manifest.json
```

这个 skill 的作用：

- 读取 `zc_driver_patch_manifest.json`。
- 查找真实的驱动数据生产点。
- 修改驱动源码。
- 修改构建文件。
- 输出验证命令和风险提示。

手动修改时，代码形态大致如下：

```cpp
#include "std_msgs/msg/string.hpp"
#include "zc_pubsub.hpp"
```

需要做的事情：

- 为 RGB 和 depth 分别创建一个 ZC publisher。
- 使用 sidecar README 里的共享内存名初始化共享内存。
- 把已经完成填充的 `sensor_msgs::msg::Image` 写入 `ShmImage`。
- 调用 ZC publisher 发布共享内存对象名。

## 6. 修改 RealSense 构建文件

打开 RealSense package 的构建文件：

```bash
sed -n '1,220p' "$RS_CAMERA_SRC/CMakeLists.txt"
sed -n '1,180p' "$RS_CAMERA_SRC/package.xml"
```

命令作用：

- 查看 `realsense2_camera` target 是在哪里定义的。
- 确认应该把 ZC 依赖加到哪个 target 上。

CMake 修改形态参考：

```cmake
find_package(cpp_pubsub REQUIRED)

target_link_libraries(realsense2_camera
  robonix_zc
)
```

如果该 package 使用 `ament_target_dependencies` 管理依赖，则按实际 target 补充：

```cmake
ament_target_dependencies(realsense2_camera
  rclcpp
  sensor_msgs
  std_msgs
  cpp_pubsub
)
```

如果 package 需要在 `package.xml` 声明依赖，则添加：

```xml
<depend>cpp_pubsub</depend>
```

这些修改的作用：

- `find_package(cpp_pubsub REQUIRED)`：让 CMake 找到 ZC 相关包。
- `target_link_libraries(... robonix_zc)`：让 RealSense 驱动链接 Boost 共享内存 ZC 库。
- `<depend>cpp_pubsub</depend>`：记录 ROS package 依赖。

## 7. 编译修改后的 RealSense 工作区

```bash
cd "$RS_DRV"

source /opt/ros/humble/setup.bash
source /home/warth/Desktop/build/ros/install/setup.bash
source ~/ros2_caret_ws/install/local_setup.bash

colcon build --symlink-install --cmake-args -DBUILD_TESTING=OFF
```

命令作用：

- `cd "$RS_DRV"`：进入 RealSense 驱动工作区根目录。
- `source /opt/ros/humble/setup.bash`：加载 ROS 2 Humble 环境。
- `source /home/warth/Desktop/build/ros/install/setup.bash`：加载包含 `cpp_pubsub` / `robonix_zc` 的工作区。
- `source ~/ros2_caret_ws/install/local_setup.bash`：保留该 RealSense 驱动原有构建环境。
- `colcon build --symlink-install --cmake-args -DBUILD_TESTING=OFF`：重新编译修改后的 RealSense package。

如果编译失败，优先检查：

```bash
ros2 pkg prefix cpp_pubsub
```

命令作用：

- 确认当前 shell 能找到 `cpp_pubsub` 包。

还需要检查：

- `realsense2_camera` target 是否正确链接了 `robonix_zc`。
- `zc_pubsub.hpp` 是否能被 include。
- `package.xml` 是否缺少依赖声明。

## 8. 启动 sidecar

打开一个终端，运行：

```bash
cd "$RBNX_REPO"

source /opt/ros/humble/setup.bash
source /home/warth/Desktop/build/ros/install/setup.bash
source "$RS_DRV/install/setup.bash"

export PYTHONPATH="$RBNX_REPO/pylib/robonix-api:$RBNX_REPO/pylib/robonix-zc/install/robonix_zc/lib/python3.*/site-packages:$PYTHONPATH"

python3 "$ZC_OUT/sidecars/camera_rgb_zc_sidecar.py" &
python3 "$ZC_OUT/sidecars/camera_depth_zc_sidecar.py" &
wait
```

命令作用：

- `cd "$RBNX_REPO"`：回到 Robonix 仓库。
- `source /opt/ros/humble/setup.bash`：加载 ROS 2 环境。
- `source /home/warth/Desktop/build/ros/install/setup.bash`：加载 ZC 运行时相关包。
- `source "$RS_DRV/install/setup.bash"`：加载刚编译的 RealSense 驱动工作区。
- `export PYTHONPATH=...`：让 Python 能找到 Robonix API 和 `robonix_zc` Python 包。
- `python3 "$ZC_OUT/sidecars/camera_rgb_zc_sidecar.py" &`：启动 RGB ZC sidecar。
- `python3 "$ZC_OUT/sidecars/camera_depth_zc_sidecar.py" &`：启动 depth ZC sidecar。
- `wait`：保持当前终端不退出。

sidecar 的作用：

- 向 Robonix 注册 `ROS2_ZC` capability。
- 创建或打开对应共享内存段。
- 暴露消费者需要的 `Ros2ZcParams`。

sidecar 不负责写图像数据。图像数据必须由修改后的 RealSense 驱动写入共享内存。

## 9. 启动修改后的 RealSense 驱动

打开另一个终端，运行：

```bash
cd "$RS_DRV"

source /opt/ros/humble/setup.bash
source /home/warth/Desktop/build/ros/install/setup.bash
source install/setup.bash

ros2 launch realsense2_camera rs_launch.py \
  camera_namespace:="/" \
  camera_name:="camera_435i" \
  enable_imu:=true \
  enable_gyro:=true \
  enable_accel:=true \
  unite_imu_method:=2 \
  align_depth.enable:=true \
  enable_sync:=true \
  publish_tf:=false \
  temporal_filter.enable:=true \
  hole_filling_filter.enable:=true \
  rgb_camera.color_profile:=640x480x30
```

命令作用：

- `cd "$RS_DRV"`：进入 RealSense 驱动工作区。
- `source /opt/ros/humble/setup.bash`：加载 ROS 2 环境。
- `source /home/warth/Desktop/build/ros/install/setup.bash`：加载 ZC 相关依赖。
- `source install/setup.bash`：加载修改并编译后的 RealSense package。
- `ros2 launch ...`：用现有参数启动 RealSense 相机节点。

启动后，修改过的驱动应该会：

- 保持原有 RealSense ROS topic 行为。
- 额外在 `/camera/rgb_zc` 发布 RGB 共享内存对象名。
- 额外在 `/camera/depth_zc` 发布 depth 共享内存对象名。

## 10. 验证 ROS topic

```bash
ros2 topic list | grep '_zc'
ros2 topic echo /camera/rgb_zc --once
ros2 topic echo /camera/depth_zc --once
```

命令作用：

- `ros2 topic list | grep '_zc'`：确认系统里出现了 ZC topic。
- `ros2 topic echo /camera/rgb_zc --once`：确认 RGB ZC topic 能输出一次对象名。
- `ros2 topic echo /camera/depth_zc --once`：确认 depth ZC topic 能输出一次对象名。

预期输出是 `std_msgs/msg/String`，内容类似：

```text
data: ShmObject_123
```

如果没有看到这些 topic，优先检查：

- 修改后的 RealSense 驱动是否真的启动了。
- `publishFrame(...)` 是否走到了 RGB/depth 分支。
- ZC publisher 是否初始化成功。
- sidecar 和驱动使用的 `SHM_NAME` / `TOPIC` 是否一致。

## 11. 验证 Robonix capability 注册

按你平时的方式启动 Robonix Atlas/boot，然后启动 sidecar。

预期 Robonix 能发现：

```text
robonix/primitive/camera/rgb_zc
robonix/primitive/camera/depth_zc
```

预期参数：

```text
rgb_zc:   transport=ROS2_ZC, shm_name=robonix_zc_rgb,   topic=/camera/rgb_zc
depth_zc: transport=ROS2_ZC, shm_name=robonix_zc_depth, topic=/camera/depth_zc
```

这一步的目的：

- 确认 Robonix 不是靠硬编码猜共享内存名。
- 确认消费者可以通过 capability 获取 `Ros2ZcParams`。

## 12. 接入 Robonix 部署流程

当前限制：

```text
tools/deploy-zc/deploy_zc.py
```

还没有完整支持“sidecar + 已修改驱动 package”的自动部署。因此现阶段建议手动接入：

1. 把生成的 sidecar 放到 RealSense primitive package 或你选定的 sidecar package 位置。
2. 确保 `rbnx/start.sh` 或部署脚本会启动：
   - 修改后的 RealSense 驱动
   - `camera_rgb_zc_sidecar.py`
   - `camera_depth_zc_sidecar.py`
3. 在部署使用的 package manifest 中加入 `_zc` capability。

不要只部署：

```text
providers/camera_rgb_zc_provider.py
providers/camera_depth_zc_provider.py
```

这些文件只是 bridge 模板，不能达到驱动侧 ZC 优化目标。

## 13. 测试后清理进程

```bash
pkill -f camera_rgb_zc_sidecar.py || true
pkill -f camera_depth_zc_sidecar.py || true
pkill -f realsense2_camera || true
```

命令作用：

- 停止 RGB sidecar。
- 停止 depth sidecar。
- 停止 RealSense 驱动进程。

## 检查清单

- 已运行 `robonix-codegen --lang ros2_zc`。
- 已生成 RGB/depth 的 ZC contract。
- 已生成 RGB/depth 的 sidecar。
- `zc_driver_patch_manifest.json` 中包含 `base_realsense_node.cpp`。
- RealSense 源码已在 `BaseRealSenseNode::publishFrame(...)` 附近修改。
- 没有新增“订阅原始 image topic 再写共享内存”的 bridge。
- `realsense2_camera` target 已链接 `robonix_zc`。
- sidecar 和驱动使用相同的 `SHM_NAME` / `TOPIC`。
- `/camera/rgb_zc` 能输出共享内存对象名。
- `/camera/depth_zc` 能输出共享内存对象名。
- Robonix 能看到 `rgb_zc` 和 `depth_zc` 两个 `ROS2_ZC` capability。
