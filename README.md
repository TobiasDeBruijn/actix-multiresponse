# actix-multiresponse
actix-multiresponse intents to allow supporting multiple response/request data formats depending on the `Content-Type` and `Accept` header.

## Supported formats
- Json
- Protobuf
- XML

All formats can be enabled using equally-named feature flags. At least one format should be enabled.
By default `json` and `protobuf` are enabled.

## Example
```rs
use prost_derive::Message;
use serde_derive::{Deserialize, Serialize};
use actix_multiresponse::Payload;

#[derive(Deserialize, Serialize, Message, Clone)]
struct TestPayload {
    #[prost(string, tag = "1")]
    foo: String,
    #[prost(int64, tag = "2")]
    bar: i64,
}

async fn responder(payload: Payload<TestPayload>) -> Payload<TestPayload> {
    payload
}
```

## License
actix-multiresponse is dual licensed under the MIT or the Apache-2.0 license, at your discretion