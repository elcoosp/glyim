# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
## [0.11.0] - 2026-11-25

### Bug Fixes
- pin the GAP-0008 witness to its strict parse cascade by @elcoosp in [#271](https://github.com/elcoosp/glyim/pull/271) ([8ded0a6](https://github.com/elcoosp/glyim/commit/8ded0a6925989a6971a94b3b38a0322a895ea438))
- invert witnesses by declared shape, not ran_native by @elcoosp in [#269](https://github.com/elcoosp/glyim/pull/269) ([4674858](https://github.com/elcoosp/glyim/commit/4674858416a55ad750328a24e72f43bc40956186))
- name both witness tags in the discovery error by @elcoosp in [#269](https://github.com/elcoosp/glyim/pull/269) ([2f53f1e](https://github.com/elcoosp/glyim/commit/2f53f1efbfcbbac3156a73bd23d91880f5319ebe))
- invert run witnesses by backend for known-gap debts by @elcoosp in [#268](https://github.com/elcoosp/glyim/pull/268) ([3b866a9](https://github.com/elcoosp/glyim/commit/3b866a94870683e90ab21f0b6f4ad083db84b4bb))
- re-base evidence fixture paths onto the repo root by @elcoosp in [#266](https://github.com/elcoosp/glyim/pull/266) ([704c745](https://github.com/elcoosp/glyim/commit/704c745f746bb6f88344c1824e8a2b5e972588bb))

### Documentation
- retarget the witness example at GAP-0006 by @elcoosp in [#247](https://github.com/elcoosp/glyim/pull/247) ([c0051b6](https://github.com/elcoosp/glyim/commit/c0051b652dde04d1f4a351145a9bbfbf14fbea35))

### Features
- expose property::mir module by @elcoosp ([9cd8c6c](https://github.com/elcoosp/glyim/commit/9cd8c6ce6d3712f73a83009fea9d8d812303d50e))
- add random well-formed MIR body generator by @elcoosp ([df4fc20](https://github.com/elcoosp/glyim/commit/df4fc2025e5858b5f5ae7fc9a458d84652a938ca))
- validate witness shapes per tag at discovery by @elcoosp in [#269](https://github.com/elcoosp/glyim/pull/269) ([0d1ccdb](https://github.com/elcoosp/glyim/commit/0d1ccdbae5507bec54a86117caaec3550d8cea43))
- add the KnownGapWitness contract to configs by @elcoosp in [#269](https://github.com/elcoosp/glyim/pull/269) ([9f778cf](https://github.com/elcoosp/glyim/commit/9f778cf1293eb68aced4370e72cce1e8cb7bb004))
- add the known-gap-backend-divergence annotation by @elcoosp in [#269](https://github.com/elcoosp/glyim/pull/269) ([b7638a5](https://github.com/elcoosp/glyim/commit/b7638a575e2ba8fe6c654724e1b912cfb87afbdb))

### Refactor
- index operands in the mir snapshot printer by @elcoosp in [#282](https://github.com/elcoosp/glyim/pull/282) ([91d6c5d](https://github.com/elcoosp/glyim/commit/91d6c5d3b81011edf7f21e228c0b873d83820313))
- route harness optimize through the cow entry by @elcoosp in [#282](https://github.com/elcoosp/glyim/pull/282) ([577dbd0](https://github.com/elcoosp/glyim/commit/577dbd06423a0d2d188070818375aa94d3d18cd2))
- migrate diag_tests ErrorCode to from_registered by @elcoosp in [#280](https://github.com/elcoosp/glyim/pull/280) ([8381461](https://github.com/elcoosp/glyim/commit/838146114b777a1a5dce92e7901c80b924365683))
- use from_usize instead of from_raw by @elcoosp ([5856852](https://github.com/elcoosp/glyim/commit/585685243b30cf849a03089ffe17d70eeffdfef6))
- use from_usize instead of from_raw by @elcoosp ([0fdd4dc](https://github.com/elcoosp/glyim/commit/0fdd4dc84745b293de1a65e281b9ffcb1d2e96be))
- use from_usize instead of from_raw by @elcoosp ([e19ffdc](https://github.com/elcoosp/glyim/commit/e19ffdcf2dd6786fa76085bb01a00c0f28d822b0))
- use from_usize instead of from_raw by @elcoosp ([a2e45b0](https://github.com/elcoosp/glyim/commit/a2e45b073e025b92ae5ee5cc01d055facc9cffa6))
- use from_usize instead of from_raw by @elcoosp ([2ff93d0](https://github.com/elcoosp/glyim/commit/2ff93d06861ca5ed34b64f3b8bb6e4eed6d953b2))
- use from_usize instead of from_raw by @elcoosp ([6632da0](https://github.com/elcoosp/glyim/commit/6632da0f00d2e2de5b36676806fef16a6e932c2b))
- use from_usize instead of from_raw by @elcoosp ([4605877](https://github.com/elcoosp/glyim/commit/4605877157fd8c5f927c94d50815d5abb4d1028e))
- use from_usize instead of from_raw by @elcoosp ([f8bab0b](https://github.com/elcoosp/glyim/commit/f8bab0b45b15931f8a20ed246d186084eacacdfb))
- use from_usize instead of from_raw by @elcoosp ([20e65bf](https://github.com/elcoosp/glyim/commit/20e65bfdb1644ddde97ddbc1b30760d5d2dd5229))
- gate linker on real-llvm feature by @elcoosp ([2211d1f](https://github.com/elcoosp/glyim/commit/2211d1f4c5ab0e42054ae0a9c999aaa9c8d25e99))
- expect MIR optimization failure by @elcoosp ([1bb002b](https://github.com/elcoosp/glyim/commit/1bb002b107d6a0647710b6c3de8b368f04a260b5))
- CLI arg panic! -> usage error (micro-debt) by @elcoosp in [#273](https://github.com/elcoosp/glyim/pull/273) ([2e7e2d2](https://github.com/elcoosp/glyim/commit/2e7e2d2696f14b62a5dfe598754f4f5c5da4bb73))
- unreachable!() -> fallback arms (micro-debt) by @elcoosp in [#273](https://github.com/elcoosp/glyim/pull/273) ([d39b006](https://github.com/elcoosp/glyim/commit/d39b006a4026c4ef6085377b1058998e1c66d50b))

### Testing
- M6.4 def_map body-edit invariance property test by @elcoosp in [#276](https://github.com/elcoosp/glyim/pull/276) ([f59d3ae](https://github.com/elcoosp/glyim/commit/f59d3aee9e41e336e3b3f7216dfb717cac524205))
- M6.1 DefPathHash + M6.3 fingerprint body-edit invariance by @elcoosp in [#275](https://github.com/elcoosp/glyim/pull/275) ([558f5ac](https://github.com/elcoosp/glyim/commit/558f5ac9856de69ac0ca3cd2a2f1b89bb8ed77d0))
- register the stable_identity_tests module by @elcoosp in [#275](https://github.com/elcoosp/glyim/pull/275) ([22cb071](https://github.com/elcoosp/glyim/commit/22cb0710b27fd0780bad1dc9f0a569e957fe17d2))
- fuzz 10k malformed bodies through the validator (0 panics) by @elcoosp in [#275](https://github.com/elcoosp/glyim/pull/275) ([25674aa](https://github.com/elcoosp/glyim/commit/25674aa332e5f7744d068dd570c640b4c47cb66b))
- MirBodyGenerator mutation mode (6 bug classes) by @elcoosp in [#275](https://github.com/elcoosp/glyim/pull/275) ([6e4f999](https://github.com/elcoosp/glyim/commit/6e4f99906cdf72935e5036802f3eddf5a7f3fed3))
- cover unify-with-itself and MIR declaredness by @elcoosp ([3aff1ca](https://github.com/elcoosp/glyim/commit/3aff1ca2a339e5ade723229e2148037b19eb5807))
- needs_drop conservative run-pass witness by @elcoosp in [#273](https://github.com/elcoosp/glyim/pull/273) ([e2a630b](https://github.com/elcoosp/glyim/commit/e2a630be33a8fc6360572ff019bc4b47b946c824))
- panic no-unwind run-fail witness by @elcoosp in [#273](https://github.com/elcoosp/glyim/pull/273) ([51c2b4b](https://github.com/elcoosp/glyim/commit/51c2b4b4f6e5cd0c3552e53dd428c339c00b6be3))
- add strict-xfail witness for unreachable SEH cleanup by @elcoosp in [#272](https://github.com/elcoosp/glyim/pull/272) ([bbc27e7](https://github.com/elcoosp/glyim/commit/bbc27e75870c72448f4f4c3b5e64e904a337957d))
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

### build
- wire pipeline llvm-codegen feature by @elcoosp ([daf574f](https://github.com/elcoosp/glyim/commit/daf574fc01263e142d7bbaad4d650255cc197485))
**Full Changelog**: https://github.com/elcoosp/glyim/compare/v0.10.0...v0.11.0
