//! `actix-multiresponse` intents to allow supporting multiple response/request data formats depending on the
//! `Content-Type` and `Accept` headers.
//!
//! ### Supported formats
//! - Json
//! - Protobuf
//!
//! All formats can be enabled with feature flags. At least one format should be enabled to make this library useful.
//!
//! ### Example
//! ```
//!     use prost_derive::Message;
//!     use serde_derive::{Deserialize, Serialize};
//!     use actix_multiresponse::Payload;
//!
//!     #[derive(Deserialize, Serialize, Message, Clone)]
//!     struct TestPayload {
//!         #[prost(string, tag = "1")]
//!         foo: String,
//!         #[prost(int64, tag = "2")]
//!         bar: i64,
//!     }
//!
//!     async fn responder(payload: Payload<TestPayload>) -> Payload<TestPayload> {
//!         payload
//!     }
//! ```

use crate::error::PayloadError;
use crate::headers::ContentType;

use actix_web::body::BoxBody;
use actix_web::{FromRequest, HttpRequest, HttpResponse, Responder};
use log::trace;

use std::future::Future;
use std::ops::{Deref, DerefMut};
use std::pin::Pin;

mod error;
mod headers;

#[cfg(feature = "protobuf")]
pub trait ProtobufSupport: prost::Message {}
#[cfg(not(feature = "protobuf"))]
pub trait ProtobufSupport {}

#[cfg(feature = "protobuf")]
impl<T: prost::Message> ProtobufSupport for T {}
#[cfg(not(feature = "protobuf"))]
impl<T> ProtobufSupport for T {}

#[cfg(any(feature = "json"))]
pub trait SerdeSupportDeserialize: serde::de::DeserializeOwned {}
#[cfg(not(any(feature = "json")))]
pub trait SerdeSupportDeserialize {}

#[cfg(any(feature = "json"))]
impl<T: serde::de::DeserializeOwned> SerdeSupportDeserialize for T {}
#[cfg(not(any(feature = "json")))]
impl<T> SerdeSupportDeserialize for T {}

#[cfg(any(feature = "json"))]
pub trait SerdeSupportSerialize: serde::Serialize {}
#[cfg(not(any(feature = "json")))]
pub trait SerdeSupportSerialize {}

#[cfg(any(feature = "json"))]
impl<T: serde::Serialize> SerdeSupportSerialize for T {}
#[cfg(not(any(feature = "json")))]
impl<T> SerdeSupportSerialize for T {}

/// Payload wrapper which facilitates tje (de)serialization.
/// This type can be used as both the request and response payload type.
///
/// The proper format is chosen based on the `Content-Type` and `Accept` headers.
/// When deserializing only the `Content-Type` header is used.
/// When serializing, the `Accept` header is checked first, if it is missing
/// the `Content-Type` header will be used. If both are missing the payload will
/// default to `JSON`.
///
/// # Errors
///
/// When the `Content-Type` header is not provided in the request or is invalid, this will return a HTTP 400 error.
/// If the `Content-Type` header, or `Accept` header is invalid when responding this will return a HTTP 400 error,
/// however this is *not* done if both headers are missing on response.
///
/// # Panics
///
/// If during serializing no format is enabled
#[derive(Debug)]
pub struct Payload<T: 'static + Default + Clone>(pub T);

impl<T: 'static + Default + Clone> Deref for Payload<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T: 'static + Default + Clone> DerefMut for Payload<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<T: 'static + SerdeSupportDeserialize + ProtobufSupport + Default + Clone> FromRequest
    for Payload<T>
{
    type Error = PayloadError;
    type Future = Pin<Box<dyn Future<Output = Result<Self, Self::Error>>>>;

    fn from_request(req: &HttpRequest, payload: &mut actix_web::dev::Payload) -> Self::Future {
        let req = req.clone();
        #[allow(unused)]
        let mut payload = payload.take();

        Box::pin(async move {
            match ContentType::from_request_content_type(&req) {
                #[cfg(feature = "json")]
                ContentType::Json => {
                    trace!("Received JSON payload, deserializing");
                    let json: actix_web::web::Json<T> =
                        actix_web::web::Json::from_request(&req, &mut payload).await?;
                    Ok(Self(json.clone()))
                }
                #[cfg(feature = "protobuf")]
                ContentType::Protobuf => {
                    trace!("Received Protobuf payload, deserializing");
                    let protobuf: actix_protobuf::ProtoBuf<T> =
                        actix_protobuf::ProtoBuf::from_request(&req, &mut payload).await?;
                    Ok(Self(protobuf.clone()))
                }
                _ => {
                    trace!("User did not set a valid Content-Type header");
                    Err(Self::Error::InvalidContentType)
                }
            }
        })
    }
}

