// SPDX-License-Identifier: MulanPSL-2.0
// Reference input: a normal ROS 2 Image driver before source-side ZC patching.

#include <cstdint>
#include <memory>
#include <vector>

#include "rclcpp/rclcpp.hpp"
#include "sensor_msgs/msg/image.hpp"

using namespace std::chrono_literals;

class ExampleImageDriver : public rclcpp::Node
{
public:
  ExampleImageDriver()
  : rclcpp::Node("example_image_driver")
  {
    image_pub_ = create_publisher<sensor_msgs::msg::Image>("/camera/rgb", rclcpp::SensorDataQoS());
    timer_ = create_wall_timer(33ms, [this]() { publish_frame(); });
  }

private:
  /// Fill one RGB frame as if it came from a camera SDK.
  /// Side effects: allocates the ROS message payload for the demo frame.
  sensor_msgs::msg::Image read_frame_from_sdk()
  {
    sensor_msgs::msg::Image msg;
    msg.header.stamp = now();
    msg.header.frame_id = "camera_frame";
    msg.height = 480;
    msg.width = 640;
    msg.encoding = "rgb8";
    msg.is_bigendian = 0;
    msg.step = msg.width * 3;
    msg.data.assign(static_cast<size_t>(msg.height) * msg.step, 0x80);
    return msg;
  }

  /// Publish the SDK frame on the legacy ROS payload topic.
  /// Constraint: this is the point a source-side ZC patch should target.
  void publish_frame()
  {
    auto msg = read_frame_from_sdk();
    image_pub_->publish(msg);
  }

  rclcpp::Publisher<sensor_msgs::msg::Image>::SharedPtr image_pub_;
  rclcpp::TimerBase::SharedPtr timer_;
};

int main(int argc, char ** argv)
{
  rclcpp::init(argc, argv);
  rclcpp::spin(std::make_shared<ExampleImageDriver>());
  rclcpp::shutdown();
  return 0;
}
