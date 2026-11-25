//! Thingiverse's official REST API.
//!
//! Search and file lists need the user's own app token. Shale never ships
//! one. The developer page is <https://www.thingiverse.com/developers>.

use serde_json::Value;

use super::{
    id_string, licence_label, plain_text, string_field, Library, LibraryFile, Listing, ModelCard,
    ModelError, SearchPage, Sort,
};

const PAGE: u32 = 20;

pub fn search_url(keyword: &str, sort: Sort, page: u32) -> String {
    let order = match sort {
        Sort::Relevant | Sort::Downloads => "relevant",
        Sort::Popular => "popular",
        Sort::Newest => "newest",
    };
    format!(
        "https://api.thingiverse.com/search/{}?page={page}&per_page={PAGE}&sort={order}",
        super::encode_query(keyword)
    )
}

pub fn thing_url(id: &str) -> String {
    format!("https://api.thingiverse.com/things/{id}")
}

pub fn files_url(id: &str) -> String {
    format!("https://api.thingiverse.com/things/{id}/files")
}

pub fn parse_search(value: &Value) -> Result<SearchPage, ModelError> {
    let hits = if let Some(list) = value.as_array() {
        list
    } else {
        value
            .get("hits")
            .or_else(|| value.get("things"))
            .and_then(Value::as_array)
            .ok_or_else(|| ModelError::Shape {
                call: "Thingiverse search".into(),
                message: "the answer had no hit list".into(),
            })?
    };
    let cards = hits.iter().filter_map(card_from).collect::<Vec<_>>();
    let total = value
        .get("total")
        .and_then(Value::as_u64)
        .unwrap_or(cards.len() as u64);
    Ok(SearchPage { total, hits: cards })
}

pub fn parse_thing(value: &Value) -> Result<Listing, ModelError> {
    let thing = value.get("thing").unwrap_or(value);
    let card = card_from(thing).ok_or_else(|| ModelError::Shape {
        call: "Thingiverse model".into(),
        message: "the answer had no thing id".into(),
    })?;
    let summary = thing
        .get("description")
        .or_else(|| thing.get("description_html"))
        .and_then(Value::as_str)
        .map(plain_text)
        .unwrap_or_default();
    Ok(Listing {
        card,
        summary,
        profiles: Vec::new(),
        files: Vec::new(),
        remote_model_id: String::new(),
    })
}

pub(crate) struct RemoteFile {
    pub file: LibraryFile,
    pub url: String,
}

pub fn parse_files(value: &Value) -> Vec<RemoteFile> {
    let list = value
        .as_array()
        .or_else(|| value.get("files").and_then(Value::as_array));
    let Some(list) = list else {
        return Vec::new();
    };
    list.iter().filter_map(remote_file).collect()
}

fn card_from(value: &Value) -> Option<ModelCard> {
    let id = value.get("id").and_then(id_string)?;
    if !id.chars().all(|ch| ch.is_ascii_digit()) {
        return None;
    }
    let title = string_field(value, &["name", "title"])
        .filter(|title| !title.is_empty())
        .unwrap_or_else(|| "Untitled model".into());
    let page_url = string_field(value, &["public_url", "url"])
        .unwrap_or_else(|| format!("https://www.thingiverse.com/thing:{id}"));
    let person = value.get("creator").filter(|item| item.is_object());
    let creator = person
        .and_then(person_name)
        .unwrap_or_else(|| "Unknown creator".into());
    let creator_url = person.and_then(|person| string_field(person, &["public_url", "url"]));
    let licence_code = string_field(value, &["license", "licence"]).unwrap_or_default();
    Some(ModelCard {
        library: Library::Thingiverse,
        id,
        title,
        creator,
        creator_url,
        licence: licence_label(&licence_code),
        thumbnail_url: string_field(value, &["thumbnail", "preview_image"]),
        page_url,
        likes: value
            .get("like_count")
            .or_else(|| value.get("likes"))
            .and_then(Value::as_u64),
        downloads: None,
        nsfw: false,
    })
}

fn person_name(person: &Value) -> Option<String> {
    if let Some(name) = string_field(person, &["name", "username"]) {
        if !name.is_empty() {
            return Some(name);
        }
    }
    let first = string_field(person, &["first_name"]).unwrap_or_default();
    let last = string_field(person, &["last_name"]).unwrap_or_default();
    let joined = format!("{first} {last}").trim().to_string();
    if joined.is_empty() {
        None
    } else {
        Some(joined)
    }
}

fn remote_file(value: &Value) -> Option<RemoteFile> {
    let name = string_field(value, &["name"])?;
    let kind = super::kind_of(&name)?;
    let id = value.get("id").and_then(id_string)?;
    let url = string_field(value, &["direct_url", "download_url", "public_url"])?;
    Some(RemoteFile {
        file: LibraryFile {
            id,
            name,
            kind,
            bytes: value.get("size").and_then(Value::as_u64),
        },
        url,
    })
}
