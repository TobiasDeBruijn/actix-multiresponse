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
pub use crate::headers::ContentType;

use actix_web::body::BoxBody;
use actix_web::{FromRequest, HttpRequest, HttpResponse, Responder};
use actix_web::http::StatusCode;

use std::future::Future;
use std::ops::{Deref, DerefMut};
use std::pin::Pin;

use futures_util::StreamExt;
use thiserror::Error;

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

#[cfg(any(feature = "json", feature = "xml"))]
pub trait SerdeSupportDeserialize: serde::de::DeserializeOwned {}
#[cfg(not(any(feature = "json", feature = "xml")))]
pub trait SerdeSupportDeserialize {}

#[cfg(any(feature = "json", feature = "xml"))]
impl<T: serde::de::DeserializeOwned> SerdeSupportDeserialize for T {}
#[cfg(not(any(feature = "json", feature = "xml")))]
impl<T> SerdeSupportDeserialize for T {}

#[cfg(any(feature = "json", feature = "xml"))]
pub trait SerdeSupportSerialize: serde::Serialize {}
#[cfg(not(any(feature = "json", feature = "xml")))]
pub trait SerdeSupportSerialize {}

#[cfg(any(feature = "json", feature = "xml"))]
impl<T: serde::Serialize> SerdeSupportSerialize for T {}
#[cfg(not(any(feature = "json", feature = "xml")))]
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
            let mut payload_bytes = Vec::new();
            while let Some(Ok(b)) = payload.next().await {
                payload_bytes.append(&mut b.to_vec())
            }

            let content_type = ContentType::from_request_content_type(&req);
            if content_type.eq(&ContentType::Other) {
                return Err(PayloadError::InvalidContentType)
            }

            let this = Payload::deserialize(&payload_bytes, content_type)?;

            Ok(this)
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

        let serialized = match self.serialize(content_type.clone()) {
            Ok(x) => x,
            Err(e) => {
                return HttpResponse::build(StatusCode::INTERNAL_SERVER_ERROR)
                    .body(e.to_string());
            }
        };

        let mut response = HttpResponse::build(StatusCode::OK);
        match content_type {
            #[cfg(feature = "json")]
            ContentType::Json => response.insert_header(("Content-Type", "application/json")),
            #[cfg(feature = "protobuf")]
            ContentType::Protobuf => response.insert_header(("Content-Type", "application/protobuf")),
            #[cfg(feature = "xml")]
            ContentType::Xml => response.insert_header(("Content-Type", "application/xml")),
            ContentType::Other => panic!("Must have ast least one format feature enabled.")
        };

        response.body(serialized)
    }
}

#[derive(Debug, Error)]
pub enum SerializeError {
    #[cfg(feature = "json")]
    #[error("Failed to serialize to JSON: {0}")]
    SerdeJson(#[from] serde_json::Error),
    #[cfg(feature = "json")]
    #[error("Failed to encode to protobuf: {0}")]
    Prost(String),
    #[cfg(feature = "xml")]
    #[error("Failed to serialize to XML: {0}")]
    QuickXml(#[from] quick_xml::DeError),
    #[error("Unable to serialize")]
    Unserializable,
}

#[derive(Debug, Error)]
pub enum DeserializeError {
    #[cfg(feature = "json")]
    #[error("Failed to deserialize from JSON: {0}")]
    SerdeJson(#[from] serde_json::Error),
    #[cfg(feature = "protobuf")]
    #[error("Failed to decode from protobuf: {0}")]
    Prost(String),
    #[cfg(feature = "xml")]
    #[error("Failed to deserialize from XML: {0}")]
    Xml(#[from] quick_xml::DeError),
    #[error("Unable to deserialize")]
    Undeserializable
}

impl<T: ProtobufSupport + SerdeSupportSerialize + Default + Clone> Payload<T> {
    pub fn serialize(&self, content_type: ContentType) -> Result<Vec<u8>, SerializeError> {
        match content_type {
            #[cfg(feature = "json")]
            ContentType::Json => {
                let json = serde_json::to_string_pretty(&self.0)?;
                Ok(json.into_bytes())
            },
            #[cfg(feature = "protobuf")]
            ContentType::Protobuf => {
                let mut protobuf = Vec::new();
                self.0.encode(&mut protobuf)
                    .map_err(|e| SerializeError::Prost(e.to_string()))?;
                Ok(protobuf)
            },
            #[cfg(feature = "xml")]
            ContentType::Xml => {
                let xml = quick_xml::se::to_string(&self.0)?;
                Ok(xml.into_bytes())
            }
            ContentType::Other => Err(SerializeError::Unserializable)
        }
    }
}

impl<T: ProtobufSupport + SerdeSupportDeserialize + Default + Clone> Payload<T> {
    pub fn deserialize(body: &[u8], content_type: ContentType) -> Result<Self, DeserializeError> {
        match content_type {
            #[cfg(feature = "json")]
            ContentType::Json => {
                let payload: T = serde_json::from_slice(body)?;
                Ok(Self(payload))
            },
            #[cfg(feature = "protobuf")]
            ContentType::Protobuf => {
                let payload = T::decode(body)
                    .map_err(|e| DeserializeError::Prost(e.to_string()))?;
                Ok(Self(payload))
            },
            #[cfg(feature = "xml")]
            ContentType::Xml => {
                let payload: T = quick_xml::de::from_reader(body)?;
                Ok(Self(payload) )
            }
            ContentType::Other => Err(DeserializeError::Undeserializable)
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
            serde_json::to_string_pretty(&Self::default()).unwrap()
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
