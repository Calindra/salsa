// Copyright Cartesi and individual authors (see AUTHORS)
// SPDX-License-Identifier: Apache-2.0
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

#[cfg(test)]
mod tests {
    use actix_server::ServerHandle;
    use http_body_util::{Empty, Full};
    use hyper::{
        body::{Bytes, Incoming},
        server::conn::http1,
        service::service_fn,
        Method, Request, Response, StatusCode,
    };
    use hyper_util::rt::TokioIo;
    use rstest::*;
    use salsa::{config::Config, http_service, utils};
    use serde::{Deserialize, Serialize};
    use serde_json::json;
    use std::{convert::Infallible, future::Future, net::SocketAddr};
    use tokio::{net::TcpListener, sync::oneshot, task};
    use tower::ServiceBuilder;
    const HOST: &str = "127.0.0.1";

    #[derive(Debug, Clone, Serialize, Deserialize)]
    struct GIORequest {
        domain: u32,
        payload: String,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct GIOResponse {
        pub response_code: u16,
        pub response: String,
    }

    async fn handle_request(req: Request<Incoming>) -> Result<Response<Full<Bytes>>, Infallible> {
        match (req.method(), req.uri().path()) {
            (&Method::POST, "/gio") => {
                let result = GIOResponse {
                    response_code: 0,
                    response: "0x".to_string(),
                };
                Ok(Response::new(utils::body_bytes(json!(result).to_string())))
            }
            _ => {
                let not_found = Response::builder()
                    .status(404)
                    .body(utils::body_bytes("404 Not Found".to_string()))
                    .unwrap();
                Ok(not_found)
            }
        }
    }

    async fn start_server(tx: oneshot::Sender<()>) {
        let addr = SocketAddr::from(([127, 0, 0, 1], 5004));
        let listener = TcpListener::bind(addr).await.unwrap();

        let _ = tx.send(());

        loop {
            let (stream, _) = listener.accept().await.unwrap();
            let io = TokioIo::new(stream);

            tokio::spawn(async move {
                let svc = service_fn(handle_request);
                let svc = ServiceBuilder::new().service(svc);

                if let Err(e) = http1::Builder::new().serve_connection(io, svc).await {
                    eprintln!("Server error: {}", e);
                }
            });
        }
    }

    #[allow(dead_code)]
    struct Context {
        lambada_address: String,
        lambada_server_handle: actix_server::ServerHandle,
    }

    impl Drop for Context {
        fn drop(&mut self) {
            // Shut down http server+
            println!("shutting down http service in drop cleanup");
        }
    }

    fn run_test_lambada_http_service(
        host: &str,
        port: u16,
    ) -> std::io::Result<Option<actix_server::ServerHandle>> {
        let http_config = Config {
            http_address: host.to_string(),
            http_port: port,
        };
        println!("Creating lambada http server");
        let server = http_service::create_server(&http_config)?;
        let server_handle = server.handle();
        println!("Spawning lambada http server");
        tokio::spawn(server);
        println!("Http lambada server spawned");
        Ok(Some(server_handle))
    }
    #[rstest]
    #[actix_web::test]
    async fn test_server() {
        let (tx, rx) = oneshot::channel();
        let server_task = task::spawn(start_server(tx));
        rx.await.expect("Server failed to start");
        let gio_request = GIORequest {
            domain: 0x100,
            payload: hex::encode(vec![0, 0, 0]),
        };
        let client = utils::create_client();

        let req = hyper::Request::builder()
            .method(hyper::Method::POST)
            .header(hyper::header::CONTENT_TYPE, "application/json")
            .uri("http://127.0.0.1:5004/gio")
            .body(utils::body_bytes(
                serde_json::to_string(&gio_request).unwrap(),
            ))
            .expect("gio request");
        client.request(req).await.unwrap();
        drop(server_task);
    }

    #[fixture]
    async fn context_future() -> Context {
        let mut server_handle: Option<ServerHandle> = None;
        let port = 5005;
        match run_test_lambada_http_service(HOST, port) {
            Ok(handle) => {
                server_handle = handle;
            }
            Err(ex) => {
                eprint!("Error instantiating rollup http service {}", ex);
            }
        };
        Context {
            lambada_address: format!("http://{}:{}", HOST, port),
            lambada_server_handle: server_handle.unwrap(),
        }
    }

    #[rstest]
    #[tokio::test]
    async fn test_server_instance_creation(
        context_future: impl Future<Output = Context>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let context = context_future.await;
        println!("Sleeping in the test... ");
        std::thread::sleep(std::time::Duration::from_secs(5));
        println!("End sleeping");
        println!("Shutting down http service");
        context.lambada_server_handle.stop(true).await;
        println!("Http server closed");
        Ok(())
    }

    #[rstest]
    #[tokio::test]
    async fn test_open_state(
        context_future: impl Future<Output = Context>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let (tx, rx) = oneshot::channel();
        let server_task = task::spawn(start_server(tx));
        rx.await.expect("Server failed to start");
        let context = context_future.await;
        let client = utils::create_client();
        let req = hyper::Request::builder()
            .method(hyper::Method::GET)
            .header(hyper::header::CONTENT_TYPE, "application/json")
            .uri(context.lambada_address.clone() + "/open_state")
            .body(Empty::<Bytes>::new())
            .expect("open_state request");
        match client.request(req).await {
            Ok(gio_response) => {
                let body = utils::response_to_bytes(gio_response)
                    .await
                    .expect("error get response from rollup_http_server qio request")
                    .to_vec();
                println!("result {:?}", String::from_utf8(body.clone()));
            }
            Err(e) => {
                println!("failed to handle gio_response request: {}", e);
            }
        }

        context.lambada_server_handle.stop(true).await;
        drop(server_task);

        Ok(())
    }

