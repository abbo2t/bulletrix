//! Import outlines from OPML files (port of opml_import.py) - the format
//! WorkFlowy and most other outliners use for export.
//!
//! WorkFlowy writes notes as `_note`; there's no agreed attribute for
//! "completed", so a few plausible spellings are checked.

use crate::{NodeId, Outline};
use roxmltree::{Document, Node as XmlNode, ParsingOptions};
use std::io::{self, ErrorKind};
use std::path::Path;

const TRUE_STRINGS: [&str; 3] = ["true", "1", "yes"];
const COMPLETE_ATTRS: [&str; 4] = ["_complete", "complete", "_completed", "completed"];

/// Appends the file's top-level items to the top of `outline`, leaving
/// existing content untouched. Returns how many top-level items were added.
/// A malformed file is an `InvalidData` error and adds nothing.
pub fn import_opml(outline: &mut Outline, path: &Path) -> io::Result<usize> {
    let xml = std::fs::read_to_string(path)?;
    import_opml_str(outline, &xml).map_err(|e| io::Error::new(ErrorKind::InvalidData, e))
}

pub fn import_opml_str(outline: &mut Outline, xml: &str) -> Result<usize, roxmltree::Error> {
    // Python's ElementTree accepts a DOCTYPE; roxmltree rejects one by default.
    let options = ParsingOptions {
        allow_dtd: true,
        ..ParsingOptions::default()
    };
    let doc = Document::parse_with_options(xml, options)?;
    let Some(body) = child_elements(doc.root_element(), "body").next() else {
        return Ok(0);
    };
    let placeholder = untouched_placeholder(outline);
    let root = outline.root();
    let mut added = 0;
    for elem in child_elements(body, "outline") {
        let node = build(outline, elem);
        outline.append_child(root, node);
        added += 1;
    }
    // Replace a fresh outline's blank starter item rather than leaving an
    // empty row above the import (the Python version kept it).
    if let (Some(p), true) = (placeholder, added > 0) {
        outline.delete(p);
        if outline.selected_id == Some(p) {
            outline.selected_id = None;
        }
    }
    Ok(added)
}

/// The blank item a new outline starts with, if it's still the only
/// top-level item and hasn't been used in any way.
fn untouched_placeholder(outline: &Outline) -> Option<NodeId> {
    let root = outline.get(outline.root());
    let [only] = root.children[..] else {
        return None;
    };
    let n = outline.get(only);
    let untouched = n.text.is_empty() && n.note.is_empty() && n.children.is_empty() && !n.completed;
    (untouched && !outline.breadcrumb().contains(&only)).then_some(only)
}

fn child_elements<'a, 'input>(
    parent: XmlNode<'a, 'input>,
    tag: &'static str,
) -> impl Iterator<Item = XmlNode<'a, 'input>> {
    parent
        .children()
        .filter(move |n| n.is_element() && n.tag_name().name() == tag)
}

