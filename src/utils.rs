use http_body_util::{BodyExt, Full};
use hyper::body::{Body, Bytes};
use hyper::http::Response;
use hyper_util::{
    client::legacy::{connect::HttpConnector, Client},
    rt::TokioExecutor,
};

pub fn create_client<T>() -> Client<HttpConnector, T>
where
    T: Body + std::marker::Send,
    T::Data: Send,
{
    Client::builder(TokioExecutor::new()).build_http()
}

pub fn body_bytes(body: String) -> Full<Bytes> {
    Full::new(Bytes::from(body))
}

pub async fn response_to_bytes<B>(response: Response<B>) -> Result<Bytes, B::Error>
where
    B: BodyExt,
{
    Ok(response.into_body().collect().await?.to_bytes())
}
