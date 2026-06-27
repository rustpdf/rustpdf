//! Builds the nested `StructTreeRoot` for Tagged PDF (Fase 7.5) from a flat,
//! ordered list of leaves. Each leaf knows its own role (`P`, `H1`, `Figure`,
//! `TD`, …), the page it lives on, its marked-content id (MCID) and a path of
//! grouping ancestors (`[Table, TR]` for a cell). Groups are created lazily and
//! shared by key prefix, so the same table/row reached twice nests correctly.

use cos::{Dict, Object, PdfString, Reference};
use writer::Document as WriterDoc;

use crate::tag::{StructNode, StructTag};

/// One tagged piece of content (text block or figure).
pub(crate) struct Leaf {
    pub tag: StructTag,
    pub page: Reference,
    pub mcid: i32,
    pub ancestors: Vec<StructNode>,
    pub alt: Option<String>,
    /// For table cells: the column index (used for `/Headers` ↔ `/ID`).
    pub col: Option<usize>,
    /// Inline structure spans nested in this block: `(role, MCID)`. Each becomes
    /// a child structure element of this leaf (e.g. `/Span`).
    pub spans: Vec<(StructTag, i32)>,
    /// Index of the page the leaf belongs to (for the ParentTree number tree).
    pub page_index: usize,
}

/// The Table grouping key in a leaf's ancestor path, if any.
fn table_key(ancestors: &[StructNode]) -> Option<u64> {
    ancestors
        .iter()
        .find(|(t, _)| *t == StructTag::Table)
        .map(|(_, k)| *k)
}

/// Accumulates leaves, then materializes the structure tree into `doc`.
#[derive(Default)]
pub(crate) struct TreeBuilder {
    leaves: Vec<Leaf>,
}

impl TreeBuilder {
    pub fn push(&mut self, leaf: Leaf) {
        self.leaves.push(leaf);
    }

