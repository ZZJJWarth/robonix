# SPDX-License-Identifier: MulanPSL-2.0
"""Install Chronos reference readings while retaining real transport deadlines."""

import asyncio
from contextlib import asynccontextmanager
import json
import logging

import grpc
import robonix_chronos_clock as clock
from robonix_api import ATLAS

from . import clock as scene_clock

log = logging.getLogger(__name__)
CONTRACT = "robonix/system/chronos/reference_time"


def _invalidate(reason):
    reading = clock.reference_time()
    clock.update_reference_time(reading.timestamp_ns, reading.generation, False,
                                reading.uncertainty_ns, reading.verified, reason)


def _release_channels(consumer, provider):
    stub, wire = ATLAS._wire_stub, ATLAS._wire_pb
    response = stub.InspectAtlas(wire.InspectAtlasRequest(), timeout=2.0)
    channels = json.loads(response.json)["channels"]
    for channel_id, channel in channels.items():
        if (channel["consumer_id"] == consumer and channel["provider_id"] == provider
                and channel["contract_id"] == CONTRACT
                and channel["transport"] == "TRANSPORT_GRPC"):
            stub.DisconnectCapability(wire.DisconnectCapabilityRequest(channel_id=channel_id),
                                      timeout=1.0)


async def _follow(consumer, provider, reset_callbacks, stub_type):
    from google.protobuf.empty_pb2 import Empty

    previous_generation = None
    try:
        while True:
            try:
                # Reconcile uncertain replies before acquiring another channel.
                await asyncio.to_thread(_release_channels, consumer, provider)
                stub, wire = ATLAS._wire_stub, ATLAS._wire_pb
                connect = asyncio.create_task(asyncio.to_thread(
                    stub.ConnectCapability,
                    wire.ConnectCapabilityRequest(consumer_id=consumer, provider_id=provider,
                                                  contract_id=CONTRACT, transport=wire.TRANSPORT_GRPC),
                    timeout=2.0,
                ))
                try:
                    reply = await asyncio.shield(connect)
                except asyncio.CancelledError:
                    await asyncio.gather(connect, return_exceptions=True)
                    raise
                endpoint = reply.endpoint
                channel = (grpc.aio.secure_channel(endpoint.removeprefix("https://"), grpc.ssl_channel_credentials())
                           if endpoint.startswith("https://") else
                           grpc.aio.insecure_channel(endpoint.removeprefix("http://")))
                async with channel:
                    call = stub_type(channel).ReferenceTime(Empty())
                    try:
                        while True:
                            reading = await asyncio.wait_for(call.read(), timeout=5.0)
                            if reading is grpc.aio.EOF:
                                raise OSError("Chronos reference stream ended")
                            if (reading.available and previous_generation is not None
                                    and reading.generation != previous_generation):
                                _invalidate("Chronos reference generation changed")
                                for reset in reset_callbacks:
                                    await reset()
                            clock.update_reference_time(
                                reading.timestamp_ns, reading.generation, reading.available,
                                reading.uncertainty_ns, reading.verified, reading.reason,
                            )
                            if reading.available:
                                previous_generation = reading.generation
                    finally:
                        call.cancel()
            except (grpc.RpcError, OSError, asyncio.TimeoutError) as exc:
                _invalidate("Chronos reference stream is disconnected")
                log.warning("[scene/reference] %s", exc)
            await asyncio.sleep(1.0)
    finally:
        _invalidate("Scene reference subscription stopped")
        await asyncio.to_thread(_release_channels, consumer, provider)


async def _follow_linux(reset_callbacks):
    while True:
        reading = clock.reference_time()
        if reading.available and reading.generation != scene_clock._reference_generation:
            for reset in reset_callbacks:
                await reset()
            scene_clock._reference_generation = reading.generation
        await asyncio.sleep(0.1)


@asynccontextmanager
async def subscription(config, consumer, on_failure):
    config = config or {}
    kind = str(config.get("kind", "linux_system"))
    clock.configure_reference(kind)
    callbacks = []
    if kind == "linux_system":
        scene_clock._reference_generation = clock.reference_time().generation
        follow = _follow_linux(callbacks)
    else:
        scene_clock._reference_generation = None
        provider = str(config.get("provider_id", "chronos"))
        if not provider or provider != provider.strip():
            raise ValueError("reference provider_id must be nonempty and whitespace-normalized")
        from robonix_contracts_pb2_grpc import RobonixSystemChronosReferenceTimeStub
        follow = _follow(consumer, provider, callbacks, RobonixSystemChronosReferenceTimeStub)
    task = asyncio.create_task(follow, name="scene-reference-time")

    def completed(task):
        if not task.cancelled() and task.exception() is not None:
            on_failure(task.exception())

    task.add_done_callback(completed)
    try:
        yield callbacks
    finally:
        task.cancel()
        result = await asyncio.gather(task, return_exceptions=True)
        if isinstance(result[0], BaseException) and not isinstance(result[0], asyncio.CancelledError):
            raise RuntimeError(f"Scene reference subscription failed: {result[0]}") from result[0]
