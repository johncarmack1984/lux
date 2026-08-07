//! Planning Center's JSON:API envelope, typed once.
//!
//! Every response is `{"data": …}` with optional `included`, `links` and
//! `meta`. Resources carry their attributes in `attributes` and point at each
//! other through `relationships` — so "which item is the service on?" is two
//! hops (Live → its current ItemTime → that ItemTime's Item), and both hops
//! come back in one response when the request asks for them with `?include=`.
//!
//! Everything here is deliberately forgiving: unknown attributes are ignored,
//! missing ones default. Planning Center adds fields without asking, and a
//! bridge that fails a Sunday because a new attribute appeared would be a
//! worse bridge than one that ignores it.

use std::collections::BTreeMap;

use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::Value;

/// One resource object: a type, an id, its attributes, and its pointers.
#[derive(Debug, Clone, Deserialize)]
#[serde(bound(deserialize = "A: Deserialize<'de> + Default"))]
pub struct Resource<A> {
    #[serde(rename = "type")]
    pub kind: String,
    pub id: String,
    #[serde(default)]
    pub attributes: A,
    #[serde(default)]
    pub relationships: BTreeMap<String, Relationship>,
}

impl<A> Resource<A> {
    /// The id on the far side of a to-one relationship, if it has one.
    /// A to-many relationship answers `None`: nothing in this crate follows
    /// one, and quietly taking the first element would be a guess.
    pub fn related_id(&self, name: &str) -> Option<&str> {
        match self.relationships.get(name)?.data.as_ref()? {
            RelationshipData::One(reference) => Some(reference.id.as_str()),
            RelationshipData::Many(_) => None,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Relationship {
    /// `null` is a real, meaningful value here: "the plan has no next item".
    #[serde(default)]
    pub data: Option<RelationshipData>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum RelationshipData {
    One(ResourceRef),
    Many(Vec<ResourceRef>),
}

#[derive(Debug, Clone, Deserialize)]
pub struct ResourceRef {
    #[serde(rename = "type")]
    pub kind: String,
    pub id: String,
}

/// A list response.
#[derive(Debug, Clone, Deserialize)]
#[serde(bound(deserialize = "A: Deserialize<'de> + Default"))]
pub struct Collection<A> {
    pub data: Vec<Resource<A>>,
    #[serde(default)]
    pub included: Vec<Value>,
    #[serde(default)]
    pub links: Links,
}

/// A single-resource response.
#[derive(Debug, Clone, Deserialize)]
#[serde(bound(deserialize = "A: Deserialize<'de> + Default"))]
pub struct Single<A> {
    pub data: Resource<A>,
    #[serde(default)]
    pub included: Vec<Value>,
    #[serde(default)]
    pub links: Links,
}

/// A single-resource response whose `data` may legitimately be `null` — what a
/// to-one association endpoint answers when the far side isn't there ("the
/// service is live, and nothing is queued after this item").
#[derive(Debug, Clone, Deserialize)]
#[serde(bound(deserialize = "A: Deserialize<'de> + Default"))]
pub struct MaybeSingle<A> {
    #[serde(default)]
    pub data: Option<Resource<A>>,
    #[serde(default)]
    pub included: Vec<Value>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Links {
    /// The next page's absolute URL. Its presence — not a count, not an
    /// offset — is how this crate knows to keep paging.
    #[serde(default)]
    pub next: Option<String>,
    #[serde(default, rename = "self")]
    pub self_link: Option<String>,
}

/// Pull a sideloaded resource out of `included` by type and id.
///
/// `included` stays untyped (`Value`) because one response can sideload
/// several different types; the caller says which one it wants and gets it
/// typed, or `None` if the server didn't send it.
pub fn included_as<A: DeserializeOwned + Default>(
    included: &[Value],
    kind: &str,
    id: &str,
) -> Option<Resource<A>> {
    let found = included.iter().find(|value| {
        value.get("type").and_then(Value::as_str) == Some(kind)
            && value.get("id").and_then(Value::as_str) == Some(id)
    })?;
    serde_json::from_value(found.clone()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Default, Deserialize, PartialEq, Eq)]
    #[serde(default)]
    struct Attrs {
        title: Option<String>,
        sequence: Option<i64>,
    }

    #[test]
    fn a_collection_parses_data_links_and_ignores_unknown_fields() {
        let doc: Collection<Attrs> = serde_json::from_str(
            r#"{
              "links": {"self": "https://x/items?offset=0", "next": "https://x/items?offset=25"},
              "data": [
                {"type":"Item","id":"1","attributes":{"title":"Welcome","sequence":1,"who_knows":true},
                 "relationships":{"song":{"data":null}}}
              ],
              "meta": {"total_count": 30}
            }"#,
        )
        .unwrap();
        assert_eq!(doc.data.len(), 1);
        assert_eq!(doc.data[0].attributes.title.as_deref(), Some("Welcome"));
        assert_eq!(doc.data[0].related_id("song"), None);
        assert_eq!(doc.links.next.as_deref(), Some("https://x/items?offset=25"));
    }

    #[test]
    fn a_resource_with_no_attributes_still_parses() {
        // Defaults everywhere: a vertex we read one field of must not fail
        // because Planning Center omitted the rest.
        let doc: Single<Attrs> =
            serde_json::from_str(r#"{"data":{"type":"Live","id":"1"}}"#).unwrap();
        assert_eq!(doc.data.attributes, Attrs::default());
        assert_eq!(doc.data.id, "1");
        assert!(doc.included.is_empty());
    }

    #[test]
    fn to_one_relationships_resolve_and_to_many_do_not() {
        let doc: Single<Attrs> = serde_json::from_str(
            r#"{"data":{"type":"Live","id":"1","relationships":{
                 "current_item_time":{"data":{"type":"ItemTime","id":"77"}},
                 "watchable_plans":{"data":[{"type":"Plan","id":"9"}]}
               }}}"#,
        )
        .unwrap();
        assert_eq!(doc.data.related_id("current_item_time"), Some("77"));
        assert_eq!(doc.data.related_id("watchable_plans"), None);
        assert_eq!(doc.data.related_id("absent"), None);
    }

    #[test]
    fn included_is_looked_up_by_type_and_id() {
        let doc: Single<Attrs> = serde_json::from_str(
            r#"{"data":{"type":"Live","id":"1"},
                "included":[
                  {"type":"ItemTime","id":"77","attributes":{"title":"wrong type on purpose"}},
                  {"type":"Item","id":"77","attributes":{"title":"Sermon","sequence":6}}
                ]}"#,
        )
        .unwrap();
        let item: Resource<Attrs> = included_as(&doc.included, "Item", "77").unwrap();
        assert_eq!(item.attributes.sequence, Some(6));
        assert!(included_as::<Attrs>(&doc.included, "Item", "78").is_none());
    }
}
