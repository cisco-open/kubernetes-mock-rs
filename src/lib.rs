// Copyright 2023 Cisco Systems, Inc. and its affiliates
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.
//
// SPDX-License-Identifier: Apache-2.0

//! Mock Kubernetes client in Rust

use k8s_openapi::apimachinery::pkg::apis::meta::v1::WatchEvent;
use k8s_openapi::http::{Request, Response};
use kube::core::ObjectList;
use kube::Client;

use hyper::Body;
use std::future::Future;
use std::pin::pin;
use std::time::Duration;
use tower_test::mock::{self, Handle};

const DEFAULT_NS: &str = "default";
const API_TIMEOUT_SECS: u64 = 1;
const TIMEOUT_DURATION: Duration = Duration::from_secs(API_TIMEOUT_SECS);

/// The main mocker struct. Holds all the information about expected API calls, and provides
/// functionality to set up expected requests and responses before testing with `.run()`.
pub struct KubernetesMocker {
    handle: Handle<Request<Body>, Response<Body>>,
    // Vec of (expected request, response)
    // This allows us to represent the invariant that the number of
    // requests/responses is the same (rather than having two vecs).
    expected_requests: Vec<(Request<Body>, MockApiResponse)>,
}

/// Returns (kube client, mock struct).
/// Use the kubernetes client as normal, both during calls to `mocker.expect()` and then during
/// `mocker.run()`.
///
/// # Examples
/// ```
/// # use kubernetes_mock::*;
/// # use kube::{Api, api::ListParams};
/// # use k8s_openapi::api::core::v1::Node;
/// # #[tokio::main]
/// # async fn main() {
/// let (client, mut mocker) = make_mocker();
/// let api: Api<Node> = Api::all(client);
/// mocker.expect(|| async {
///     let nodes = api.list(&ListParams::default()).await;
///   },
///   MockReturn::List(&[Node::default()]),
/// ).await.unwrap();
/// // ...
/// let handle = tokio::spawn(mocker.run());
/// api.list(&ListParams::default()).await;
/// handle.await.unwrap().unwrap(); // Assert tests pass with `unwrap()`.
/// # }
/// ```
#[must_use]
pub fn make_mocker() -> (Client, KubernetesMocker) {
    let (mock_service, handle) = mock::pair::<Request<Body>, Response<Body>>();
    let client = Client::new(mock_service, DEFAULT_NS);
    let mocker = KubernetesMocker {
        handle,
        expected_requests: vec![],
    };

    (client, mocker)
}

/// Represents errors generated during `mocker.expect()`.
#[derive(thiserror::Error, Debug)]
pub enum MockError {
    /// Serde failed to serialize the mock response.
    #[error("serde_json failed to serialize mock response")]
    Serde(#[from] serde_json::Error),

    /// The given closure did not make an API request.
    #[error("expected a request in closure passed to KubernetesMocker::expect()")]
    NoRequest,

    /// The given closure ran for longer than expected (uses internal `API_TIMEOUT_SECS` -
    /// currently 1).
    #[error("expected_api_call timed out")]
    Timeout,

    /// The given closure panicked during execution.
    #[error("expected_api_call panicked")]
    Panicked,

    /// If expecting a `client.watch()` call, you must provide at least one [`WatchEvent`].
    /// TODO: maybe this isn't necessary?
    #[error("Watch events list must contain at least one WatchEvent")]
    WatchNoItems,

    /// Failed to turn the serialized bytes into an [`k8s_openapi::http::Response`].
    /// (This error should be impossible in practice?)
    #[error("failed to create an http::Response from the serialized bytes")]
    HttpBody(#[from] k8s_openapi::http::Error),
}

/// Represents possible errors during `mocker.run()`.
#[derive(thiserror::Error, Debug)]
pub enum MockRunError {
    /// The mocker had more `expect()` calls than API calls received during `run()`.
    #[error(
        "Mock API received too few API calls. Expected {expected} but only received {received}"
    )]
    TooFewApiCalls { expected: usize, received: usize },

    /// The mocker had more API calls than calls to `expect()`. The `call` field holds a vector of
    /// all extraneous calls received.
    #[error("Mock API received too many API calls. Expected: {expected}, received {received}, call {call:#?}")]
    TooManyApiCalls {
        expected: usize,
        received: usize,
        call: Vec<Request<Body>>,
    },

    /// The mocker received an API call that's different to the corresponding expected API call at
    /// index `idx`.
    #[error("Mock API received a different call at {idx} than expected. Expected: {expected:#?}, received: {received:#?}")]
    IncorrectApiCall {
        received: Request<Body>,
        expected: Request<Body>,
        idx: usize,
    },
}

