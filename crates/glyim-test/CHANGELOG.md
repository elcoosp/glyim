# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
## [0.11.0] - 2026-11-21

### Bug Fixes
- invert witnesses by declared shape, not ran_native by @elcoosp in [#269](https://github.com/elcoosp/glyim/pull/269) ([4674858](https://github.com/elcoosp/glyim/commit/4674858416a55ad750328a24e72f43bc40956186))
- name both witness tags in the discovery error by @elcoosp in [#269](https://github.com/elcoosp/glyim/pull/269) ([2f53f1e](https://github.com/elcoosp/glyim/commit/2f53f1efbfcbbac3156a73bd23d91880f5319ebe))
- invert run witnesses by backend for known-gap debts by @elcoosp in [#268](https://github.com/elcoosp/glyim/pull/268) ([3b866a9](https://github.com/elcoosp/glyim/commit/3b866a94870683e90ab21f0b6f4ad083db84b4bb))
- re-base evidence fixture paths onto the repo root by @elcoosp in [#266](https://github.com/elcoosp/glyim/pull/266) ([704c745](https://github.com/elcoosp/glyim/commit/704c745f746bb6f88344c1824e8a2b5e972588bb))

### Documentation
- retarget the witness example at GAP-0006 by @elcoosp in [#247](https://github.com/elcoosp/glyim/pull/247) ([c0051b6](https://github.com/elcoosp/glyim/commit/c0051b652dde04d1f4a351145a9bbfbf14fbea35))

### Features
- validate witness shapes per tag at discovery by @elcoosp in [#269](https://github.com/elcoosp/glyim/pull/269) ([0d1ccdb](https://github.com/elcoosp/glyim/commit/0d1ccdbae5507bec54a86117caaec3550d8cea43))
- add the KnownGapWitness contract to configs by @elcoosp in [#269](https://github.com/elcoosp/glyim/pull/269) ([9f778cf](https://github.com/elcoosp/glyim/commit/9f778cf1293eb68aced4370e72cce1e8cb7bb004))
- add the known-gap-backend-divergence annotation by @elcoosp in [#269](https://github.com/elcoosp/glyim/pull/269) ([b7638a5](https://github.com/elcoosp/glyim/commit/b7638a575e2ba8fe6c654724e1b912cfb87afbdb))

### Testing
- cover the witness matrix and tag exclusivity by @elcoosp in [#269](https://github.com/elcoosp/glyim/pull/269) ([052fb50](https://github.com/elcoosp/glyim/commit/052fb5031ab9a9ab1b074d90581297a27107cb04))
- key differential skips on the witness contract by @elcoosp in [#269](https://github.com/elcoosp/glyim/pull/269) ([07ffada](https://github.com/elcoosp/glyim/commit/07ffada52ed6fea37ee925f2f2cef783b1f0d6d2))
- declare the GAP-0007 witness as a divergence by @elcoosp in [#269](https://github.com/elcoosp/glyim/pull/269) ([14483f7](https://github.com/elcoosp/glyim/commit/14483f7beaedc7697f2821b7bc394fee58d8e620))
- witness GAP-0008 leading-or parse gap (compile-fail) by @elcoosp in [#268](https://github.com/elcoosp/glyim/pull/268) ([a36cc84](https://github.com/elcoosp/glyim/commit/a36cc845b294e490a5dce202f3d7668010e813c3))
- cover the backend-divergence witness inversion by @elcoosp in [#268](https://github.com/elcoosp/glyim/pull/268) ([542fde1](https://github.com/elcoosp/glyim/commit/542fde1cefd204d7cb87bb60c450a2f50c47dbe0))
- skip registered known-gap witnesses in differential by @elcoosp in [#268](https://github.com/elcoosp/glyim/pull/268) ([55083b2](https://github.com/elcoosp/glyim/commit/55083b20c38eaf09a9cc9448625cb3c0e149e68c))
- pin async_loop_await as a GAP-0007 witness by @elcoosp in [#268](https://github.com/elcoosp/glyim/pull/268) ([c738bdd](https://github.com/elcoosp/glyim/commit/c738bdd7701be5dc24191e20ef96d079d7d59c72))
- pin async_for_loop_await as a GAP-0007 witness by @elcoosp in [#268](https://github.com/elcoosp/glyim/pull/268) ([f5bff55](https://github.com/elcoosp/glyim/commit/f5bff55a1df0b21a95368c9654574de2d964bbe1))
- add the GAP-0007 strict-xfail differential witness by @elcoosp in [#268](https://github.com/elcoosp/glyim/pull/268) ([f79e67e](https://github.com/elcoosp/glyim/commit/f79e67e3b548965ceccdef3b3b2595a595a23390))
- pin hermetic fixture paths and golden evidence writes by @elcoosp in [#266](https://github.com/elcoosp/glyim/pull/266) ([79bfd66](https://github.com/elcoosp/glyim/commit/79bfd66cee628e1c6f6b0969c2d800f44cfecb7a))
- retire the loop-await witness into run-pass by @elcoosp in [#250](https://github.com/elcoosp/glyim/pull/250) ([82ef09b](https://github.com/elcoosp/glyim/commit/82ef09b678c8402d0addf52ae55108801ba08751))
- re-target the gap witness at non-range for by @elcoosp in [#250](https://github.com/elcoosp/glyim/pull/250) ([579e629](https://github.com/elcoosp/glyim/commit/579e629709405db61bb9450bc76e08039b0e04a4))
- retire the unbounded-loop witness fixture by @elcoosp in [#250](https://github.com/elcoosp/glyim/pull/250) ([562c605](https://github.com/elcoosp/glyim/commit/562c6054c9e5fca537d3d7d89aba6399caad0b14))
- migrate the for-loop await witness to run-pass by @elcoosp in [#247](https://github.com/elcoosp/glyim/pull/247) ([24cc424](https://github.com/elcoosp/glyim/commit/24cc424f75e4156451acfcbb7ba881f741e0a9b3))
- witness the remaining async loop shapes as GAP-0006 by @elcoosp in [#247](https://github.com/elcoosp/glyim/pull/247) ([ac262c4](https://github.com/elcoosp/glyim/commit/ac262c473bea99d43185390bc29a35261f80cfeb))
- retire the await-in-loop known-gap witness by @elcoosp in [#247](https://github.com/elcoosp/glyim/pull/247) ([fedc224](https://github.com/elcoosp/glyim/commit/fedc224a0cacc1f0143476dac59309096c9d38ff))
- thread fixture --opt-level into MIR pass plan by @elcoosp in [#211](https://github.com/elcoosp/glyim/pull/211) ([4ab3aa9](https://github.com/elcoosp/glyim/commit/4ab3aa92cf7731f4bbb7ddd458974101cb122b65))
**Full Changelog**: https://github.com/elcoosp/glyim/compare/v0.10.0...v0.11.0
