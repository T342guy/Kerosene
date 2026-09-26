// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! What the editor knows about a map that the game never needs to.
//!
//! VisGroups, groups, a cordon, an object's colour and comments: none of it
//! changes how a map plays, all of it changes whether a map is editable once
//! it has a few hundred brushes in it. It is kept in its own blocks -- an
//! `editor {}` block on each object, `visgroups {}`, `group {}` and
//! `cordon {}` at the root -- so that Cleave's verbatim copy of an entity's
//! properties into the compiled entity lump never picks any of it up, and
//! so that a reader that predates all of this simply skips it.
//!
//! This is the `.vmf` arrangement, key for key where it made sense, so that
//! anyone who has read a Hammer file knows what they are looking at.

use crate::vec3_to_kv;
use kerosene_kv::{Entry, KeyValues, Vec3Value};
use kerosene_math::{Aabb, Vec3};

/// Editor-only metadata carried by a solid or an entity.
#[derive(Clone, Debug, PartialEq)]
pub struct EditorData {
    /// The visgroups this object belongs to, by id. An object in a hidden
    /// visgroup is hidden.
    pub visgroups: Vec<u32>,
    /// The group this object belongs to, by id. Picking any member selects
    /// the whole group unless the editor is told to ignore groups.
    pub group: Option<u32>,
    /// An outline colour for the 2D panes. Absent means "whatever the object
    /// would be drawn in anyway".
    pub color: Option<[u8; 3]>,
    /// Whether the object is shown at all. Quick-hide (Hammer's `H`) clears
    /// it; unhide-all sets it back. Written only when false.
    pub visible: bool,
    pub comments: String,
}

impl Default for EditorData {
    fn default() -> Self {
        EditorData {
            visgroups: Vec::new(),
            group: None,
            color: None,
            visible: true,
            comments: String::new(),
        }
    }
}

impl EditorData {
    pub fn is_default(&self) -> bool {
        *self == EditorData::default()
    }

    pub fn in_visgroup(&self, id: u32) -> bool {
        self.visgroups.contains(&id)
    }

    /// Add the object to a visgroup; a second add is a no-op rather than a
    /// duplicate, so removing it once removes it.
    pub fn add_to_visgroup(&mut self, id: u32) {
        if !self.in_visgroup(id) {
            self.visgroups.push(id);
        }
    }

    pub fn remove_from_visgroup(&mut self, id: u32) -> bool {
        let before = self.visgroups.len();
        self.visgroups.retain(|&g| g != id);
        before != self.visgroups.len()
    }

    pub(crate) fn from_kv(kv: &KeyValues) -> EditorData {
        let mut data = EditorData {
            visible: kv.get_or("visible", true),
            comments: kv.get("comments").unwrap_or("").to_string(),
            ..EditorData::default()
        };
        for value in kv.get_all("visgroupid") {
            if let Ok(id) = value.trim().parse::<u32>() {
                data.add_to_visgroup(id);
            }
        }
        data.group = kv.get("groupid").and_then(|v| v.trim().parse().ok());
        data.color = kv.get("color").and_then(parse_color);
        data
    }

    /// The `editor {}` block, or `None` when there is nothing to say.
    pub(crate) fn to_kv(&self) -> Option<KeyValues> {
        if self.is_default() {
            return None;
        }
        let mut kv = KeyValues::new("editor");
        if let Some(color) = self.color {
            kv.push("color", format_color(color));
        }
        for id in &self.visgroups {
            kv.push_value("visgroupid", *id);
        }
        if let Some(group) = self.group {
            kv.push_value("groupid", group);
        }
        if !self.visible {
            kv.push("visible", "0");
        }
        if !self.comments.is_empty() {
            kv.push("comments", self.comments.clone());
        }
        Some(kv)
    }
}

/// `"r g b"`, each 0-255, the way `.vmf` spells a colour.
pub fn parse_color(text: &str) -> Option<[u8; 3]> {
    let mut parts = text.split_whitespace().map(|p| p.parse::<u8>().ok());
    let r = parts.next()??;
    let g = parts.next()??;
    let b = parts.next()??;
    Some([r, g, b])
}

pub fn format_color(c: [u8; 3]) -> String {
    format!("{} {} {}", c[0], c[1], c[2])
}

