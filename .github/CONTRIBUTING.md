# Contributing

Contributions are welcome when they preserve ccvl's truth, privacy, and
portability boundaries.

1. Do not include personal application data, recipient details, source
   documents, credentials, or third-party confidential material.
2. Keep third-party code isolated and record its exact source, revision,
   copyright, and license.
3. Run `bash ./cv check` and `bash ./cv public-check` before opening a pull
   request (or use `.\cv.cmd` on Windows).
4. Explain any intentional rendering difference and include page-count evidence.

Every external pull request must contain the exact affirmation required by the
[Corbet Labs contribution policy](https://github.com/corbet-labs/.github/blob/main/CONTRIBUTING.md),
referencing the immutable
[Individual Contributor License Agreement v1.0](https://git.corbet.ch/corbet-legal/cla/src/tag/cla-v1.0/CLA.md).

## Where development happens

This repository is a one-way export of the `careervector-cli/` folder of a larger repository;
commits are made there and exported here, and releases are cut here. A pull request to this
repository is welcome and is taken in by hand (the maintainers merge it there, and the export
then carries it back), so it may be closed with a pointer to that commit.

Everything in this repository is licensed under FSL-1.1-ALv2. Do not copy code into it from a
project under another licence, and do not reference files outside this repository (a Cargo
`path` dependency, an include, a script path that leaves the repository): the export publishes
this tree and nothing else.
