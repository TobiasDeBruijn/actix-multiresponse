use actix_web::body::BoxBody;
use actix_web::http::StatusCode;
use actix_web::{HttpResponse, ResponseError};
use thiserror::Error;
use crate::DeserializeError;

#[derive(Debug, Error)]
pub enum PayloadError {
    #[error("Payload error: {0}")]
    ActixPayload(#[from] actix_web::error::PayloadError),
    #[error("Error: {0}")]
    Deserialize(#[from] DeserializeError),
    #[error("Invalid content type")]
    InvalidContentType,
}

impl ResponseError for PayloadError {
    fn status_code(&self) -> StatusCode {
        StatusCode::BAD_REQUEST
    }

    fn error_response(&self) -> HttpResponse<BoxBody> {
        HttpResponse::build(self.status_code()).body(format!("{self}"))
    }
}
