// SPDX-License-Identifier: MulanPSL-2.0
// Reference output: source-side ZC Image driver with legacy ROS fallback.

#include <cstddef>
#include <cstdint>
#include <cstring>
#include <memory>
#include <vector>

#include "rclcpp/create_publisher.hpp"
#include "rclcpp/rclcpp.hpp"
#include "sensor_msgs/msg/image.hpp"
#include "std_msgs/msg/string.hpp"
#include "zc_pubsub.hpp"

using namespace std::chrono_literals;

namespace {

static constexpr const char * kShmName = "robonix_zc_rgb";
static constexpr size_t kShmSize = 67'108'864;
static constexpr const char * kLegacyTopic = "/camera/rgb";
static constexpr const char * kZcTopic = "/camera/rgb_zc";

}  // namespace

class ExampleImageDriver : public rclcpp::Node
{
public:
  ExampleImageDriver()
  : rclcpp::Node("example_image_driver")
  {
    image_pub_ = create_publisher<sensor_msgs::msg::Image>(kLegacyTopic, rclcpp::SensorDataQoS());
    zc_pub_ = rclcpp::create_publisher<std_msgs::msg::String, std::allocator<void>, ZcPublisher>(
      *this, kZcTopic, rclcpp::SensorDataQoS());
    publish_legacy_ros_ = declare_parameter<bool>("publish_legacy_ros", true);
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

  /// Copy the completed ROS Image into Boost shared memory.
  /// Constraint: caller must have initialized the same segment as the sidecar.
  ShmImage * write_image_to_shm(const sensor_msgs::msg::Image & msg)
  {
    auto * out = manager_->ShmImageGetNew(shm);
    out->header.stamp.sec = msg.header.stamp.sec;
    out->header.stamp.nanosec = msg.header.stamp.nanosec;
    out->header.frame_id = msg.header.frame_id.c_str();
    out->height = msg.height;
    out->width = msg.width;
    out->encoding = msg.encoding.c_str();
    out->is_bigendian = msg.is_bigendian;
    out->step = msg.step;
    out->data.resize(msg.data.size());
    if (!msg.data.empty()) {
      std::memcpy(out->data.data(), msg.data.data(), msg.data.size());
    }
    return out;
  }

  /// Publish from the driver production point to shared memory.
  /// Side effects: publishes an object name on kZcTopic and optionally legacy ROS.
  void publish_frame()
  {
    auto msg = read_frame_from_sdk();

    // @ai-filled source: image_driver_before.cpp:L38-L42
    auto * shm_msg = write_image_to_shm(msg);
    zc_pub_->publish(shm_msg);

    if (publish_legacy_ros_) {
      image_pub_->publish(msg);
    }
  }

  rclcpp::Publisher<sensor_msgs::msg::Image>::SharedPtr image_pub_;
  std::shared_ptr<ZcPublisher> zc_pub_;
  rclcpp::TimerBase::SharedPtr timer_;
  bool publish_legacy_ros_{true};
};

int main(int argc, char ** argv)
{
  shm_init(kShmName, kShmSize);
  rclcpp::init(argc, argv);
  rclcpp::spin(std::make_shared<ExampleImageDriver>());
  rclcpp::shutdown();
  shm_shutdown(kShmName);
  return 0;
}
