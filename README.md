# kubernetes-mock-rs

[![Rust](https://github.com/cisco-open/kubernetes-mock-rs/actions/workflows/rust.yml/badge.svg)](https://github.com/cisco-open/kubernetes-mock-rs/actions/workflows/rust.yml)

Mock [Kubernetes](https://kubernetes.io/) client for testing things that interact with the [Kubernetes API](https://kubernetes.io/docs/concepts/overview/kubernetes-api/), including [controllers](https://kubernetes.io/docs/concepts/architecture/controller/).

View documentation, examples, and more on [docs.rs](https://docs.rs/kubernetes-mock/) and [crates.io](https://crates.io/crates/kubernetes-mock)!

488 lines of code, but that's 24% doc comments.

## Features
* Very simple to mock, just make the same API calls as your program should make
* Easy setup errors with good documentation - if there are problems, it should help you solve them
* Verbose errors when tests fail, showing you exactly what went wrong
* Works with multiple versions of Kubernetes (currently v1.25, v1.21) - to change versions, please use features `v1_25` or `v1_21`.
  * Default is v1.25, if you want to use a different version please remember to use `default-features = false` in your Cargo.toml

## Example

Minimal compilable example:

```rust
use kubernetes_mock::*;
use kube::{Api, api::ListParams};
use k8s_openapi::api::core::v1::Node;
#[tokio::main]
async fn main() {
    let (client, mut mocker) = make_mocker();
    let api: Api<Node> = Api::all(client);
    mocker.expect(|| async {
        let nodes = api.list(&ListParams::default()).await;
      },
      MockReturn::List(&[Node::default()]),
    ).await.unwrap();
    // ...
    let handle = tokio::spawn(mocker.run());
    api.list(&ListParams::default()).await;
    handle.await.unwrap().unwrap(); // Assert tests pass with `unwrap()`.
    // Requires two calls to unwrap - one for tokio spawn result, the other for the mocker result
}
```

Note: `unwrap()` is not great for formatting, if you'd like to see errors pretty printed you can do the following:
```rust
if let Err(e) = handle.await.unwrap() { // still need one unwrap() for tokio spawn
    panic!("{e:#?}"); // debug pretty-print
}
```


## Still Todo

* Testing `mocker.watch()` - it works but is not tested.
  * We could use a controller to test this, to show how it works.
* More edge cases tested - functions passed to `expect()` panicking, etc.
* Explore better API design?
  * Can we let `expect()` take multiple API calls, meaning people can use one for all API calls 
* Should/could we be less restrictive (e.g. unordered API calls, allowing the same call multiple times/"more than once", allowing different parameters)?

Feel free to help, check out [CONTRIBUTING.md](CONTRIBUTING.md)!

Distributed under Apache 2.0. See [LICENSE](LICENSE) for more information.
