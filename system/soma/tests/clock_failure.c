// SPDX-License-Identifier: MulanPSL-2.0
#define _GNU_SOURCE
#include <dlfcn.h>
#include <errno.h>
#include <stdint.h>
#include <time.h>

// Scope interposition to the test thread so Tokio's other threads keep real time.
static _Thread_local int fail_after;
static _Thread_local uint64_t steady_ns;
static _Thread_local long reference_offset;

void soma_test_clock(int after, uint64_t steady, long offset) {
    fail_after = after;
    steady_ns = steady;
    reference_offset = offset;
}

int clock_gettime(clockid_t clock, struct timespec *value) {
    if (clock == CLOCK_MONOTONIC) {
        if (fail_after > 0 && --fail_after == 0) {
            errno = EIO;
            return -1;
        }
        if (steady_ns != 0) {
            value->tv_sec = steady_ns / 1000000000;
            value->tv_nsec = steady_ns % 1000000000;
            return 0;
        }
    }
    int result = ((int (*)(clockid_t, struct timespec *))dlsym(RTLD_NEXT, "clock_gettime"))(clock, value);
    if (result == 0 && clock == CLOCK_REALTIME) value->tv_sec += reference_offset;
    return result;
}
