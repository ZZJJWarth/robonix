// SPDX-License-Identifier: MulanPSL-2.0
// Reference output: source-side ZC PointCloud2 driver with ROS fallback.

#include <cstddef>
#include <cstdint>
#include <cstring>
#include <memory>
#include <vector>

#include "rclcpp/create_publisher.hpp"
#include "rclcpp/rclcpp.hpp"
#include "sensor_msgs/msg/point_cloud2.hpp"
#include "sensor_msgs/msg/point_field.hpp"
#include "std_msgs/msg/string.hpp"
#include "zc_pubsub.hpp"

using namespace std::chrono_literals;

namespace {

static constexpr const char * kShmName = "robonix_zc_lidar3d";
static constexpr size_t kShmSize = 67'108'864;
static constexpr const char * kLegacyTopic = "/lidar/points";
static constexpr const char * kZcTopic = "/lidar/lidar3d_zc";

}  // namespace

class ExamplePointCloudDriver : public rclcpp::Node
{
public:
  ExamplePointCloudDriver()
  : rclcpp::Node("example_pointcloud_driver")
  {
    cloud_pub_ =
      create_publisher<sensor_msgs::msg::PointCloud2>(kLegacyTopic, rclcpp::SensorDataQoS());
    zc_pub_ = rclcpp::create_publisher<std_msgs::msg::String, std::allocator<void>, ZcPublisher>(
      *this, kZcTopic, rclcpp::SensorDataQoS());
    publish_legacy_ros_ = declare_parameter<bool>("publish_legacy_ros", true);
    timer_ = create_wall_timer(100ms, [this]() { publish_scan(); });
  }

private:
  /// Fill one point cloud as if it came from a lidar SDK.
  /// Side effects: allocates fields and payload for the demo scan.
  sensor_msgs::msg::PointCloud2 read_scan_from_sdk()
  {
    sensor_msgs::msg::PointCloud2 msg;
    msg.header.stamp = now();
    msg.header.frame_id = "lidar_frame";
    msg.height = 1;
    msg.width = 16;
    msg.is_bigendian = 0;
    msg.point_step = 16;
    msg.row_step = msg.width * msg.point_step;
    msg.is_dense = 1;
    msg.fields.resize(4);
    const char * names[] = {"x", "y", "z", "intensity"};
    for (size_t i = 0; i < msg.fields.size(); ++i) {
      msg.fields[i].name = names[i];
      msg.fields[i].offset = static_cast<uint32_t>(i * 4);
      msg.fields[i].datatype = sensor_msgs::msg::PointField::FLOAT32;
      msg.fields[i].count = 1;
    }
    msg.data.assign(static_cast<size_t>(msg.row_step) * msg.height, 0);
    return msg;
  }

  /// Copy the completed ROS PointCloud2 into Boost shared memory.
  /// Constraint: caller must have initialized the same segment as the sidecar.
  ShmPointCloud2 * write_pointcloud_to_shm(const sensor_msgs::msg::PointCloud2 & msg)
  {
    auto * out = manager_->ShmPointCloud2GetNew(shm);
    out->header.stamp.sec = msg.header.stamp.sec;
    out->header.stamp.nanosec = msg.header.stamp.nanosec;
    out->header.frame_id = msg.header.frame_id.c_str();
    out->height = msg.height;
    out->width = msg.width;
    out->is_bigendian = msg.is_bigendian;
    out->point_step = msg.point_step;
    out->row_step = msg.row_step;
    out->is_dense = msg.is_dense;

    out->fields.clear();
    out->fields.reserve(msg.fields.size());
    for (const auto & field : msg.fields) {
      ShmPointField shm_field(ShmCharAllocator(shm->get_segment_manager()));
      shm_field.name = field.name.c_str();
      shm_field.offset = field.offset;
      shm_field.datatype = field.datatype;
      shm_field.count = field.count;
      out->fields.push_back(std::move(shm_field));
    }

    out->data.resize(msg.data.size());
    if (!msg.data.empty()) {
      std::memcpy(out->data.data(), msg.data.data(), msg.data.size());
    }
    return out;
  }

  /// Publish from the driver production point to shared memory.
  /// Side effects: publishes an object name on kZcTopic and optionally legacy ROS.
  void publish_scan()
  {
    auto msg = read_scan_from_sdk();

    // @ai-filled source: pointcloud_driver_before.cpp:L50-L54
    auto * shm_msg = write_pointcloud_to_shm(msg);
    zc_pub_->publish(shm_msg);

    if (publish_legacy_ros_) {
      cloud_pub_->publish(msg);
    }
  }

  rclcpp::Publisher<sensor_msgs::msg::PointCloud2>::SharedPtr cloud_pub_;
  std::shared_ptr<ZcPublisher> zc_pub_;
  rclcpp::TimerBase::SharedPtr timer_;
  bool publish_legacy_ros_{true};
};

int main(int argc, char ** argv)
{
  shm_init(kShmName, kShmSize);
  rclcpp::init(argc, argv);
  rclcpp::spin(std::make_shared<ExamplePointCloudDriver>());
  rclcpp::shutdown();
  shm_shutdown(kShmName);
  return 0;
}
