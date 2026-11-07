# ADR 0001: Pin TypeScript to 6.x for `astro check`

**Status:** Accepted
**Date:** 2026-11-07

## Context

The docs site runs `astro check` as a quality gate. Astro 7.4's `astro check` does not currently support TypeScript 7. Running it with TypeScript 7.0.2 installed fails with:

> `astro check does not currently support TypeScript 7.0`

An experimental workaround exists (`tsc` + `@astrojs/ts-content-mapper`), but it is not the stable default.

Verified at execution time with pnpm:

- `astro`: **7.4.7** (latest stable, 2026-11-06)
- `@astrojs/starlight`: **0.42.10** (latest stable, 2026-11-06)
- `typescript` latest 6.x: **6.0.3**
- `typescript` 7.x latest: 7.0.2 (excluded)

## Decision

Pin `typescript` to `6.0.3` in `package.json` (exact, not a range) so `pnpm check` uses the supported major.

## Consequences

- `pnpm check` runs on the supported TypeScript 6.x line.
- The experimental TypeScript 7 path is not used. Revisit this ADR when Astro's `astro check` supports TypeScript 7 in the stable default path.

## Environment note: `@emnapi/runtime` pin

Astro's own TS 7 guard confirms the situation at runtime: `astro check` with TypeScript 7 installed fails with *"astro check does not currently support TypeScript 7.0. To continue using astro check, install TypeScript 6 instead."*

Additionally, in one Linux/Node build environment, loading `@astrojs/check` failed with a wasm-bridge error (`this.bridge.setLastError is not a function`) because its `@astrojs/language-server` chain resolved `@emnapi/runtime@2.0.0-beta.2` via `@napi-rs/wasm-runtime`. Pinning `@emnapi/runtime` to the `1.11.1` release (the same version Starlight's markdown chain uses successfully) as a devDependency fixes the load. This pin is environment-specific and harmless elsewhere; remove it once `@napi-rs/wasm-runtime` ships a compatible runtime resolution.
