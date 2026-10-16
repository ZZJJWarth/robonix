specVersion: 1
description: >-
  OpenAI-compatible vision model used to judge camera images. The camera is
  chosen per Executor verification rule (verifier_args.camera_provider_id),
  not here.

properties:
  reference:
    type: object
    description: Selected clock inherited from the deployment for camera waits and image age.
    properties:
      kind:
        type: string
        enum: [linux_system, ros_time]
      provider_id:
        type: string
        description: Chronos provider ID for ros_time; defaults to chronos.
  vlm:
    type: object
    description: Model endpoint, credential and model ID. All three fields are required and must be non-empty strings.
    properties:
      base_url:
        type: string
        description: >-
          API base URL, normally ending in /v1. Must use http or https and
          have a host, with no user credentials, query or fragment. Trailing
          slashes are removed and /chat/completions is appended.
      api_key:
        type: string
        description: API credential, sent as a Bearer token.
        x-secret: true
      model:
        type: string
        description: Model ID that accepts image_url data URIs. There is no default.
    required: [base_url, api_key, model]
required: [vlm]
