# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
## [0.10.1] - 2026-11-21

### Bug Fixes
- compile the LedgerGate end-to-end test by @elcoosp in [#267](https://github.com/elcoosp/glyim/pull/267) ([b6a196e](https://github.com/elcoosp/glyim/commit/b6a196e2726e50622c038f001224bad52ecbbf89))
- reject unusable bind addresses as config errors by @elcoosp in [#229](https://github.com/elcoosp/glyim/pull/229) ([ed5825a](https://github.com/elcoosp/glyim/commit/ed5825adaf95a1117b7abdb871f84cfe923fa12d))
- error on corrupt state loads instead of panicking by @elcoosp in [#229](https://github.com/elcoosp/glyim/pull/229) ([3954960](https://github.com/elcoosp/glyim/commit/39549607e44153e16dc1fbad146cdf9a5b1a8ad2))
- error on streams.json entries without ids by @elcoosp in [#226](https://github.com/elcoosp/glyim/pull/226) ([41ef084](https://github.com/elcoosp/glyim/commit/41ef08422da97608840bb20927eb902402583196))

### Features
- register the LedgerGate in the done pipeline by @elcoosp in [#266](https://github.com/elcoosp/glyim/pull/266) ([40db2e8](https://github.com/elcoosp/glyim/commit/40db2e83945b824b58b11405196f43c01a3088b0))
- wire the LedgerGate through the done-gate config by @elcoosp in [#266](https://github.com/elcoosp/glyim/pull/266) ([2f38e31](https://github.com/elcoosp/glyim/commit/2f38e31e7d065fac2b6d787b92a3575161f168a6))
- add the LedgerGate consuming ledger next by @elcoosp in [#266](https://github.com/elcoosp/glyim/pull/266) ([99c5f25](https://github.com/elcoosp/glyim/commit/99c5f25879658b196f8b0c203e3d0c71887b3f02))

### Miscellaneous
- export the ledger gate module by @elcoosp in [#266](https://github.com/elcoosp/glyim/pull/266) ([502a18b](https://github.com/elcoosp/glyim/commit/502a18b98ddc57e349d7714c7e8e1aa64fa33057))
**Full Changelog**: https://github.com/elcoosp/glyim/compare/v0.10.0...v0.10.1
