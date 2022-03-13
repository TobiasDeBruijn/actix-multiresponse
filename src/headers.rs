use actix_web::HttpRequest;

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum ContentType {
    #[cfg(feature = "json")]
    Json,
    #[cfg(feature = "protobuf")]
    Protobuf,
    Other,
}

impl Default for ContentType {
    fn default() -> Self {
        cfg_if::cfg_if! {
            if #[cfg(feature = "json")] {
                Self::Json
            } else if #[cfg(feature = "protobuf")] {
                Self::Protobuf
            } else {
                Self::Other
            }
        }
    }
}

impl ContentType {
    #[inline]
    pub fn from_request_content_type(req: &HttpRequest) -> Self {
        Self::from_request_header(req, "Content-Type")
    }

    #[inline]
    pub fn from_request_accepts(req: &HttpRequest) -> Self {
        Self::from_request_header(req, "Accept")
    }

    #[inline]
    pub fn from_request_header<S: AsRef<str>>(req: &HttpRequest, name: S) -> Self {
        if let Some(header_value) = req.headers().get(name.as_ref()) {
            if let Ok(hv_str) = header_value.to_str() {
                let l = hv_str.to_lowercase();

                if l.starts_with("application/json") {
                    #[cfg(feature = "json")]
                    return Self::Json;
                    #[cfg(not(feature = "json"))]
                    return Self::Other;
                } else if l.starts_with("application/protobuf") {
                    #[cfg(feature = "protobuf")]
                    return Self::Protobuf;
                    #[cfg(not(feature = "protobuf"))]
                    return Self::Other;
                } else {
                    return Self::Other;
                }
            }
        }

        ContentType::Other
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use actix_web::test::TestRequest;

    #[test]
    #[cfg(feature = "json")]
    fn test_json_plain() {
        let req = TestRequest::get()
            .insert_header(("Content-Type", "application/json"))
            .to_http_request();

        assert_eq!(
            ContentType::Json,
            ContentType::from_request_content_type(&req)
        )
    }

    #[test]
    #[cfg(feature = "json")]
    fn test_json_with_charset() {
        let req = TestRequest::get()
            .insert_header(("Content-Type", "application/json; charset=UTF-8"))
            .to_http_request();

        assert_eq!(
            ContentType::Json,
            ContentType::from_request_content_type(&req)
        )
    }

    #[test]
    #[cfg(feature = "protobuf")]
    fn test_protobuf() {
        let req = TestRequest::get()
            .insert_header(("Content-Type", "application/protobuf"))
            .to_http_request();

        assert_eq!(
            ContentType::Protobuf,
            ContentType::from_request_content_type(&req)
        )
    }

    #[test]
    #[cfg(feature = "protobuf")]
    fn test_protobuf_with_charset() {
        let req = TestRequest::get()
            .insert_header(("Content-Type", "application/protobuf; charset=UTF-8"))
            .to_http_request();

        assert_eq!(
            ContentType::Protobuf,
            ContentType::from_request_content_type(&req)
        )
    }

    #[test]
    fn test_other() {
        let req = TestRequest::get()
            .insert_header(("Content-Type", "foo/bar"))
            .to_http_request();

        assert_eq!(
            ContentType::Other,
            ContentType::from_request_content_type(&req)
        );
    }
}