/// A named set of objects that can be hidden together.
///
/// VisGroups nest: hiding a parent hides everything under it. A visgroup
/// marked `stream` is also a *section* -- Cleave tags its brushes with it
/// and the engine loads and unloads the section's geometry around the
/// player. That is the one thing here the game does care about, and it
/// reaches the game through the compiled map, never through this block.
#[derive(Clone, Debug, PartialEq)]
pub struct VisGroup {
    pub id: u32,
    pub name: String,
    pub color: [u8; 3],
    pub visible: bool,
    /// Whether this visgroup is a streamed section.
    pub stream: bool,
    pub children: Vec<VisGroup>,
}

impl VisGroup {
    pub fn new(id: u32, name: &str) -> VisGroup {
        VisGroup {
            id,
            name: name.to_string(),
            color: color_for_id(id),
            visible: true,
            stream: false,
            children: Vec::new(),
        }
    }

    /// This visgroup and every descendant, depth first, with the depth of
    /// each -- what a tree view draws.
    pub fn walk(&self) -> Vec<(&VisGroup, usize)> {
        let mut out = Vec::new();
        self.walk_into(0, &mut out);
        out
    }

    fn walk_into<'a>(&'a self, depth: usize, out: &mut Vec<(&'a VisGroup, usize)>) {
        out.push((self, depth));
        for child in &self.children {
            child.walk_into(depth + 1, out);
        }
    }

    pub fn find(&self, id: u32) -> Option<&VisGroup> {
        if self.id == id {
            return Some(self);
        }
        self.children.iter().find_map(|c| c.find(id))
    }

    pub fn find_mut(&mut self, id: u32) -> Option<&mut VisGroup> {
        if self.id == id {
            return Some(self);
        }
        self.children.iter_mut().find_map(|c| c.find_mut(id))
    }

    pub(crate) fn from_kv(kv: &KeyValues) -> VisGroup {
        let id = crate::read_id(kv);
        let mut group = VisGroup::new(id, kv.get("name").unwrap_or("visgroup"));
        if let Some(color) = kv.get("color").and_then(parse_color) {
            group.color = color;
        }
        group.visible = kv.get_or("visible", true);
        group.stream = kv.get_or("stream", false);
        for child in kv.blocks("visgroup") {
            group.children.push(VisGroup::from_kv(child));
        }
        group
    }

    pub(crate) fn to_kv(&self) -> KeyValues {
        let mut kv = KeyValues::new("visgroup");
        kv.push("name", self.name.clone());
        kv.push_value("id", self.id);
        kv.push("color", format_color(self.color));
        if !self.visible {
            kv.push("visible", "0");
        }
        if self.stream {
            kv.push("stream", "1");
        }
        for child in &self.children {
            kv.push_block(child.to_kv());
        }
        kv
    }
}

/// A colour that tells neighbouring visgroups apart, from the id alone, so
/// a new group is distinguishable before anyone picks a colour for it.
pub fn color_for_id(id: u32) -> [u8; 3] {
    const PALETTE: [[u8; 3]; 8] = [
        [90, 200, 120],
        [120, 170, 255],
        [255, 150, 90],
        [220, 120, 220],
        [255, 220, 90],
        [90, 220, 220],
        [255, 110, 110],
        [170, 190, 110],
    ];
    PALETTE[(id as usize) % PALETTE.len()]
}

/// A group: objects that select together. Flat -- a group inside a group is
/// not a thing here.
#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    pub id: u32,
    pub editor: EditorData,
}

impl Group {
    pub(crate) fn from_kv(kv: &KeyValues) -> Group {
        Group {
            id: crate::read_id(kv),
            editor: kv
                .block("editor")
                .map(EditorData::from_kv)
                .unwrap_or_default(),
        }
    }

    pub(crate) fn to_kv(&self) -> KeyValues {
        let mut kv = KeyValues::new("group");
        kv.push_value("id", self.id);
        if let Some(editor) = self.editor.to_kv() {
            kv.push_block(editor);
        }
        kv
    }
}

/// A box that, while active, is all the editor shows and all the compiler
/// compiles -- for working on one corner of a large map.
#[derive(Clone, Debug, PartialEq)]
pub struct Cordon {
    pub bounds: Aabb,
    pub active: bool,
}

impl Cordon {
    pub(crate) fn from_kv(kv: &KeyValues) -> Option<Cordon> {
        use kerosene_kv::FromKvValue;
        let mins = Vec3Value::from_kv(kv.get("mins")?).ok()?;
        let maxs = Vec3Value::from_kv(kv.get("maxs")?).ok()?;
        Some(Cordon {
            bounds: Aabb::new(
                Vec3::from_array(mins.to_array()),
                Vec3::from_array(maxs.to_array()),
            ),
            active: kv.get_or("active", false),
        })
    }

