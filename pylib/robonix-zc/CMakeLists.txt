cmake_minimum_required(VERSION 3.20)
project(robonix_zc)

# find dependencies
find_package(ament_cmake REQUIRED)
find_package(Python3 COMPONENTS Interpreter Development REQUIRED)
find_package(pybind11_vendor QUIET)
find_package(pybind11 QUIET)

# ROS 2 packages
find_package(rclcpp REQUIRED)

# librobonix_zc.so — exported by cpp_pubsub
find_package(cpp_pubsub REQUIRED)

if(NOT pybind11_FOUND)
  message(FATAL_ERROR "pybind11 is required to build the robonix ZC Python lifecycle module")
endif()

# ── pybind11 module ────────────────────────────────────────────────────────
pybind11_add_module(_zc_bridge src/_zc_bridge.cpp)

# Include headers from cpp_pubsub (zc_shm.hpp, zc_pubsub.hpp)
target_include_directories(_zc_bridge PRIVATE ${cpp_pubsub_INCLUDE_DIRS})

target_link_libraries(_zc_bridge PRIVATE
  ${cpp_pubsub_LIBRARIES}
  rclcpp::rclcpp)

# Install into site-packages so "from robonix_zc import init_shm" works
set(PY_SITE_PACKAGES_DIR
  "lib/python${Python3_VERSION_MAJOR}.${Python3_VERSION_MINOR}/site-packages/robonix_zc")

install(
  TARGETS _zc_bridge
  LIBRARY DESTINATION ${PY_SITE_PACKAGES_DIR})

install(
  FILES robonix_zc/__init__.py
  DESTINATION ${PY_SITE_PACKAGES_DIR})

ament_package()
