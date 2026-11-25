//! MakerWorld, through the host Bambu's own app uses.
//!
//! `makerworld.com` sits behind a Cloudflare challenge, including its JSON
//! routes, so a desktop client cannot scrape the site. Bambu Studio browses
//! that site in a web view and downloads through its cloud plugin. The plugin
//! is closed. The calls below are the same ones the MakerWorld site and the
//! Bambu app make against `api.bambulab.com`: search is public, and a 3MF
//! download is a short-lived link minted with the account token Shale already
//! keeps for cloud sign-in.

use serde_json::Value;

use super::{
    id_string, licence_label, plain_text, string_field, Library, LibraryFile, Listing, ModelCard,
    ModelError, PrintProfile, SearchPage, Sort,
};

const SEARCH: &str = "https://api.bambulab.com/v1/search-service/select/design2";
const PAGE: u32 = 20;

pub fn search_url(keyword: &str, sort: Sort, page: u32) -> String {
    let order = match sort {
        Sort::Relevant | Sort::Newest => "score",
        Sort::Popular => "likeCount",
        Sort::Downloads => "downloadCount",
    };
    let offset = page.saturating_sub(1).saturating_mul(PAGE);
    format!(
        "{SEARCH}?keyword={}&orderBy={order}&designType=0&isFromSearchList=false&offset={offset}&limit={PAGE}",
        super::encode_query(keyword)
    )
}

pub fn design_url(id: &str) -> String {
    format!("https://api.bambulab.com/v1/design-service/design/{id}")
}

pub fn instances_url(id: &str) -> String {
    format!("https://api.bambulab.com/v1/design-service/design/{id}/instances")
}

pub fn profile_url(profile_id: &str, model_id: &str) -> String {
    format!(
        "https://api.bambulab.com/v1/iot-service/api/user/profile/{profile_id}?model_id={}",
        super::encode_query(model_id)
    )
}

pub fn parse_search(value: &Value) -> Result<SearchPage, ModelError> {
    if value.get("hits").is_some_and(|hits| hits.is_null()) {
        let total = value
            .get("total")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0);
        if total > 0 {
            return Err(ModelError::Blocked);
        }
    }
    let hits = value
        .get("hits")
        .or_else(|| value.get("designs"))
        .and_then(Value::as_array)
        .ok_or_else(|| ModelError::Shape {
            call: "MakerWorld search".into(),
            message: "the answer had no hit list".into(),
        })?;
    let cards = hits.iter().filter_map(card_from_hit).collect::<Vec<_>>();
    let total = value
        .get("total")
        .or_else(|| value.get("totalCount"))
        .and_then(Value::as_u64)
        .unwrap_or(cards.len() as u64);
    Ok(SearchPage { total, hits: cards })
}

pub fn parse_listing(value: &Value) -> Result<Listing, ModelError> {
    let design = unwrap_design(value);
    let card = card_from_hit(design).ok_or_else(|| ModelError::Shape {
        call: "MakerWorld model".into(),
        message: "the answer had no design id".into(),
    })?;
    let summary = design
        .get("summary")
        .or_else(|| design.get("description"))
        .and_then(Value::as_str)
        .map(plain_text)
        .unwrap_or_default();
    let profiles = profiles_from(design);
    let files = raw_files(design);
    let remote_model_id = string_field(design, &["modelId", "model_id"]).unwrap_or_default();
    Ok(Listing {
        card,
        summary,
        profiles,
        files,
        remote_model_id,
    })
}

pub fn parse_instances(value: &Value) -> Vec<PrintProfile> {
    if let Some(list) = value.as_array() {
        return list.iter().filter_map(profile_from).collect();
    }
    for key in ["hits", "instances", "list"] {
        if let Some(list) = value.get(key).and_then(Value::as_array) {
            return list.iter().filter_map(profile_from).collect();
        }
    }
    Vec::new()
}

/// `(url, file name)` for the short-lived 3MF link.
pub fn signed_download(value: &Value) -> Result<(String, String), ModelError> {
    let url = string_field(value, &["url", "downloadUrl", "download_url"]).or_else(|| {
        value
            .get("data")
            .and_then(|data| string_field(data, &["url", "downloadUrl"]))
    });
    let Some(url) = url else {
        return Err(ModelError::Shape {
            call: "MakerWorld download".into(),
            message: "the answer had no file link".into(),
        });
    };
    let name = string_field(value, &["name", "fileName", "filename"])
        .unwrap_or_else(|| "model.3mf".into());
    Ok((url, name))
}