    #[rstest]
    #[tokio::test]
    async fn test_commit_state(
        context_future: impl Future<Output = Context>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let (tx, rx) = oneshot::channel();
        let server_task = task::spawn(start_server(tx));
        rx.await.expect("Server failed to start");
        let context = context_future.await;
        let client = utils::create_client();
        let req = hyper::Request::builder()
            .method(hyper::Method::GET)
            .header(hyper::header::CONTENT_TYPE, "application/json")
            .uri(context.lambada_address.clone() + "/commit_state")
            .body(Empty::<Bytes>::new())
            .expect("commit_state request");
        match client.request(req).await {
            Ok(res) => {
                let body = utils::response_to_bytes(res)
                    .await
                    .expect("error get response from rollup_http_server qio request")
                    .to_vec();
                println!("result {:?}", String::from_utf8(body.clone()));
            }
            Err(e) => {
                println!(
                    "failed to send commit_state request to lambada http server: {}",
                    e
                );
            }
        }
        context.lambada_server_handle.stop(true).await;
        drop(server_task);
        Ok(())
    }

    #[rstest]
    #[tokio::test]
    async fn test_metadata(
        context_future: impl Future<Output = Context>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let (tx, rx) = oneshot::channel();
        let server_task = task::spawn(start_server(tx));
        rx.await.expect("Server failed to start");
        let context = context_future.await;
        let client = utils::create_client();
        let req = hyper::Request::builder()
            .method(hyper::Method::GET)
            .header(hyper::header::CONTENT_TYPE, "application/json")
            .uri(context.lambada_address.clone() + "/metadata/some_test_text")
            .body(Empty::<Bytes>::new())
            .expect("metadata request");
        match client.request(req).await {
            Ok(res) => {
                let body = utils::response_to_bytes(res)
                    .await
                    .expect("error get response from rollup_http_server qio request")
                    .to_vec();
                println!("output {:?}", String::from_utf8(body.clone()));
            }
            Err(e) => {
                println!(
                    "failed to send metadata request to lambada http server: {}",
                    e
                );
            }
        }
        context.lambada_server_handle.stop(true).await;
        drop(server_task);
        Ok(())
    }

    #[rstest]
    #[tokio::test]
    async fn test_get_data(
        context_future: impl Future<Output = Context>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let (tx, rx) = oneshot::channel();
        let server_task = task::spawn(start_server(tx));
        rx.await.expect("Server failed to start");
        let context = context_future.await;

        let client = utils::create_client();
        let req = hyper::Request::builder()
            .method(hyper::Method::GET)
            .header(hyper::header::CONTENT_TYPE, "application/json")
            .uri(context.lambada_address.clone() + "/get_data/keccak256/data_id")
            .body(Empty::<Bytes>::new())
            .expect("get_data request");
        match client.request(req).await {
            Ok(res) => {
                println!("output {:?}", res);
                match res.status() {
                    StatusCode::BAD_REQUEST => {
                        let body = utils::response_to_bytes(res)
                            .await
                            .expect("error get response from rollup_http_server qio request")
                            .to_vec();
                        println!("error {:?}", String::from_utf8(body.clone()));
                        context.lambada_server_handle.stop(true).await;
                        drop(server_task);
                        panic!()
                    }
                    _ => {
                        let body = utils::response_to_bytes(res)
                            .await
                            .expect("error get response from rollup_http_server qio request")
                            .to_vec();
                        println!("output {:?}", String::from_utf8(body.clone()));
                    }
                }
            }
            Err(e) => {
                println!(
                    "failed to send get_data request to lambada http server: {}",
                    e
                );
            }
        }
        context.lambada_server_handle.stop(true).await;
        drop(server_task);
        Ok(())
    }

    #[rstest]
    #[tokio::test]
    #[should_panic]
    async fn test_get_data_fail(context_future: impl Future<Output = Context>) {
        let (tx, rx) = oneshot::channel();
        let server_task = task::spawn(start_server(tx));
        rx.await.expect("Server failed to start");
        let context = context_future.await;

        let client = utils::create_client();
        let req = hyper::Request::builder()
            .method(hyper::Method::GET)
            .header(hyper::header::CONTENT_TYPE, "application/json")
            .uri(context.lambada_address.clone() + "/get_data/namespace/data_id")
            .body(Empty::<Bytes>::new())
            .expect("get_data request");
        match client.request(req).await {
            Ok(res) => {
                println!("output {:?}", res);
                match res.status() {
                    StatusCode::BAD_REQUEST => {
                        let body = utils::response_to_bytes(res)
                            .await
                            .expect("error get response from rollup_http_server qio request")
                            .to_vec();
                        println!("error {:?}", String::from_utf8(body.clone()));
                        context.lambada_server_handle.stop(true).await;
                        drop(server_task);
                        panic!()
                    }
                    _ => {
                        let body = utils::response_to_bytes(res)
                            .await
                            .expect("error get response from rollup_http_server qio request")
                            .to_vec();
                        println!("output {:?}", String::from_utf8(body.clone()));
                    }
                }
            }
            Err(e) => {
                println!(
                    "failed to send get_data request to lambada http server: {}",
                    e
                );
            }
        }
        context.lambada_server_handle.stop(true).await;
        drop(server_task);
    }
}
