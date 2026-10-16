# SPDX-License-Identifier: MulanPSL-2.0
"""Transport-independent verifier request handling."""
import asyncio
import json
import logging

from .vlm import strict_object

log = logging.getLogger("vlm_verifier")
_MODEL_TIMEOUT = 40.0


def parse_envelope(raw):
    """Validate required context while retaining JSON-or-string target values."""
    try:
        envelope = json.loads(raw, object_pairs_hook=strict_object)
    except (ValueError, TypeError):
        raise ValueError("args_json must contain a JSON object") from None
    if not isinstance(envelope, dict):
        raise ValueError("args_json must contain a JSON object")
    for key in ("target_provider_id", "target_contract_id", "target_description"):
        if not isinstance(envelope.get(key), str) or not envelope[key].strip():
            raise ValueError(f"{key} is required")
    if "target_args" not in envelope or "target_output" not in envelope:
        raise ValueError("target_args and target_output are required")
    args = envelope.get("verifier_args")
    if not isinstance(args, dict) or not isinstance(args.get("camera_provider_id"), str) or not args["camera_provider_id"].strip():
        raise ValueError("verifier_args.camera_provider_id is required")
    return envelope


async def verify(call_id, raw, observe, evaluate, reference=None):
    """Keep robot evidence waits separate from real model limits."""
    if not isinstance(call_id, str) or not call_id.strip():
        raise ValueError("call_id is required")
    envelope = parse_envelope(raw)
    start = reference() if reference is not None else None
    if reference is not None and start is None:
        raise OSError("Chronos reference unavailable")

    def check_reference():
        if reference is None:
            return
        current = reference()
        if current is None or current[1:] != start[1:] or current[0] < start[0]:
            raise OSError("Chronos reference interrupted during verification")

    try:
        image = await observe(envelope["verifier_args"]["camera_provider_id"].strip())
        check_reference()
        model = asyncio.create_task(evaluate(envelope, image))
        loop = asyncio.get_running_loop()
        deadline = loop.time() + _MODEL_TIMEOUT
        try:
            while True:
                remaining = deadline - loop.time()
                if remaining <= 0:
                    raise asyncio.TimeoutError()
                done, _ = await asyncio.wait({model}, timeout=min(.1, remaining))
                check_reference()
                if done:
                    verdict = model.result()
                    break
        finally:
            if not model.done():
                model.cancel()
            await asyncio.gather(model, return_exceptions=True)
    except asyncio.CancelledError:
        log.info("call_id=%r verification cancelled", call_id)
        raise
    except Exception as exc:
        log.warning("call_id=%r verification unavailable (%s)", call_id, type(exc).__name__)
        # Upstream exceptions may contain URLs, credentials, or response bodies.
        raise RuntimeError(f"visual verification unavailable ({type(exc).__name__})") from None
    log.info("call_id=%r verification passed=%s", call_id, verdict["passed"])
    return verdict