/// An enum to represent the possible return values from the API.
pub enum MockReturn<'a, T: kube::api::Resource + serde::Serialize> {
    /// Return a single item. This should be used as the response for `api.get()` and
    /// `api.create()` calls.
    Single(T),
    /// Return multiple items in a list. This should be used for `api.list()` calls.
    List(&'a [T]),
    /// Return [`WatchEvent`]s, with the option of adding delays in between. This should be used for
    /// `api.watch()` calls.
    Watch(&'a [MockWatch<T>]),
    /// Catch-all for anything else - if it's not a [`kube::Resource`] or serializable, you can use
    /// this to return whatever bytes you want.
    Raw(Vec<u8>),
}

/// Used to represent a stream of `api.watch()` events as a list.
pub enum MockWatch<T: kube::api::Resource + serde::Serialize> {
    /// Wait for this long before sending the next [`WatchEvent`]. This keeps the watch socket
    /// open, so feel free to use this at the end of [`MockReturn::Watch`] lists to stop a
    /// controller failing due to the watch call being ended, and creating a new, unexpected API
    /// call to set it up again.
    ///
    /// Note that `mocker.run()` waits for all of these to finish before returning, so don't make
    /// it too long!
    Wait(Duration),
    /// Send a Kubernetes [`WatchEvent`] to whatever called `api.watch()`.
    Event(WatchEvent<T>),
}

#[derive(Debug)]
enum MockApiWatchResponse {
    Wait(Duration),
    Event(Vec<u8>),
}

enum MockApiResponse {
    Single(Vec<u8>),
    Stream(Vec<MockApiWatchResponse>),
}

impl KubernetesMocker {
    /// `mocker.expect()` - takes a closure producing a future and what to return when receiving
    /// the API call given by the closure.
    /// Each closure should only have 1 API call. More will be ignored, fewer will return an error.
    ///
    /// Will return an error on panic, timeout, or if the closure does not make a request - feel
    /// free to use `unwrap()` liberally!
    ///
    /// Returns `Ok(&mut self)` on success, meaning you can chain
    /// `.expect().await.unwrap().expect().await.unwrap()`...
    ///
    /// # Errors
    /// * [`MockError::Serde`] - `serde_json` failed to serialize mock response
    /// * [`MockError::NoRequest`] - closure passed in did not make an API request.
    /// * [`MockError::Timeout`] - closure passed in timed out.
    /// * [`MockError::Panicked`] - closure passed in panicked.
    /// * [`MockError::WatchNoItems`] - Watch events list must contain at least one `WatchEvent`.
    /// * [`MockError::HttpBody`] - failed to create an `http::Response` from the serialized bytes.
    ///
    /// # Examples
    /// ```
    /// # use kubernetes_mock::*;
    /// # use kube::{Api, api::{ListParams, PostParams}};
    /// # use k8s_openapi::api::core::v1::Node;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let (client, mut mocker) = make_mocker();
    /// let api: Api<Node> = Api::all(client);
    /// mocker.expect(|| async {
    ///       api.list(&ListParams::default()).await.unwrap();
    ///     },
    ///     MockReturn::List(&[Node::default()]),
    ///   ).await.unwrap()
    ///   .expect(|| async {
    ///       api.create(&PostParams::default(), &Node::default()).await.unwrap();
    ///     },
    ///     MockReturn::Single(Node::default()),
    ///   ).await.unwrap();
    /// # }
    /// ```
    ///
    pub async fn expect<'a, Fut, T, F>(
        &mut self,
        expected_api_call: F,
        result: MockReturn<'a, T>,
    ) -> Result<&mut Self, MockError>
    where
        Fut: Future<Output = ()> + Send,
        T: kube::Resource + serde::Serialize + Clone,
        F: FnOnce() -> Fut,
    {
        let response = match result {
            MockReturn::Single(ref item) => serde_json::to_vec(&item)?,
            MockReturn::List(items) => serde_json::to_vec(&list(items))?,
            MockReturn::Raw(ref vec) => vec.clone(),
            // If we're mocking a `watch`, just send the first event back to the closure
            MockReturn::Watch(items) => {
                let first_event = match items
                    .iter()
                    .find(|i| matches!(i, MockWatch::Event(_)))
                    .ok_or(MockError::WatchNoItems)?
                {
                    MockWatch::Wait(_) => unreachable!(), // Filtered out earlier by the `find`
                    MockWatch::Event(e) => e,
                };
                serde_json::to_vec(first_event)?
            }
        };
        let fut = tokio::time::timeout(TIMEOUT_DURATION, expected_api_call());
        let handle_request = async {
            let (request, send) =
                tokio::time::timeout(TIMEOUT_DURATION, self.handle.next_request())
                    .await
                    .ok()
                    .flatten()
                    .ok_or(MockError::NoRequest)?;

            // Looks intimidating, but let's step through this
            let api_return = match result {
                // If this is a `watch` request, we need to turn the Vec<MockWatch> into
                // Vec<MockApiWatchResponse>. This is represented by Stream()
                MockReturn::Watch(items) => MockApiResponse::Stream(
                    items
                        .iter()
                        .map(|e| match e {
                            MockWatch::Wait(duration) => Ok(MockApiWatchResponse::Wait(*duration)),
                            MockWatch::Event(e) => {
                                let mut vec = serde_json::to_vec(e)?;
                                vec.push(b'\n');
                                Ok(MockApiWatchResponse::Event(vec))
                            }
                        })
                        // Collect as a result, to allow collecting errors from serde inside the
                        // loop. We then return if there are any errors, and are left with a
                        // Vec<MockApiResponse>.
                        .collect::<Result<Vec<_>, MockError>>()?,
                ),
                // Otherwise, we can use the existing `response`.
                _ => MockApiResponse::Single(response.clone()),
            };
            self.expected_requests.push((request, api_return));
            send.send_response(Response::builder().body(Body::from(response))?);
            Ok::<(), MockError>(())
        };
        let (expected_handle, handle_request) = futures::future::join(fut, handle_request).await;
        handle_request?;
        expected_handle
            //.map_err(|_| MockError::Panicked)? // if using tokio::task::spawn (needs 'static)
            .map_err(|_| MockError::Timeout)?;
        Ok(self)
    }

    /// `KubernetesMocker::run()` - produces a future which will compare the received API calls
    /// past this point, and compare them to the ones received during the `expect()` calls.
    ///
    /// # Errors
    /// * [`MockRunError::TooFewApiCalls`] - it did not receive as many API calls as `expect()` got.
    /// * [`MockRunError::TooManyApiCalls`] - received at least 1 more API call that was not expected.
    /// * [`MockRunError::IncorrectApiCall`] - an API call did not match the one received in the
    /// corresponding `expect()` call.
    ///
    /// Should be run with [`tokio::task::spawn`] so it runs concurrently with whatever is being tested.
    ///
    /// # Examples
    /// See examples for [`make_mocker()`].
    ///
    /// # Panics
    /// Should not panic, if an HTTP body fails to be made it should be presented as an error
    /// during `expect()`. Still uses `unwrap()` for brevity, and to avoid a duplicate of
    /// [`MockError::HttpBody`] in [`MockRunError`].
    #[allow(clippy::similar_names)]
    pub async fn run(self) -> Result<(), MockRunError> {
        let KubernetesMocker {
            handle,
            expected_requests,
        } = self;
        let mut handle = pin!(handle);
        let expected_num_requests = expected_requests.len();
        let mut watch_handles = vec![];

        for (i, (expected_api_call, result)) in expected_requests.into_iter().enumerate() {
            println!("api call {i}, expected {}", expected_api_call.uri());
            let (request, send) = tokio::time::timeout(TIMEOUT_DURATION, handle.next_request())
                .await
                .ok()
                .flatten()
                .ok_or(MockRunError::TooFewApiCalls {
                    expected: expected_num_requests,
                    received: i,
                })?;
            println!("Got request {request:#?}");

            // Need to deconstruct/reconstruct to get body bytes without consuming the requests
            let (eparts, ebody) = expected_api_call.into_parts();
            let (aparts, abody) = request.into_parts();
            let ebody = hyper::body::to_bytes(ebody).await.unwrap();
            let abody = hyper::body::to_bytes(abody).await.unwrap();

            let same_as_expected =
                eparts.uri == aparts.uri && ebody == abody && eparts.method == aparts.method;

            let expected_api_call = Request::from_parts(eparts, ebody.into());
            let request = Request::from_parts(aparts, abody.into());

            if !same_as_expected {
                println!("NOT SAME AS EXPECTED");
                return Err(MockRunError::IncorrectApiCall {
                    received: request,
                    expected: expected_api_call,
                    idx: i,
                });
            }

            match result {
                MockApiResponse::Single(resp) => {
                    send.send_response(Response::builder().body(Body::from(resp)).unwrap());
                }
                MockApiResponse::Stream(mut stream) => {
                    // spawn a new future, feed items from stream into a body
                    stream.reverse();
                    let (mut sender, body) = Body::channel(); // Need to use a channel to
                                                              // continuously send data
                    let time = std::time::Instant::now();
                    let fut = async move {
                        while let Some(watch_response) = stream.pop() {
                            println!(
                                "Sending event {watch_response:?} at {:?}",
                                std::time::Instant::now().duration_since(time)
                            );
                            match watch_response {
                                MockApiWatchResponse::Wait(duration) => {
                                    tokio::time::sleep(duration).await;
                                }
                                MockApiWatchResponse::Event(e) => {
                                    sender.send_data(e.into()).await.unwrap();
                                    println!("Sent data");
                                }
                            }
                        }
                    };
                    watch_handles.push(tokio::task::spawn(fut));
                    send.send_response(Response::new(body));
                }
            };
        }
        let mut extra_requests = Vec::new();
        while let Ok(Some((request, _))) =
            tokio::time::timeout(TIMEOUT_DURATION, handle.next_request()).await
        {
            extra_requests.push(request);
        }
        // Close all watches (we do this after waiting for extra requests, to make sure closing the
        // watch does not trigger extra requests that we fail on).
        futures::future::join_all(watch_handles.into_iter()).await;
        if extra_requests.is_empty() {
            Ok(())
        } else {
            Err(MockRunError::TooManyApiCalls {
                expected: expected_num_requests,
                received: expected_num_requests + extra_requests.len(),
                call: extra_requests,
            })
        }
    }
}

