use std::sync::Arc;

use crate::config::Config;
use crate::rollup::{GIORequest, GIOResponse};
use crate::utils;
use actix_web::http::header::ContentType;
use actix_web::web;
use actix_web::web::{Bytes, BytesMut};
use actix_web::{middleware::Logger, App, HttpResponse, HttpServer};
use cid::Cid;
use futures::StreamExt;
use ipfs_api_backend_hyper::{IpfsApi, IpfsClient, TryFromUri};
use sha3::{Digest, Sha3_256};
use std::io::Cursor;
use tokio::sync::Notify;

const CURRENT_STATE_CID: u16 = 0x20;
const SET_STATE_CID: u16 = 0x21;
const METADATA: u16 = 0x22;
const KECCAK256_NAMESPACE: u16 = 0x23;
const EXTERNALIZE_STATE: u16 = 0x24;
const IPFS_GET_BLOCK: u16 = 0x25;
const HINT: u16 = 0x26;

/// Create new instance of http server
pub fn create_server(config: &Config) -> std::io::Result<actix_server::Server> {
    let server = HttpServer::new(move || {
        App::new()
            .wrap(Logger::default())
            .service(open_state)
            .service(commit_state)
            .service(delete_state)
            .service(set_state)
            .service(get_state)
            .service(get_metadata)
            .service(get_data)
            .service(ipfs_get)
            .service(ipfs_put)
            .service(ipfs_has)
            .service(hint)
            .service(get_app)
    })
    .bind((config.http_address.as_str(), config.http_port))?
    .run();
    Ok(server)
}

/// Create and run new instance of http server
pub async fn run(config: &Config, server_ready: Arc<Notify>) -> std::io::Result<()> {
    log::info!("starting http dispatcher http service!");
    let server = create_server(config)?;
    server_ready.notify_one();
    server.await
}

// Deletes state with a particular key
#[actix_web::delete("/delete_state/{key}")]
async fn delete_state(key: web::Path<String>) -> HttpResponse {
    let client = IpfsClient::from_str("http://127.0.0.1:5001").unwrap();
    let key_path = format!("/state/{}", key.into_inner());
    match client.files_rm(&key_path, true).await {
        Ok(_) => HttpResponse::Ok().finish(),
        Err(_) => HttpResponse::InternalServerError().finish(),
    }
}

// Sets state with a particular key
#[actix_web::post("/set_state/{key}")]
async fn set_state(key: web::Path<String>, body: Bytes) -> HttpResponse {
    let client = IpfsClient::from_str("http://127.0.0.1:5001").unwrap();
    let base_path = "/state";
    let _ = client.files_mkdir(base_path, true).await;
    let key_path = format!("{}/{}", base_path, key.into_inner());

    let reader = Cursor::new(body);

    match client.files_write(&key_path, true, true, reader).await {
        Ok(_) => HttpResponse::Ok().finish(),
        Err(_) => HttpResponse::InternalServerError().finish(),
    }
}

// Receives state with a particular key
#[actix_web::get("/get_state/{key}")]
async fn get_state(key: web::Path<String>) -> HttpResponse {
    let client = IpfsClient::from_str("http://127.0.0.1:5001").unwrap();
    let key_path = format!("/state/{}", key.into_inner());

    let stream = client.files_read(&key_path);
    let result = stream
        .fold(BytesMut::new(), |mut acc, item| async move {
            match item {
                Ok(chunk) => {
                    acc.extend_from_slice(&chunk);
                    acc
                }
                Err(_) => acc,
            }
        })
        .await;

    HttpResponse::Ok()
        .content_type("application/octet-stream")
        .body(result.freeze())
}

#[actix_web::get("/get_app")]
async fn get_app() -> HttpResponse {
    let mut hasher = Sha3_256::new();
    hasher.update("lambada-app".as_bytes());
    let hash_result = hasher.finalize();
    let gio_request = GIORequest {
        domain: METADATA,
        payload: format!("0x{}", hex::encode(hash_result)),
    };

    let client = utils::create_client();

    //Request for getting state_cid from rollup_http_server qio request
    let req = hyper::Request::builder()
        .method(hyper::Method::POST)
        .header(hyper::header::CONTENT_TYPE, "application/json")
        .uri("http://127.0.0.1:5004/gio")
        .body(utils::body_bytes(
            serde_json::to_string(&gio_request).unwrap(),
        ))
        .expect("gio request");
    match client.request(req).await {
        Ok(gio_response) => {
            let gio_response = serde_json::from_slice::<GIOResponse>(
                &utils::response_to_bytes(gio_response)
                    .await
                    .expect("error get response from rollup_http_server qio request"),
            )
            .unwrap();

            let endpoint = "http://127.0.0.1:5001".to_string();
            let cid = Cid::try_from(hex::decode(&gio_response.response[2..]).unwrap()).unwrap();

            // Updates new state using cid received from rollup_http_server qio request
            let client = IpfsClient::from_str(&endpoint).unwrap();

            client.files_rm("/app", true).await.unwrap();
            client
                .files_cp(&("/ipfs/".to_string() + &cid.to_string()), "/app")
                .await
                .unwrap();

            HttpResponse::Ok()
                .append_header(ContentType::octet_stream())
                .body(cid.to_string())
        }
        Err(e) => {
            log::error!("failed to handle open_state request: {}", e);
            HttpResponse::BadRequest().body(format!("Failed to handle open_state request: {}", e))
        }
    }
}

