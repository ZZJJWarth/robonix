// SPDX-License-Identifier: MulanPSL-2.0
// Reference input: a normal ROS 2 PointCloud2 driver before source-side ZC.

#include <cstdint>
#include <memory>
#include <vector>

#include "rclcpp/rclcpp.hpp"
#include "sensor_msgs/msg/point_cloud2.hpp"
#include "sensor_msgs/msg/point_field.hpp"

using namespace std::chrono_literals;

class ExamplePointCloudDriver : public rclcpp::Node
{
public:
  ExamplePointCloudDriver()
  : rclcpp::Node("example_pointcloud_driver")
  {
    cloud_pub_ =
      create_publisher<sensor_msgs::msg::PointCloud2>("/lidar/points", rclcpp::SensorDataQoS());
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

  /// Publish the SDK scan on the legacy ROS payload topic.
  /// Constraint: this is the point a source-side ZC patch should target.
  void publish_scan()
  {
    auto msg = read_scan_from_sdk();
    cloud_pub_->publish(msg);
  }

  rclcpp::Publisher<sensor_msgs::msg::PointCloud2>::SharedPtr cloud_pub_;
  rclcpp::TimerBase::SharedPtr timer_;
};

int main(int argc, char ** argv)
{
  rclcpp::init(argc, argv);
  rclcpp::spin(std::make_shared<ExamplePointCloudDriver>());
  rclcpp::shutdown();
  return 0;
}
