# rotex-gl

WebGL2 and GLES 3.0 backend that reports the features it lacks.

> [!WARNING]
> **Beta, pre-1.0.** Rotex is under active development. APIs, behavior, and crate
> boundaries can change in any 0.x release. Do not depend on it for production work yet.

## Overview

`rotex-gl` implements `GpuPlatform` and `GpuBackend` through glow, on native and wasm. Shader input is the GLSL payload from the cooker. A command whose capability is absent returns an error.

## Responsibilities

- `GlGpuPlatform` and `GlBridge`
- Copies, readback, render-to-texture, dynamic offsets, and explicit pipelines
- Capability bits for the features GLES 3.0 does not provide

## Dependencies

`rotex-core` and `rotex-types`. Tests use `rotex-conformance`.

## Status

Version 0.2.0. Compute, storage buffers, push constants, and queries are absent. Barriers are accepted and do nothing. Native context creation needs EGL; tests return when it is missing.

## Part of Rotex

Rotex is a graphics HAL workspace. See [rotex-dev](https://github.com/rotex-labs/rotex-dev) for the architecture and the other crates.

## License

Licensed under [MIT](LICENSE).
