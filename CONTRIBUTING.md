# Contributing
[![PRs Welcome](https://img.shields.io/badge/PRs-welcome-brightgreen.svg?style=flat-square)](https://makeapullrequest.com)

Thanks for taking an interest in `kubernetes-mock`! Together we can make sure that more projects consider Rust for cloud-native and Kubernetes software.

Kubernetes-Mock is an open-source Rust library and all suggestions are welcome. There are several areas that still need work, they are listed in the [README](README.md) - any help with these, or any other area you think needs work, will be sincerely appreciated and given the time it needs to become part of this project.

## Rules
* Code should pass `cargo clippy -- -D clippy::all -D clippy::pedantic` (`make clippy`).
* Design should be discussed in issues/PRs before being implemented - we should give people a chance to make their voices heard.
* We encourage newcomers and new contributors, the goal of this library is to make it easier to help people use Rust in their projects, whether they're new or experienced with Rust or Cloud Native code. Please refer to the [Rust Code of Conduct](https://www.rust-lang.org/policies/code-of-conduct) and [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).

## First Contributions
**Working on your first Pull Request?** You can learn how from this *free* series [How to Contribute to an Open Source Project on GitHub](https://kcd.im/pull-request)

First time contributors may want to help with adding more tests for `mocker.expect()` (and `mocker.run()`) covering panics, no API calls in the closure, too many API calls in the closure, unserializable objects - it'd be nice to have one for every error in the docs (link to come when crate is published).
We also welcome exploratory PRs, for example testing how unordered API calls might work.

## Reporting Bugs
Please see [SECURITY.md](SECURITY.md)

## Suggesting features or enhancements
Feel free to raise an issue for improvements, please highlight what features/changes you would like, and provide a mock code example showing how it would be used.
Please remember that this library is specifically for mocking the Kubernetes API, features should align with this goal and not introduce new scope to the project.

## PR review
I'll look at them when I have time :)
(should be relatively quickly, but if I don't respond in a week, ping @Nabushika)
