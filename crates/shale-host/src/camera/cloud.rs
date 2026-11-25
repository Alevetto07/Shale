//! A cloud still, if the account will give one.
//!
//! Bambu's remote camera is a private relay (TUTK / Agora). There is no
//! documented JPEG URL. Shale asks the iot-service once, and treats anything
//! that is not a JPEG — including a 404 — as the cloud camera being
//! unavailable. The LAN picture is the one that works.

use crate::cloud::HttpRequest;

use super::CameraFault;

/// The path that is asked. It is a probe, not a promise: a body that is not
/// a JPEG is [`CameraFault::CloudUnavailable`].
pub const CLOUD_CAMERA_PATH: &str = "/v1/iot-service/api/user/device/camera";

pub fn snapshot_request(api_host: &str, dev_id: &str, token: &str) -> HttpRequest {
    let host = api_host.trim_end_matches('/');
    HttpRequest {
        name: "GET camera snapshot",
        method: "GET",
        url: format!("{host}{CLOUD_CAMERA_PATH}?dev_id={dev_id}"),
        headers: vec![
            ("Authorization".into(), format!("Bearer {token}")),
            ("Accept".into(), "image/jpeg".into()),
        ],
        body: None,
    }
}

/// A 200 whose body starts with `FF D8` is a picture. Everything else is the
/// cloud camera being unavailable, which is the usual answer.
pub fn interpret_cloud_body(status: u16, body: &[u8]) -> Result<Vec<u8>, CameraFault> {
    if status == 200 && body.len() >= 4 && body.starts_with(&[0xff, 0xd8]) {
        return Ok(body.to_vec());
    }
    Err(CameraFault::CloudUnavailable)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_jpeg_is_kept_and_anything_else_is_unavailable() {
        let jpeg = [0xff, 0xd8, 0xff, 0xd9];
        assert_eq!(interpret_cloud_body(200, &jpeg).unwrap(), jpeg);
        assert_eq!(
            interpret_cloud_body(404, b"missing"),
            Err(CameraFault::CloudUnavailable)
        );
        assert_eq!(
            interpret_cloud_body(200, b"{\"error\":true}"),
            Err(CameraFault::CloudUnavailable)
        );
    }

    #[test]
    fn the_request_names_the_device_and_not_a_guessed_token_in_the_url() {
        let request = snapshot_request("https://api.bambulab.com", "01", "token-secret");
        assert!(request.url.contains(CLOUD_CAMERA_PATH));
        assert!(request.url.contains("dev_id=01"));
        assert!(!request.url.contains("token-secret"));
        assert!(format!("{request:?}").contains("[redacted]"));
    }
}