fn list<T: kube::Resource + Clone>(items: &[T]) -> ObjectList<T> {
    use kube::core::ListMeta;
    ObjectList::<T> {
        items: items.iter().map(Clone::clone).collect::<Vec<_>>(),
        metadata: ListMeta {
            resource_version: Some("1".into()),
            ..Default::default()
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mocker() {
        use k8s_openapi::api::core::v1::Node;
        use kube::{api::ListParams, Api};
        let (client, mut mocker) = make_mocker();
        let mocker_client = client.clone();
        mocker
            .expect(
                move || async {
                    let api: Api<Node> = Api::all(mocker_client);
                    let _nodes = api.list(&ListParams::default()).await;
                },
                MockReturn::List(&[Node::default()]),
            )
            .await
            // TODO: look into DSL?
            //.expect(list(Node, ListParams::default().labels("foo=bar"))))
            .unwrap();
        let spawned = tokio::spawn(mocker.run());
        let api: Api<Node> = Api::all(client);
        let _nodes = api.list(&ListParams::default()).await;
        // Need two unwraps: the first one verifies that the spawned future hasn't panicked, and
        // the second unwraps the result returned from the mocker (an error indicates a failure).
        spawned.await.unwrap().unwrap();
    }

    #[tokio::test]
    #[should_panic(expected = "TooFewApiCalls")]
    async fn test_mocker_too_few_api_calls() {
        use k8s_openapi::api::core::v1::Node;
        use kube::{api::ListParams, Api};
        let (client, mut mocker) = make_mocker();
        let mocker_client = client.clone();
        mocker
            .expect(
                move || async {
                    let api: Api<Node> = Api::all(mocker_client);
                    let _nodes = api.list(&ListParams::default()).await;
                },
                MockReturn::List(&[Node::default()]),
            )
            .await
            .unwrap();
        let spawned = tokio::spawn(mocker.run());
        //let api: Api<Node> = Api::all(client);
        //let _nodes = api.list(&ListParams::default()).await;
        spawned.await.unwrap().unwrap();
    }

    #[tokio::test]
    #[should_panic(expected = "TooManyApiCalls")]
    async fn test_mocker_too_many_api_calls() {
        use k8s_openapi::api::core::v1::Node;
        use kube::{api::ListParams, Api};
        let (client, mocker) = make_mocker();
        let spawned = tokio::spawn(mocker.run());

        let api: Api<Node> = Api::all(client);
        let _nodes = api.list(&ListParams::default()).await;
        spawned.await.unwrap().unwrap();
    }

    #[tokio::test]
    #[should_panic(expected = "IncorrectApiCall")]
    async fn wrong_api_call() {
        use k8s_openapi::api::core::v1::{Node, Pod};
        use kube::{api::ListParams, Api};
        let (client, mut mocker) = make_mocker();
        let mocker_client = client.clone();
        mocker
            .expect(
                move || async {
                    let api: Api<Node> = Api::all(mocker_client);
                    let _nodes = api.list(&ListParams::default()).await;
                },
                MockReturn::List(&[Node::default()]),
            )
            .await
            .unwrap();
        let spawned = tokio::spawn(mocker.run());
        let api: Api<Pod> = Api::all(client);
        let _pods = api.list(&ListParams::default()).await;
        spawned.await.unwrap().unwrap();
    }
}
