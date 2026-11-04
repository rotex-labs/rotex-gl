<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://raw.githubusercontent.com/rotex-labs/rotex-assets/55fa2474cb788a445c33b4ccf615eb8bbb9dbe64/brand/docs/crate-headers/dark/rotex-gl-header.svg">
    <source media="(prefers-color-scheme: light)" srcset="https://raw.githubusercontent.com/rotex-labs/rotex-assets/55fa2474cb788a445c33b4ccf615eb8bbb9dbe64/brand/docs/crate-headers/light/rotex-gl-header.svg">
    <img alt="rotex-gl" src="https://raw.githubusercontent.com/rotex-labs/rotex-assets/55fa2474cb788a445c33b4ccf615eb8bbb9dbe64/brand/docs/crate-headers/light/rotex-gl-header.svg" width="1200">
  </picture>
</p>

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

## License

Licensed under either of

* Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
* MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.