impl<T: ProtobufSupport + SerdeSupportSerialize + Default + Clone> Responder for Payload<T> {
    type Body = BoxBody;

    fn respond_to(self, req: &HttpRequest) -> HttpResponse<Self::Body> {
        // Determine the response format
        // - Check if the Accepts header was set to a valid value, use that
        // - If not, check the Content-Type header, if that is valid, use that
        // - Else, default to Json
        let content_type = ContentType::from_request_accepts(req);
        let content_type = if content_type.eq(&ContentType::Other) {
            let content_type_second = ContentType::from_request_content_type(req);
            if content_type_second.eq(&ContentType::Other) {
                ContentType::default()
            } else {
                content_type_second
            }
        } else {
            content_type
        };

        match content_type {
            #[cfg(feature = "json")]
            ContentType::Json => {
                let json = actix_web::web::Json(self.0);
                json.respond_to(req).map_into_boxed_body()
            }
            #[cfg(feature = "protobuf")]
            ContentType::Protobuf => {
                let protobuf = actix_protobuf::ProtoBuf(self.0);
                protobuf.respond_to(req)
            }
            ContentType::Other => panic!("Unable to serialize. Content type to use could not be determined. Do you have at least one format enabled?"),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use prost_derive::Message;
    use serde_derive::{Deserialize, Serialize};

    #[derive(Deserialize, Serialize, Message, Clone)]
    struct TestPayload {
        #[prost(string, tag = "1")]
        foo: String,
        #[prost(int64, tag = "2")]
        bar: i64,
    }

    impl TestPayload {
        #[allow(unused)]
        fn json() -> String {
            serde_json::to_string(&Self::default()).unwrap()
        }

        #[allow(unused)]
        fn protobuf() -> Vec<u8> {
            use prost::Message;
            Self::default().encode_to_vec()
        }
    }

    #[allow(unused)]
    async fn responder(payload: Payload<TestPayload>) -> Payload<TestPayload> {
        payload
    }

    #[allow(unused)]
    macro_rules! setup {
        () => {
            actix_web::test::init_service(
                actix_web::App::new().route("/", actix_web::web::get().to(responder)),
            )
            .await
        };
    }

    #[allow(unused)]
    macro_rules! body {
        ($res:expr) => {
            actix_web::body::to_bytes($res.into_body()).await.unwrap()
        };
    }

    #[actix_macros::test]
    #[cfg(feature = "json")]
    async fn test_json_req_json_res() {
        let app = setup!();
        let req = actix_web::test::TestRequest::default()
            .insert_header(("Content-Type", "application/json"))
            .set_payload(TestPayload::json())
            .to_request();
        let resp = actix_web::test::call_service(&app, req).await;

        assert!(resp.status().is_success());

        let body = body!(resp);
        assert_eq!(
            TestPayload::json(),
            String::from_utf8(body.to_vec()).unwrap()
        );
    }

    #[actix_macros::test]
    #[cfg(all(feature = "json", feature = "protobuf"))]
    async fn test_json_req_protobuf_response() {
        let app = setup!();
        let req = actix_web::test::TestRequest::default()
            .insert_header(("Content-Type", "application/json"))
            .insert_header(("Accept", "application/protobuf"))
            .set_payload(TestPayload::json())
            .to_request();
        let resp = actix_web::test::call_service(&app, req).await;

        assert!(resp.status().is_success());

        let body = body!(resp);
        assert_eq!(TestPayload::protobuf(), body.to_vec());
    }

    #[actix_macros::test]
    #[cfg(all(feature = "json", feature = "protobuf"))]
    async fn test_protobuf_req_json_response() {
        let app = setup!();
        let req = actix_web::test::TestRequest::default()
            .insert_header(("Accept", "application/json"))
            .insert_header(("Content-Type", "application/protobuf"))
            .set_payload(TestPayload::protobuf())
            .to_request();
        let resp = actix_web::test::call_service(&app, req).await;

        assert!(resp.status().is_success());

        let body = body!(resp);
        assert_eq!(
            TestPayload::json(),
            String::from_utf8(body.to_vec()).unwrap()
        );
    }

    #[actix_macros::test]
    #[cfg(feature = "protobuf")]
    async fn test_protobuf_req_protobuf_response() {
        let app = setup!();
        let req = actix_web::test::TestRequest::default()
            .insert_header(("Accept", "application/protobuf"))
            .insert_header(("Content-Type", "application/protobuf"))
            .set_payload(TestPayload::protobuf())
            .to_request();
        let resp = actix_web::test::call_service(&app, req).await;

        assert!(resp.status().is_success());

        let body = body!(resp);
        assert_eq!(TestPayload::protobuf(), body.to_vec());
    }
}