// Receives state with a particular key
#[actix_web::get("/open_state")]
async fn open_state() -> HttpResponse {
    let gio_request = GIORequest {
        domain: CURRENT_STATE_CID,
        payload: "0x".to_string(),
    };

    let client = utils::create_client();

    //Request for getting state_cid from rollup_http_server qio request
    let req = hyper::Request::builder()
        .method(hyper::Method::POST)
        .header(hyper::header::CONTENT_TYPE, "application/json")
        .uri("http://127.0.0.1:5004/gio")
        .body(utils::body_bytes(
            serde_json::to_string(&gio_request).unwrap(),
        ))
        .expect("gio request");
    match client.request(req).await {
        Ok(gio_response) => {
            let gio_response = serde_json::from_slice::<GIOResponse>(
                &utils::response_to_bytes(gio_response)
                    .await
                    .expect("error get response from rollup_http_server qio request"),
            )
            .unwrap();

            let endpoint = "http://127.0.0.1:5001".to_string();
            let client = IpfsClient::from_str(&endpoint).unwrap();
            let cid = Cid::try_from(hex::decode(&gio_response.response[2..]).unwrap()).unwrap();

            // Updates new state using cid received from rollup_http_server qio request
            client
                .files_cp(&("/ipfs/".to_string() + &cid.to_string()), "/state-new")
                .await
                .unwrap();
            client.files_rm("/state-new/previous", true).await.unwrap();
            client
                .files_cp(
                    &("/ipfs/".to_string() + &cid.to_string()),
                    "/state-new/previous",
                )
                .await
                .unwrap();
            client.files_rm("/state", true).await.unwrap();
            client.files_mv("/state-new", "/state").await.unwrap();

            HttpResponse::Ok()
                .append_header(ContentType::octet_stream())
                .body(Vec::new())
        }
        Err(e) => {
            log::error!("failed to handle open_state request: {}", e);
            HttpResponse::BadRequest().body(format!("Failed to handle open_state request: {}", e))
        }
    }
}

#[actix_web::get("/commit_state")]
async fn commit_state() -> HttpResponse {
    let endpoint = "http://127.0.0.1:5001".to_string();
    let client = IpfsClient::from_str(&endpoint).unwrap();
    let cid = client.files_stat("/state").await.unwrap().hash;
    let cid = Cid::try_from(cid).unwrap();
    let cid_bytes = cid.to_bytes();

    let gio_request = GIORequest {
        domain: SET_STATE_CID,
        payload: format!("0x{}", hex::encode(cid_bytes)),
    };
    let client = utils::create_client();

    // rollup_http_server gio request with cid received from /state
    let req = hyper::Request::builder()
        .method(hyper::Method::POST)
        .header(hyper::header::CONTENT_TYPE, "application/json")
        .uri("http://127.0.0.1:5004/gio")
        .body(utils::body_bytes(
            serde_json::to_string(&gio_request).unwrap(),
        ))
        .expect("gio request");

    match client.request(req).await {
        Ok(gio_response) => {
            let _gio_response = serde_json::from_slice::<GIOResponse>(
                &utils::response_to_bytes(gio_response)
                    .await
                    .expect("error get response from rollup_http_server qio request"),
            )
            .unwrap();

            HttpResponse::Ok()
                .append_header(ContentType::octet_stream())
                .body(Vec::new())
        }
        Err(e) => {
            log::error!("failed to handle commit_state request: {}", e);
            HttpResponse::BadRequest().body(format!("Failed to handle commit_state request: {}", e))
        }
    }
}

