use http_body_util::{BodyExt, Empty};
use hyper::{body::Bytes, Request};
use hyper_rustls::HttpsConnectorBuilder;
use hyper_util::{client::legacy::Client, rt::TokioExecutor};
use openapiv3::OpenAPI;
use prettyplease::unparse as unparseAST;
use progenitor::Generator;
use serde_yml::{from_slice, from_value, Value};
use std::{env::var, error::Error, fs::write as write_file, path::PathBuf, str::FromStr};
use syn::{parse2 as parseAST, File as SyncFile};

async fn get_content_spec() -> Result<Bytes, Box<dyn Error>> {
    let endpoint =
        "https://raw.githubusercontent.com/cartesi/openapi-interfaces/v0.9.0/rollup.yaml";
    let https = HttpsConnectorBuilder::new()
        .with_native_roots()
        .expect("no native root CA certificates found")
        .https_only()
        .enable_http1()
        .build();
    let client = Client::builder(TokioExecutor::new()).build(https);
    let req = Request::builder()
        .uri(endpoint)
        .body(Empty::<Bytes>::new())?;
    let res = client.request(req).await?;
    let body = res.into_body().collect().await?.to_bytes();

    Ok(body)
}

fn remove_default_response_from_openapi(spec_val: &mut Value) -> OpenAPI {
    let paths = spec_val.get_mut("paths").unwrap().as_mapping_mut().unwrap();

    for (method, methods) in paths {
        let methods = methods.as_mapping_mut().unwrap();
        for (path, operation) in methods {
            let responses = operation
                .get_mut("responses")
                .unwrap()
                .as_mapping_mut()
                .unwrap();

            let val_default = responses.swap_remove("default");

            if let Some(val) = val_default {
                responses.insert(Value::from("400"), val.clone());
                responses.insert(Value::from("418"), val.clone());
                responses.insert(Value::from("500"), val.clone());
                responses.insert(Value::from("501"), val);
            }

            println!("cargo:warning={:#?},{:#?},{:#?}", method, path, responses);
        }
    }

    let spec: OpenAPI = from_value(spec_val.clone()).unwrap();

    println!("cargo:warning={:#?}", spec.operations().collect::<Vec<_>>());

    spec
}

#[tokio::main]
async fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let content = get_content_spec().await.unwrap();
    let spec: OpenAPI = from_slice(&content).unwrap();
    // let mut spec_val: Value = from_slice(&content).unwrap();
    let mut generator = Generator::default();
    // let spec = remove_default_response_from_openapi(&mut spec_val);
    // println!("cargo:warning={:#?}", spec);

    let x = generator.get_type_space();
    println!("cargo:warning={:#?}", x);

    let tokens = generator.generate_tokens(&spec).unwrap();
    println!("cargo:warning={:#?}", tokens);
    let ast: SyncFile = parseAST(tokens).unwrap();
    println!("cargo:warning={:#?}", ast);
    let content = unparseAST(&ast);
    println!("cargo:warning={:#?}", content);

    let out_dir = var("OUT_DIR").unwrap();
    let mut out_file = PathBuf::from_str(&out_dir).unwrap();
    out_file.push("rollup_api.rs");

    write_file(&out_file, content).unwrap();
}