fn unwrap_design(value: &Value) -> &Value {
    if value.get("id").is_some() {
        return value;
    }
    for key in ["design", "data", "hit"] {
        if let Some(inner) = value.get(key).filter(|item| item.is_object()) {
            return inner;
        }
    }
    value
}

fn card_from_hit(value: &Value) -> Option<ModelCard> {
    let id = value.get("id").and_then(id_string)?;
    if !id.chars().all(|ch| ch.is_ascii_digit()) {
        return None;
    }
    let title = string_field(value, &["title", "titleTranslated", "name"])
        .filter(|title| !title.is_empty())
        .unwrap_or_else(|| "Untitled model".into());
    let slug = string_field(value, &["slug"]).unwrap_or_default();
    let slug = slug.trim().trim_matches('-');
    let page_url = if slug.is_empty() {
        format!("https://makerworld.com/en/models/{id}")
    } else {
        format!("https://makerworld.com/en/models/{id}-{slug}")
    };
    let (creator, creator_url) = creator(value);
    let licence_code =
        string_field(value, &["license", "licence", "licenseType"]).unwrap_or_default();
    Some(ModelCard {
        library: Library::MakerWorld,
        id,
        title,
        creator,
        creator_url,
        licence: licence_label(&licence_code),
        thumbnail_url: string_field(value, &["cover", "coverUrl", "thumbnail", "coverThumbnail"]),
        page_url,
        likes: number_field(value, &["likeCount", "likes"]),
        downloads: number_field(value, &["downloadCount", "downloads"]),
        nsfw: value.get("nsfw").and_then(Value::as_bool).unwrap_or(false),
    })
}

fn creator(value: &Value) -> (String, Option<String>) {
    let person = value
        .get("designCreator")
        .or_else(|| value.get("creator"))
        .filter(|item| item.is_object());
    let Some(person) = person else {
        return ("Unknown creator".into(), None);
    };
    let name = string_field(person, &["name", "nickname", "handle"])
        .unwrap_or_else(|| "Unknown creator".into());
    let url = string_field(person, &["handle"]).map(|handle| {
        format!(
            "https://makerworld.com/en/@{}",
            handle.trim_start_matches('@')
        )
    });
    (name, url)
}

fn profiles_from(design: &Value) -> Vec<PrintProfile> {
    for key in ["instances", "profiles", "instanceList"] {
        if let Some(list) = design.get(key).and_then(Value::as_array) {
            let profiles: Vec<_> = list.iter().filter_map(profile_from).collect();
            if !profiles.is_empty() {
                return profiles;
            }
        }
    }
    Vec::new()
}

fn profile_from(value: &Value) -> Option<PrintProfile> {
    let id = value
        .get("profileId")
        .or_else(|| value.get("profile_id"))
        .and_then(id_string)
        .or_else(|| value.get("id").and_then(id_string))?;
    let name = string_field(value, &["title", "name", "instanceTitle"])
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| format!("Print profile {id}"));
    let printer = value
        .get("compatibility")
        .and_then(|item| string_field(item, &["devModelName", "printer", "model"]))
        .or_else(|| string_field(value, &["printer", "devModelName"]));
    Some(PrintProfile { id, name, printer })
}

fn raw_files(design: &Value) -> Vec<LibraryFile> {
    let Some(list) = design
        .pointer("/designExtension/model_files")
        .or_else(|| design.pointer("/designExtension/modelFiles"))
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };
    list.iter()
        .filter_map(|file| {
            let name = string_field(file, &["name", "fileName"])?;
            let kind = super::kind_of(&name)?;
            let id = file
                .get("id")
                .and_then(id_string)
                .unwrap_or_else(|| name.clone());
            Some(LibraryFile {
                id,
                name,
                kind,
                bytes: file.get("size").and_then(Value::as_u64),
            })
        })
        .collect()
}

fn number_field(value: &Value, keys: &[&str]) -> Option<u64> {
    for key in keys {
        if let Some(number) = value.get(*key).and_then(Value::as_u64) {
            return Some(number);
        }
    }
    None
}