#[actix_web::get("/metadata/{text}")]
async fn get_metadata(text: web::Path<String>) -> HttpResponse {
    let mut hasher = Sha3_256::new();
    hasher.update(text.as_bytes());
    let hash_result = hasher.finalize();

    let gio_request = GIORequest {
        domain: METADATA,
        payload: format!("0x{}", hex::encode(hash_result)),
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

    match client.request(req).await {
        Ok(gio_response) => {
            let gio_response = serde_json::from_slice::<GIOResponse>(
                &utils::response_to_bytes(gio_response)
                    .await
                    .expect("error get response from rollup_http_server qio request"),
            )
            .unwrap();

            HttpResponse::Ok()
                .append_header(ContentType::octet_stream())
                .body(hex::decode(&gio_response.response[2..]).unwrap())
        }
        Err(e) => {
            log::error!("failed to handle get_metadata request: {}", e);
            HttpResponse::BadRequest().body(format!("Failed to handle get_metadata request: {}", e))
        }
    }
}

#[actix_web::put("/ipfs/put/{cid}")]
async fn ipfs_put(content: Bytes, _cid: web::Path<String>) -> HttpResponse {
    let gio_request = GIORequest {
        domain: EXTERNALIZE_STATE,
        payload: format!("0x{}", hex::encode(content)),
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

    match client.request(req).await {
        Ok(gio_response) => {
            let gio_response = serde_json::from_slice::<GIOResponse>(
                &utils::response_to_bytes(gio_response)
                    .await
                    .expect("error get response from rollup_http_server gio request"),
            )
            .unwrap();

            HttpResponse::Ok()
                .append_header(ContentType::octet_stream())
                .body(hex::decode(&gio_response.response[2..]).unwrap())
        }
        Err(e) => {
            log::error!("failed to handle ipfs_put request: {}", e);
            HttpResponse::BadRequest().body(format!("Failed to handle ipfs_put request: {}", e))
        }
    }
}

#[actix_web::head("/ipfs/has/{cid}")]
async fn ipfs_has(_cid: web::Path<String>) -> HttpResponse {
    HttpResponse::new(actix_web::http::StatusCode::from_u16(200).unwrap())
}

#[actix_web::get("/ipfs/get/{cid}")]
async fn ipfs_get(cid: web::Path<String>) -> HttpResponse {
    let cid = cid.into_inner();
    let gio_request = GIORequest {
        domain: IPFS_GET_BLOCK,
        payload: format!("0x{}", hex::encode(Cid::try_from(cid).unwrap().to_bytes())),
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

    match client.request(req).await {
        Ok(gio_response) => {
            let gio_response = serde_json::from_slice::<GIOResponse>(
                &utils::response_to_bytes(gio_response)
                    .await
                    .expect("error get response from rollup_http_server gio request"),
            )
            .unwrap();

            HttpResponse::Ok()
                .append_header(ContentType::octet_stream())
                .body(hex::decode(&gio_response.response[2..]).unwrap())
        }
        Err(e) => {
            log::error!("failed to handle ipfs_put request: {}", e);
            HttpResponse::BadRequest().body(format!("Failed to handle ipfs_put request: {}", e))
        }
    }
}

#[actix_web::get("/get_data/{namespace}/{data_id}")]
async fn get_data(path: web::Path<(String, String)>) -> HttpResponse {
    let (namespace, data_id) = path.into_inner();
    let data_id_as_bytes = data_id.as_bytes();

    if !namespace.eq("keccak256") {
        log::error!("failed to handle get_data request: namespace should be keccak256");
        return HttpResponse::BadRequest()
            .body("Failed to handle get_data request: namespace should be keccak256");
    }

    let gio_request = GIORequest {
        domain: KECCAK256_NAMESPACE,
        payload: format!("0x{}", hex::encode(data_id_as_bytes)),
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

    match client.request(req).await {
        Ok(gio_response) => {
            let gio_response = serde_json::from_slice::<GIOResponse>(
                &utils::response_to_bytes(gio_response)
                    .await
                    .expect("error get response from rollup_http_server qio request"),
            )
            .unwrap();

            HttpResponse::Ok()
                .append_header(ContentType::octet_stream())
                .body(hex::decode(&gio_response.response[2..]).unwrap())
        }
        Err(e) => {
            log::error!("failed to handle get_data request: {}", e);
            HttpResponse::BadRequest().body(format!("Failed to handle get_data request: {}", e))
        }
    }
}

#[actix_web::get("/hint/{what}")]
async fn hint(what: web::Path<String>) -> HttpResponse {
    let what = what.into_inner();
    let gio_request = GIORequest {
        domain: HINT,
        payload: format!("0x{}", hex::encode(what)),
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

    match client.request(req).await {
        Ok(gio_response) => {
            let gio_response = serde_json::from_slice::<GIOResponse>(
                &utils::response_to_bytes(gio_response)
                    .await
                    .expect("error get response from rollup_http_server gio request"),
            )
            .unwrap();

            HttpResponse::Ok()
                .append_header(ContentType::octet_stream())
                .body(hex::decode(&gio_response.response[2..]).unwrap())
        }
        Err(e) => {
            log::error!("failed to handle hint request: {}", e);
            HttpResponse::BadRequest().body(format!("Failed to handle hint request: {}", e))
        }
    }
}
