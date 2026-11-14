# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
## [0.10.1] - 2026-11-14

### Bug Fixes
- compile diff gate and execute scale subcommand by @elcoosp in [#214](https://github.com/elcoosp/glyim/pull/214) ([dad0ae7](https://github.com/elcoosp/glyim/commit/dad0ae7bd35333a1866eb5b12c2da99d2faa10c8))

### Documentation
- add the corpus_commit walkthrough by @elcoosp in [#218](https://github.com/elcoosp/glyim/pull/218) ([f6e5dd5](https://github.com/elcoosp/glyim/commit/f6e5dd598355c6b822fb6089fdd524ce4897082a))
- add the history_trend consumer walkthrough by @elcoosp in [#216](https://github.com/elcoosp/glyim/pull/216) ([f8613f3](https://github.com/elcoosp/glyim/commit/f8613f3ce22291752caa1cf19f6ddcb773d3da10))
- add the matrix_gate consumer walkthrough by @elcoosp in [#216](https://github.com/elcoosp/glyim/pull/216) ([cd7e88b](https://github.com/elcoosp/glyim/commit/cd7e88b34d9cd8fd4acbc2440a5775a47d390388))
- add history_baseline consumer walkthrough example by @elcoosp in [#215](https://github.com/elcoosp/glyim/pull/215) ([5669400](https://github.com/elcoosp/glyim/commit/56694002c43ed398fc4ef8999e3e7d67074cb90f))
- add budget_check consumer walkthrough example by @elcoosp in [#215](https://github.com/elcoosp/glyim/pull/215) ([732b9a9](https://github.com/elcoosp/glyim/commit/732b9a91164fafba4ca7d81d890a9e67af482e45))
- add scaling_fit consumer walkthrough example by @elcoosp in [#214](https://github.com/elcoosp/glyim/pull/214) ([876ae93](https://github.com/elcoosp/glyim/commit/876ae93c4701c75c5f60aa0ced708b5adc9baa21))
- add diff_reports consumer walkthrough example by @elcoosp in [#213](https://github.com/elcoosp/glyim/pull/213) ([a22f5b4](https://github.com/elcoosp/glyim/commit/a22f5b4d6310e8bf79d9194b555f707fd60c6432))

### Features
- wire the corpus commands and drift exit by @elcoosp in [#218](https://github.com/elcoosp/glyim/pull/218) ([dfa5335](https://github.com/elcoosp/glyim/commit/dfa533590ada2e3346e2ec3f595e2c1a45d961ae))
- add the corpus write and check commands by @elcoosp in [#218](https://github.com/elcoosp/glyim/pull/218) ([c0062ed](https://github.com/elcoosp/glyim/commit/c0062ed630fdbd4289ecd57e6c2b8585ebc31abf))
- mount the corpus_dir module by @elcoosp in [#218](https://github.com/elcoosp/glyim/pull/218) ([f15c9d6](https://github.com/elcoosp/glyim/commit/f15c9d60d1bd4a0143cc0efd3a2ab8ba5083289d))
- add the committed corpus directory seam by @elcoosp in [#218](https://github.com/elcoosp/glyim/pull/218) ([a117ab9](https://github.com/elcoosp/glyim/commit/a117ab98750ab6b6f0eba2721bc3f1b3964c9ed2))
- add the corpus-dir typed errors by @elcoosp in [#218](https://github.com/elcoosp/glyim/pull/218) ([23a827d](https://github.com/elcoosp/glyim/commit/23a827dc45104c46ef337832a37d1874a49b7436))
- gate matrix and trend verdicts on the exit code by @elcoosp in [#216](https://github.com/elcoosp/glyim/pull/216) ([c97b2ab](https://github.com/elcoosp/glyim/commit/c97b2abfd61abe8155a020fb55412ef8d016611e))
- wire matrix subcommand and history trend action by @elcoosp in [#216](https://github.com/elcoosp/glyim/pull/216) ([3d4a8c0](https://github.com/elcoosp/glyim/commit/3d4a8c050db9c708318f22879ed5baa682162e0a))
- mount the matrix and trend modules with walkthroughs by @elcoosp in [#216](https://github.com/elcoosp/glyim/pull/216) ([ad9738b](https://github.com/elcoosp/glyim/commit/ad9738b2273d3a041f57c7fe32e024c3303c086b))
- add history trend engine with windowed drift gate by @elcoosp in [#216](https://github.com/elcoosp/glyim/pull/216) ([b7c5d42](https://github.com/elcoosp/glyim/commit/b7c5d42f288b434f0d01eaca6bc415887b4c81a2))
- add the matrix engine with the geo-mean gate by @elcoosp in [#216](https://github.com/elcoosp/glyim/pull/216) ([560e034](https://github.com/elcoosp/glyim/commit/560e0345142448e4a9ffd322958bff991e79bc1c))
- add BaselineUnavailable, MatrixPlan, TrendPlan errors by @elcoosp in [#216](https://github.com/elcoosp/glyim/pull/216) ([7b1f25a](https://github.com/elcoosp/glyim/commit/7b1f25ad3b145a8ab001d13a27e1e25f70a1ed58))
- add perf-history store with latest-baseline seam by @elcoosp in [#215](https://github.com/elcoosp/glyim/pull/215) ([b61033d](https://github.com/elcoosp/glyim/commit/b61033d6d1e0bba892f5125a96e5419f9313805e))
- map the budget gate verdict to the exit code by @elcoosp in [#215](https://github.com/elcoosp/glyim/pull/215) ([efcdb4a](https://github.com/elcoosp/glyim/commit/efcdb4a6d5a8644a90d26b494386e8921c61616c))
- wire the budget and history subcommands by @elcoosp in [#215](https://github.com/elcoosp/glyim/pull/215) ([44a7d13](https://github.com/elcoosp/glyim/commit/44a7d13f63e2618a82d1e13a814146d86184306a))
- add budgets/history error variants with help text by @elcoosp in [#215](https://github.com/elcoosp/glyim/pull/215) ([214d426](https://github.com/elcoosp/glyim/commit/214d426d32ab464ebc4c8e481f3af9cfed769e0a))
- add the budget engine with the BudgetStore seam by @elcoosp in [#215](https://github.com/elcoosp/glyim/pull/215) ([ce210b0](https://github.com/elcoosp/glyim/commit/ce210b0fc5b7ca1a91b11f83f5681bedb3d1be7e))
- mount the budget and history modules by @elcoosp in [#215](https://github.com/elcoosp/glyim/pull/215) ([1398789](https://github.com/elcoosp/glyim/commit/139878994e98109fb21ba221eba8a5476dc9ef18))
- add the scale subcommand and size parser by @elcoosp in [#214](https://github.com/elcoosp/glyim/pull/214) ([363752e](https://github.com/elcoosp/glyim/commit/363752eb8cc5275906d34d818150e672ae80a074))
- mount the scaling module by @elcoosp in [#214](https://github.com/elcoosp/glyim/pull/214) ([8d91762](https://github.com/elcoosp/glyim/commit/8d9176203cf9b359965a7ce3b0426f7f68943713))
- add scaling analyzer with log-log fit seam by @elcoosp in [#214](https://github.com/elcoosp/glyim/pull/214) ([e46f190](https://github.com/elcoosp/glyim/commit/e46f190abe771e18994f395aa88537e2ca9f0c24))
- add InMemoryReportStore::new constructor by @elcoosp in [#214](https://github.com/elcoosp/glyim/pull/214) ([fec9644](https://github.com/elcoosp/glyim/commit/fec96447159e523878057159ecb00999f2bee771))
- execute diff and map gate verdict to exit code by @elcoosp in [#213](https://github.com/elcoosp/glyim/pull/213) ([14914fc](https://github.com/elcoosp/glyim/commit/14914fc4a797fe40789deb69700583b25a6bad55))
- add diff subcommand with regression budget by @elcoosp in [#213](https://github.com/elcoosp/glyim/pull/213) ([3382cdf](https://github.com/elcoosp/glyim/commit/3382cdfef4c6ab1a88fe16663b1c9765d4851b60))
- mount diff module by @elcoosp in [#213](https://github.com/elcoosp/glyim/pull/213) ([8598a0a](https://github.com/elcoosp/glyim/commit/8598a0a617ae8805f4f279c6b151852b961e7290))
- add report diff engine and ReportStore seam by @elcoosp in [#213](https://github.com/elcoosp/glyim/pull/213) ([48c6036](https://github.com/elcoosp/glyim/commit/48c6036e0e67ff5395264bcd015813dd63c31c04))

### Performance
- bench the corpus write and check passes by @elcoosp in [#218](https://github.com/elcoosp/glyim/pull/218) ([0e7ec6e](https://github.com/elcoosp/glyim/commit/0e7ec6e7a124a609e28514b7176e4a5ec2545da7))
- bench the trend engine over a 256-record history by @elcoosp in [#216](https://github.com/elcoosp/glyim/pull/216) ([4c982ac](https://github.com/elcoosp/glyim/commit/4c982ac1b3b45fc3b91e0f11d8865645be701207))
- bench the matrix gate over an 8-program history by @elcoosp in [#216](https://github.com/elcoosp/glyim/pull/216) ([3421792](https://github.com/elcoosp/glyim/commit/342179208077caa509f686d4efb4952001d2a227))
- bench the history store over 256 records by @elcoosp in [#215](https://github.com/elcoosp/glyim/pull/215) ([a78839e](https://github.com/elcoosp/glyim/commit/a78839ebb3b8f080a0e0863f0393c861b8a4cdaf))
- bench the budget check over an 8-program matrix by @elcoosp in [#215](https://github.com/elcoosp/glyim/pull/215) ([e08962d](https://github.com/elcoosp/glyim/commit/e08962dce1490f4ff93f6161ad3973755087cba1))
- bench the scaling fit over a 64-point series by @elcoosp in [#214](https://github.com/elcoosp/glyim/pull/214) ([c854c5d](https://github.com/elcoosp/glyim/commit/c854c5d9b38443fba183fcbc593526c9869091bf))
- bench load+diff+render over 4096-sample reports by @elcoosp in [#213](https://github.com/elcoosp/glyim/pull/213) ([dd89b1a](https://github.com/elcoosp/glyim/commit/dd89b1a187c29579b76fc275f449a25f3282cbdd))

### Refactor
- destructure trend endpoints without expect by @elcoosp in [#218](https://github.com/elcoosp/glyim/pull/218) ([6fbb393](https://github.com/elcoosp/glyim/commit/6fbb39343bedc0b591b5022638e098a1543132a9))
- turn the runner expect into a plan error by @elcoosp in [#218](https://github.com/elcoosp/glyim/pull/218) ([fa1970d](https://github.com/elcoosp/glyim/commit/fa1970d86dfdd19db0ed695eaeebd2de72d2b2af))
- fail latest_baseline with BaselineUnavailable by @elcoosp in [#216](https://github.com/elcoosp/glyim/pull/216) ([0681baa](https://github.com/elcoosp/glyim/commit/0681baacaf515b921ac1384aa579c4938622eb90))

### Testing
- pin the corpus commit and check contracts by @elcoosp in [#218](https://github.com/elcoosp/glyim/pull/218) ([726a270](https://github.com/elcoosp/glyim/commit/726a270f793b717929d1a612bdca8a6e47114265))
- mount the corpus-dir test module by @elcoosp in [#218](https://github.com/elcoosp/glyim/pull/218) ([3c7f616](https://github.com/elcoosp/glyim/commit/3c7f616c58d1693b7ac4ddc9425bb7151752a9df))
- expect BaselineUnavailable from the history seam by @elcoosp in [#216](https://github.com/elcoosp/glyim/pull/216) ([f28dc3c](https://github.com/elcoosp/glyim/commit/f28dc3c9f6b9845873e1dc5fc402d19268f1265e))
- pin trend window, drift, and honesty contracts by @elcoosp in [#216](https://github.com/elcoosp/glyim/pull/216) ([da96043](https://github.com/elcoosp/glyim/commit/da9604328f1a676cd84c98ff742fab0a0589c27c))
- pin matrix rows, geo-mean budget, and seam contracts by @elcoosp in [#216](https://github.com/elcoosp/glyim/pull/216) ([9d53388](https://github.com/elcoosp/glyim/commit/9d533880a490fa8d0e27c0b1986f6d59259fbffd))
- mount the matrix and trend test modules by @elcoosp in [#216](https://github.com/elcoosp/glyim/pull/216) ([cd13822](https://github.com/elcoosp/glyim/commit/cd13822a19a7dbb3f73756b1d5b94fca8b8a7039))
- pin history record, store, and baseline contracts by @elcoosp in [#215](https://github.com/elcoosp/glyim/pull/215) ([755193f](https://github.com/elcoosp/glyim/commit/755193fa655ba09f97f3ecf5ea4101ec63f70ffd))
- pin budget gate file, verdict, and wiring contracts by @elcoosp in [#215](https://github.com/elcoosp/glyim/pull/215) ([ae5bbb0](https://github.com/elcoosp/glyim/commit/ae5bbb07093584e91aea7cbc1d3b81ca0a2e5729))
- mount the budget and history test modules by @elcoosp in [#215](https://github.com/elcoosp/glyim/pull/215) ([87a04d8](https://github.com/elcoosp/glyim/commit/87a04d8be651a4aef098b392631ae035907f4905))
- pin the scaling fit and wiring contracts by @elcoosp in [#214](https://github.com/elcoosp/glyim/pull/214) ([4e1c45a](https://github.com/elcoosp/glyim/commit/4e1c45af27a40af9e8d8535fd0497d66dbda0f31))
- mount the scaling test module by @elcoosp in [#214](https://github.com/elcoosp/glyim/pull/214) ([980c4a1](https://github.com/elcoosp/glyim/commit/980c4a1ced0741c03e17ff3c7ab1279a9521ab84))
- mount diff test module by @elcoosp in [#213](https://github.com/elcoosp/glyim/pull/213) ([6b494b5](https://github.com/elcoosp/glyim/commit/6b494b573f3a12007d74e703685eeafa42b9d727))
- pin diff verdicts, gate, and store round-trips by @elcoosp in [#213](https://github.com/elcoosp/glyim/pull/213) ([28fe90f](https://github.com/elcoosp/glyim/commit/28fe90f4ba5e355e735657bd45e1c81474d1b8be))

### build
- register the corpus_dir bench by @elcoosp in [#218](https://github.com/elcoosp/glyim/pull/218) ([0ace16c](https://github.com/elcoosp/glyim/commit/0ace16cb7924864b18c4761b65eea6d3d04b4eee))
- register the matrix_engine and trend_engine benches by @elcoosp in [#216](https://github.com/elcoosp/glyim/pull/216) ([9a2feee](https://github.com/elcoosp/glyim/commit/9a2feeeb8394bbb0474010ca935010f1fc742d38))
- add toml dep and register budget/history benches by @elcoosp in [#215](https://github.com/elcoosp/glyim/pull/215) ([d11ffc5](https://github.com/elcoosp/glyim/commit/d11ffc5553d6ca1a273e70c64a2aec6845609edc))
- register the scaling_engine bench by @elcoosp in [#214](https://github.com/elcoosp/glyim/pull/214) ([20a6767](https://github.com/elcoosp/glyim/commit/20a67675d2feae8f2937b3d001a831ed76dbe4ce))
- register diff_engine bench and tempfile dev-dep by @elcoosp in [#213](https://github.com/elcoosp/glyim/pull/213) ([8df2f2f](https://github.com/elcoosp/glyim/commit/8df2f2fd522c005b66948fc2d0ee2df9d302ffb8))
**Full Changelog**: https://github.com/elcoosp/glyim/compare/v0.10.0...v0.10.1
