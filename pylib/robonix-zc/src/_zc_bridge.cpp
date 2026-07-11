// SPDX-License-Identifier: MulanPSL-2.0
// pybind11 module: Python lifecycle calls -> librobonix_zc.so

#include <cstddef>
#include <string>

#include <pybind11/pybind11.h>

#include "zc_pubsub.hpp"

namespace py = pybind11;

PYBIND11_MODULE(_zc_bridge, m)
{
  m.doc() = "Python lifecycle bindings for robonix zero-copy shared memory";

  m.def(
    "init_shm",
    [](const std::string & name, size_t size) {
      shm_init(name.c_str(), size);
    },
    py::arg("name") = "robonix_zc",
    py::arg("size") = 67108864);

  m.def(
    "shutdown_shm",
    [](const std::string & name) {
      shm_shutdown(name.c_str());
    },
    py::arg("name") = "robonix_zc");
}