fn build(outline: &mut Outline, elem: XmlNode) -> NodeId {
    let id = outline.create_node(elem.attribute("text").unwrap_or(""));
    let node = outline.get_mut(id);
    // `or`, not first-present: an empty `_note` falls through to `note`, as in Python.
    node.note = [elem.attribute("_note"), elem.attribute("note")]
        .into_iter()
        .flatten()
        .find(|n| !n.is_empty())
        .unwrap_or("")
        .to_string();
    node.completed = COMPLETE_ATTRS
        .iter()
        .find_map(|attr| elem.attribute(*attr))
        .is_some_and(|v| TRUE_STRINGS.contains(&v.trim().to_lowercase().as_str()));

    for child in child_elements(elem, "outline") {
        let child_id = build(outline, child);
        outline.append_child(id, child_id);
    }
    id
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_OPML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<opml version="2.0">
  <head><title>Bulletrix Export</title></head>
  <body>
    <outline text="Groceries" _note="weekly run">
      <outline text="Milk"/>
      <outline text="Eggs" _complete="true"/>
    </outline>
    <outline text="Work">
      <outline text="Write report">
        <outline text="Gather data"/>
        <outline text="Draft outline"/>
      </outline>
    </outline>
  </body>
</opml>
"#;

    /// An outline whose only top-level item is "existing item".
    fn existing_outline() -> Outline {
        let mut outline = Outline::new();
        let first = outline.get(outline.root()).children[0];
        outline.get_mut(first).text = "existing item".into();
        outline
    }

    fn texts(outline: &Outline, ids: &[NodeId]) -> Vec<String> {
        ids.iter().map(|&id| outline.get(id).text.clone()).collect()
    }

    fn top_level(outline: &Outline) -> Vec<NodeId> {
        outline.get(outline.root()).children.clone()
    }

    #[test]
    fn builds_correct_tree_shape() {
        let mut outline = existing_outline();
        import_opml_str(&mut outline, SAMPLE_OPML).unwrap();
        let top = top_level(&outline);
        let (groceries, work) = (top[1], top[2]);

        assert_eq!(outline.get(groceries).note, "weekly run");
        let kids = outline.get(groceries).children.clone();
        assert_eq!(texts(&outline, &kids), ["Milk", "Eggs"]);
        assert!(!outline.get(kids[0]).completed);
        assert!(outline.get(kids[1]).completed);

        let report = outline.get(work).children[0];
        assert_eq!(outline.get(report).text, "Write report");
        let steps = outline.get(report).children.clone();
        assert_eq!(texts(&outline, &steps), ["Gather data", "Draft outline"]);
    }

    #[test]
    fn merges_as_new_top_level_items_with_parent_links() {
        let mut outline = existing_outline();
        let added = import_opml_str(&mut outline, SAMPLE_OPML).unwrap();
        assert_eq!(added, 2);
        let top = top_level(&outline);
        assert_eq!(texts(&outline, &top), ["existing item", "Groceries", "Work"]);

        let groceries = top[1];
        assert_eq!(outline.get(groceries).parent, Some(outline.root()));
        let milk = outline.get(groceries).children[0];
        assert_eq!(outline.get(milk).parent, Some(groceries));
    }

    #[test]
    fn importing_into_a_fresh_outline_replaces_the_blank_starter_item() {
        let mut outline = Outline::new();
        let placeholder = top_level(&outline)[0];
        outline.selected_id = Some(placeholder);

        assert_eq!(import_opml_str(&mut outline, SAMPLE_OPML), Ok(2));
        assert_eq!(texts(&outline, &top_level(&outline)), ["Groceries", "Work"]);
        assert_eq!(outline.selected_id, None, "must not point at the deleted item");
        Outline::from_json_str(&outline.to_json_string()).unwrap();
    }

    #[test]
    fn a_blank_item_that_has_been_used_is_kept() {
        let used: [fn(&mut Outline, NodeId); 3] = [
            |o, id| o.get_mut(id).note = "a note".into(),
            |o, id| o.get_mut(id).completed = true,
            |o, id| o.zoom_in(id),
        ];
        for mark_used in used {
            let mut outline = Outline::new();
            let blank = top_level(&outline)[0];
            mark_used(&mut outline, blank);
            import_opml_str(&mut outline, SAMPLE_OPML).unwrap();
            assert_eq!(top_level(&outline)[0], blank);
        }
    }

    #[test]
    fn a_blank_item_alongside_other_items_is_kept() {
        let mut outline = existing_outline();
        let blank = outline.create_node("");
        outline.append_child(outline.root(), blank);
        import_opml_str(&mut outline, SAMPLE_OPML).unwrap();
        assert_eq!(texts(&outline, &top_level(&outline)), ["existing item", "", "Groceries", "Work"]);
    }

    #[test]
    fn an_import_that_adds_nothing_keeps_the_blank_starter_item() {
        let mut outline = Outline::new();
        let xml = "<opml><body/></opml>";
        assert_eq!(import_opml_str(&mut outline, xml), Ok(0));
        assert_eq!(top_level(&outline).len(), 1);
    }

    #[test]
    fn no_body_imports_nothing() {
        let mut outline = existing_outline();
        let added = import_opml_str(&mut outline, r#"<?xml version="1.0"?><opml version="2.0"><head/></opml>"#);
        assert_eq!(added, Ok(0));
        assert_eq!(top_level(&outline).len(), 1);
    }

    #[test]
    fn accepts_alternate_complete_attribute_names() {
        let xml = r#"<?xml version="1.0"?>
        <opml version="2.0"><body>
          <outline text="a" complete="true"/>
          <outline text="b" completed="1"/>
          <outline text="c" _completed=" YES "/>
          <outline text="d"/>
          <outline text="e" _complete="false" completed="true"/>
        </body></opml>"#;
        let mut outline = existing_outline();
        import_opml_str(&mut outline, xml).unwrap();
        let done: Vec<bool> = top_level(&outline)[1..].iter().map(|&id| outline.get(id).completed).collect();
        // "e": the first attribute present decides, as in Python.
        assert_eq!(done, [true, true, true, false, false]);
    }

    #[test]
    fn empty_underscore_note_falls_back_to_note_and_entities_decode() {
        let xml = r#"<opml><body><outline text="a &amp; b" _note="" note="x &lt; y"/></body></opml>"#;
        let mut outline = existing_outline();
        import_opml_str(&mut outline, xml).unwrap();
        let item = outline.get(top_level(&outline)[1]);
        assert_eq!(item.text, "a & b");
        assert_eq!(item.note, "x < y");
    }

    #[test]
    fn accepts_a_doctype() {
        let xml = r#"<?xml version="1.0"?><!DOCTYPE opml><opml><body><outline text="a"/></body></opml>"#;
        let mut outline = existing_outline();
        assert_eq!(import_opml_str(&mut outline, xml), Ok(1));
    }

    #[test]
    fn malformed_file_is_an_error_and_adds_nothing() {
        let mut outline = existing_outline();
        assert!(import_opml_str(&mut outline, "<not valid xml").is_err());
        assert_eq!(top_level(&outline).len(), 1);
    }

    #[test]
    fn missing_file_is_an_io_error() {
        let mut outline = existing_outline();
        let err = import_opml(&mut outline, Path::new("/definitely/not/here.opml")).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::NotFound);
    }
}