    pub(crate) fn to_kv(&self) -> KeyValues {
        let mut kv = KeyValues::new("cordon");
        kv.push("mins", vec3_to_kv(self.bounds.min));
        kv.push("maxs", vec3_to_kv(self.bounds.max));
        kv.push("active", if self.active { "1" } else { "0" });
        kv
    }
}

/// A solid, a mesh or an entity, by id: what every editor feature that
/// treats them alike addresses.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ObjectId {
    Solid(u32),
    Entity(u32),
    Mesh(u32),
}

impl ObjectId {
    pub fn id(self) -> u32 {
        match self {
            ObjectId::Solid(id) | ObjectId::Entity(id) | ObjectId::Mesh(id) => id,
        }
    }
}

/// The plain pairs of a block whose keys are not in `known`, in file order.
///
/// What lets a solid or a face carry a key nobody has defined yet, the way
/// an entity always could.
pub(crate) fn unknown_pairs(kv: &KeyValues, known: &[&str]) -> Vec<(String, String)> {
    kv.entries
        .iter()
        .filter_map(|e| match e {
            Entry::Pair(k, v) if !known.iter().any(|known| known.eq_ignore_ascii_case(k)) => {
                Some((k.clone(), v.clone()))
            }
            _ => None,
        })
        .collect()
}

/// `get`/`set`/`remove` over an ordered `(key, value)` list -- the shape
/// every object's key-values take, kept as one implementation.
pub(crate) fn kv_get<'a>(pairs: &'a [(String, String)], key: &str) -> Option<&'a str> {
    pairs
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(key))
        .map(|(_, v)| v.as_str())
}

pub(crate) fn kv_set(pairs: &mut Vec<(String, String)>, key: &str, value: String) {
    match pairs.iter_mut().find(|(k, _)| k.eq_ignore_ascii_case(key)) {
        Some((_, v)) => *v = value,
        None => pairs.push((key.to_string(), value)),
    }
}

pub(crate) fn kv_remove(pairs: &mut Vec<(String, String)>, key: &str) -> bool {
    let before = pairs.len();
    pairs.retain(|(k, _)| !k.eq_ignore_ascii_case(key));
    before != pairs.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_default_editor_block_is_not_written() {
        assert!(EditorData::default().to_kv().is_none());
    }

    #[test]
    fn editor_data_round_trips() {
        let data = EditorData {
            visgroups: vec![3, 7],
            group: Some(9),
            color: Some([220, 30, 30]),
            visible: false,
            comments: "the back stairs".into(),
        };
        let kv = data.to_kv().unwrap();
        assert_eq!(EditorData::from_kv(&kv), data);
        assert_eq!(kv.get_all("visgroupid").count(), 2);
    }

    #[test]
    fn a_visgroup_is_joined_once() {
        let mut data = EditorData::default();
        data.add_to_visgroup(4);
        data.add_to_visgroup(4);
        assert_eq!(data.visgroups, vec![4]);
        assert!(data.remove_from_visgroup(4));
        assert!(!data.remove_from_visgroup(4));
    }

    #[test]
    fn visgroups_nest_and_walk_depth_first() {
        let mut root = VisGroup::new(1, "Outside");
        let mut mid = VisGroup::new(2, "Yard");
        mid.children.push(VisGroup::new(3, "Shed"));
        root.children.push(mid);
        root.children.push(VisGroup::new(4, "Road"));
        let walked: Vec<(u32, usize)> = root.walk().iter().map(|(g, d)| (g.id, *d)).collect();
        assert_eq!(walked, vec![(1, 0), (2, 1), (3, 2), (4, 1)]);
        assert_eq!(root.find(3).unwrap().name, "Shed");
        let again = VisGroup::from_kv(&root.to_kv());
        assert_eq!(again, root);
    }

    #[test]
    fn cordon_round_trips() {
        let cordon = Cordon {
            bounds: Aabb::new(Vec3::new(-64.0, -64.0, 0.0), Vec3::new(64.0, 64.0, 128.0)),
            active: true,
        };
        assert_eq!(Cordon::from_kv(&cordon.to_kv()), Some(cordon));
    }

    #[test]
    fn colours_parse_and_reject_garbage() {
        assert_eq!(parse_color("1 2 3"), Some([1, 2, 3]));
        assert_eq!(parse_color("1 2"), None);
        assert_eq!(parse_color("300 0 0"), None);
    }
}