    /// Reserve and emit the whole tree: the `StructTreeRoot`, a `Document`
    /// grouping element, every group and leaf element, and the `ParentTree`.
    /// Returns the `StructTreeRoot` reference and the next free parent key.
    pub fn build(self, doc: &mut WriterDoc, page_count: usize) -> (Reference, usize) {
        let structtree_ref = doc.reserve();
        let docelem_ref = doc.reserve();

        // Group node key prefix -> (reference, tag, ordered children).
        // Groups are keyed by their full ancestor path *including tags*, so a
        // list `L#0` and a table `Table#0` (whose numeric keys both start at 0)
        // never collide.
        let mut group_ref: Vec<(Vec<StructNode>, Reference)> = Vec::new();
        let mut group_tag: Vec<StructTag> = Vec::new();
        let mut group_children: Vec<Vec<Object>> = Vec::new();
        let mut group_parent: Vec<Reference> = Vec::new();
        let mut root_children: Vec<Object> = Vec::new();

        // ParentTree: per page, leaf elements indexed by MCID.
        let mut per_page: Vec<Vec<Object>> = vec![Vec::new(); page_count];

        // Header association (PDF/UA 7.5): give each column header (`TH`) a
        // stable `/ID`, then point every body cell (`TD`) at its column header
        // via `/Headers`. This complements the `/Scope` on the headers.
        let mut header_id: std::collections::BTreeMap<(u64, usize), String> =
            std::collections::BTreeMap::new();
        for leaf in &self.leaves {
            if leaf.tag == StructTag::TH {
                if let (Some(tk), Some(col)) = (table_key(&leaf.ancestors), leaf.col) {
                    header_id
                        .entry((tk, col))
                        .or_insert_with(|| format!("th_{tk}_{col}"));
                }
            }
        }

        let find = |gr: &[(Vec<StructNode>, Reference)], key: &[StructNode]| -> Option<Reference> {
            gr.iter().find(|(k, _)| k == key).map(|(_, r)| *r)
        };

        for leaf in &self.leaves {
            let leaf_ref = doc.reserve();

            // Ensure every ancestor group exists; attach each to its parent.
            let mut keys: Vec<StructNode> = Vec::new();
            let mut parent_ref = docelem_ref;
            for (depth, &(gtag, gkey)) in leaf.ancestors.iter().enumerate() {
                keys.push((gtag, gkey));
                if let Some(r) = find(&group_ref, &keys) {
                    parent_ref = r;
                    continue;
                }
                let gref = doc.reserve();
                group_ref.push((keys.clone(), gref));
                group_tag.push(gtag);
                group_children.push(Vec::new());
                group_parent.push(parent_ref);
                // Register as a child of its parent (root or enclosing group).
                if depth == 0 {
                    root_children.push(Object::Reference(gref));
                } else {
                    let pidx = group_ref
                        .iter()
                        .position(|(k, _)| k == &keys[..depth])
                        .expect("parent group created first");
                    group_children[pidx].push(Object::Reference(gref));
                }
                parent_ref = gref;
            }

            // Attach the leaf to its immediate parent.
            if leaf.ancestors.is_empty() {
                root_children.push(Object::Reference(leaf_ref));
            } else {
                let pidx = group_ref
                    .iter()
                    .position(|(k, _)| k == &keys[..])
                    .expect("deepest group created");
                group_children[pidx].push(Object::Reference(leaf_ref));
            }

            // Emit the leaf element.
            // Inline spans become child structure elements; the leaf's `/K`
            // then mixes its own MCID with those children.
            let k = if leaf.spans.is_empty() {
                Object::Integer(leaf.mcid as i64)
            } else {
                let mut kids = vec![Object::Integer(leaf.mcid as i64)];
                for &(stag, smcid) in &leaf.spans {
                    let span_ref = doc.reserve();
                    doc.assign(
                        span_ref,
                        Dict::new()
                            .with("Type", Object::name("StructElem"))
                            .with("S", Object::name(stag.name()))
                            .with("Pg", leaf.page)
                            .with("K", smcid as i64)
                            .with("P", leaf_ref),
                    );
                    let slot = &mut per_page[leaf.page_index];
                    if smcid as usize >= slot.len() {
                        slot.resize(smcid as usize + 1, Object::Null);
                    }
                    slot[smcid as usize] = Object::Reference(span_ref);
                    kids.push(Object::Reference(span_ref));
                }
                Object::Array(kids)
            };
            let mut d = Dict::new()
                .with("Type", Object::name("StructElem"))
                .with("S", Object::name(leaf.tag.name()))
                .with("Pg", leaf.page)
                .with("K", k)
                .with("P", parent_ref);
            if let Some(alt) = &leaf.alt {
                d.set("Alt", PdfString::text(alt));
            }
            // PDF/UA 7.5: a table header cell must let cells be associated. Our
            // header row is the top row, so mark `TH` as a column scope and give
            // it a stable `/ID` so body cells can reference it.
            if leaf.tag == StructTag::TH {
                d.set(
                    "A",
                    Object::Dict(
                        Dict::new()
                            .with("O", Object::name("Table"))
                            .with("Scope", Object::name("Column")),
                    ),
                );
                if let (Some(tk), Some(col)) = (table_key(&leaf.ancestors), leaf.col) {
                    if let Some(id) = header_id.get(&(tk, col)) {
                        d.set("ID", PdfString::literal(id.clone().into_bytes()));
                    }
                }
            }
            // Body cell → point at its column's header via `/Headers`.
            if leaf.tag == StructTag::TD {
                if let (Some(tk), Some(col)) = (table_key(&leaf.ancestors), leaf.col) {
                    if let Some(id) = header_id.get(&(tk, col)) {
                        d.set(
                            "Headers",
                            Object::Array(vec![Object::String(PdfString::literal(
                                id.clone().into_bytes(),
                            ))]),
                        );
                    }
                }
            }
            doc.assign(leaf_ref, d);

            // Record in the ParentTree at this page's MCID slot.
            let slot = &mut per_page[leaf.page_index];
            if leaf.mcid as usize >= slot.len() {
                slot.resize(leaf.mcid as usize + 1, Object::Null);
            }
            slot[leaf.mcid as usize] = Object::Reference(leaf_ref);
        }

        // Emit group elements now that their children are known.
        for (i, (_, gref)) in group_ref.iter().enumerate() {
            doc.assign(
                *gref,
                Dict::new()
                    .with("Type", Object::name("StructElem"))
                    .with("S", Object::name(group_tag[i].name()))
                    .with("P", group_parent[i])
                    .with("K", Object::Array(std::mem::take(&mut group_children[i]))),
            );
        }

        // Document grouping element.
        doc.assign(
            docelem_ref,
            Dict::new()
                .with("Type", Object::name("StructElem"))
                .with("S", Object::name("Document"))
                .with("P", structtree_ref)
                .with("K", Object::Array(root_children)),
        );

        // ParentTree number tree: page index -> array of leaf elements by MCID.
        let mut nums: Vec<Object> = Vec::new();
        for (page_index, refs) in per_page.into_iter().enumerate() {
            nums.push(Object::Integer(page_index as i64));
            nums.push(Object::Array(refs));
        }
        let parent_tree = doc.add(Dict::new().with("Nums", Object::Array(nums)));

        doc.assign(
            structtree_ref,
            Dict::new()
                .with("Type", Object::name("StructTreeRoot"))
                .with("K", Object::Array(vec![Object::Reference(docelem_ref)]))
                .with("ParentTree", parent_tree)
                .with("ParentTreeNextKey", page_count as i64),
        );

        (structtree_ref, page_count)
    }
}
