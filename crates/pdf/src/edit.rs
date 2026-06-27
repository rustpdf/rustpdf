//! Document manipulation (Fase 6, Tier 2): load an existing PDF, edit its
//! object graph, and write it back. Built on `parser` (read) and `writer`
//! (write).
//!
//! The page tree is flattened to a single level on load, which makes merge,
//! split, reorder, delete and rotate simple list operations; the proper
//! `/Pages` tree is rebuilt on save.

use std::collections::{BTreeMap, BTreeSet};

use cos::{Dict, Object, PdfString, Reference, Stream};
use parser::PdfReader;
use writer::{Document as WriterDoc, PdfVersion};

use crate::encrypt::{self, EncryptConfig, Encryption, Permissions};
use crate::BuildError;

/// An in-memory, editable PDF document.
#[derive(Debug, Clone)]
pub struct EditableDoc {
    objects: BTreeMap<u32, Object>,
    catalog: u32,
    pages_root: u32,
    info: Option<u32>,
    metadata: Option<u32>,
    page_order: Vec<u32>,
    next_num: u32,
    encryption: Option<EncryptConfig>,
    /// Emit object streams + a cross-reference stream on save (smaller files).
    compress: bool,
}

impl EditableDoc {
    /// Load and parse an existing PDF (empty password).
    pub fn load(bytes: impl AsRef<[u8]>) -> Result<EditableDoc, parser::PdfError> {
        Self::load_with_password(bytes, b"")
    }

    /// Load an encrypted PDF with a password.
    pub fn load_with_password(
        bytes: impl AsRef<[u8]>,
        password: &[u8],
    ) -> Result<EditableDoc, parser::PdfError> {
        let reader = PdfReader::parse_with_password(bytes, password)?;
        let mut objects = BTreeMap::new();
        for n in reader.object_numbers() {
            if let Some(o) = reader.get(n) {
                objects.insert(n, o.clone());
            }
        }
        let catalog = reference_num(reader.trailer().get("Root"))
            .ok_or_else(|| parser::PdfError::Syntax("no /Root".into()))?;
        let info = reference_num(reader.trailer().get("Info"));
        let pages_root = as_dict(objects.get(&catalog))
            .and_then(|d| reference_num(d.get("Pages")))
            .ok_or_else(|| parser::PdfError::Syntax("no /Pages".into()))?;
        let metadata =
            as_dict(objects.get(&catalog)).and_then(|d| reference_num(d.get("Metadata")));

        let mut page_order = Vec::new();
        let mut seen = Vec::new();
        collect_pages(&objects, pages_root, &mut page_order, &mut seen);

        let next_num = objects.keys().copied().max().unwrap_or(0) + 1;
        Ok(EditableDoc {
            objects,
            catalog,
            pages_root,
            info,
            metadata,
            page_order,
            next_num,
            encryption: None,
            compress: false,
        })
    }

    /// Number of pages.
    pub fn page_count(&self) -> usize {
        self.page_order.len()
    }

    /// The page object numbers, in order.
    pub fn page_numbers(&self) -> &[u32] {
        &self.page_order
    }

    fn allocate(&mut self) -> u32 {
        let n = self.next_num;
        self.next_num += 1;
        n
    }

    // ---- 6.3 rotate / reorder / delete -----------------------------------

    /// Rotate page `index` by `degrees` (added to any existing `/Rotate`).
    pub fn rotate_page(&mut self, index: usize, degrees: i32) {
        let Some(&num) = self.page_order.get(index) else {
            return;
        };
        let current = as_dict(self.objects.get(&num))
            .and_then(|d| d.get("Rotate"))
            .and_then(int)
            .unwrap_or(0);
        let rot = (((current as i32 + degrees) % 360) + 360) % 360;
        self.update_dict(num, |d| {
            d.set("Rotate", rot as i64);
        });
    }

    /// Delete page `index` (the object becomes unreferenced; `optimize` drops it).
    pub fn delete_page(&mut self, index: usize) {
        if index < self.page_order.len() {
            self.page_order.remove(index);
        }
    }

    /// Reorder pages by a permutation of current indices.
    pub fn reorder_pages(&mut self, new_order: &[usize]) {
        let reordered: Vec<u32> = new_order
            .iter()
            .filter_map(|&i| self.page_order.get(i).copied())
            .collect();
        if reordered.len() == self.page_order.len() {
            self.page_order = reordered;
        }
    }

    // ---- 6.1 merge --------------------------------------------------------

    /// Append all pages of `other` to this document (Fase 6.1).
    pub fn merge(&mut self, other: &EditableDoc) {
        let base = self.next_num - 1; // other's object N becomes N + base
        let map: BTreeMap<u32, u32> = other.objects.keys().map(|&old| (old, old + base)).collect();

        for (&old, obj) in &other.objects {
            // Skip the other document's structural roots; we keep our own.
            if old == other.catalog || old == other.pages_root {
                continue;
            }
            let mut copy = obj.clone();
            remap(&mut copy, &map);
            let new_num = old + base;
            self.objects.insert(new_num, copy);
            self.next_num = self.next_num.max(new_num + 1);
        }
        for &p in &other.page_order {
            self.page_order.push(p + base);
        }
    }

    // ---- 6.2 split / extract ---------------------------------------------

    /// Build a new document containing only the pages at `indices`, with all
    /// their dependencies copied and compactly renumbered (Fase 6.2).
    pub fn extract_pages(&self, indices: &[usize]) -> EditableDoc {
        let roots: Vec<u32> = indices
            .iter()
            .filter_map(|&i| self.page_order.get(i).copied())
            .collect();

        // Reachability from the selected pages, not following `/Parent` upward.
        let mut keep = BTreeSet::new();
        let mut stack = roots.clone();
        while let Some(n) = stack.pop() {
            if !keep.insert(n) {
                continue;
            }
            if let Some(obj) = self.objects.get(&n) {
                let mut refs = Vec::new();
                collect_refs_skipping(obj, &["Parent"], &mut refs);
                stack.extend(refs);
            }
        }

        // Compact renumbering: 1 = catalog, 2 = pages root, then kept objects.
        let mut map = BTreeMap::new();
        let mut next = 3u32;
        for &n in &keep {
            map.insert(n, next);
            next += 1;
        }

        let mut objects = BTreeMap::new();
        for &n in &keep {
            let mut copy = self.objects.get(&n).unwrap().clone();
            remap(&mut copy, &map);
            objects.insert(map[&n], copy);
        }
        let page_order: Vec<u32> = roots.iter().map(|n| map[n]).collect();

        // Fresh catalog + pages root.
        objects.insert(
            1,
            Object::Dict(
                Dict::new()
                    .with("Type", Object::name("Catalog"))
                    .with("Pages", Reference::new(2)),
            ),
        );
        objects.insert(
            2,
            Object::Dict(Dict::new().with("Type", Object::name("Pages"))),
        );

        EditableDoc {
            objects,
            catalog: 1,
            pages_root: 2,
            info: None,
            metadata: None,
            page_order,
            next_num: next,
            encryption: None,
            compress: false,
        }
    }

    // ---- 6.5 metadata -----------------------------------------------------

    /// Set an `/Info` string entry (creating the Info dict if needed).
    pub fn set_info(&mut self, key: &str, value: &str) {
        let info = match self.info {
            Some(n) => n,
            None => {
                let n = self.allocate();
                self.objects.insert(n, Object::Dict(Dict::new()));
                self.info = Some(n);
                n
            }
        };
        let val = Object::String(PdfString::literal(value.as_bytes().to_vec()));
        self.update_dict(info, |d| {
            d.set(key, val.clone());
        });
    }

    /// Read an `/Info` string entry.
    pub fn get_info(&self, key: &str) -> Option<String> {
        let info = self.info?;
        match as_dict(self.objects.get(&info))?.get(key)? {
            Object::String(s) => Some(String::from_utf8_lossy(s.as_bytes()).into_owned()),
            _ => None,
        }
    }

    /// Set the XMP metadata stream (`/Metadata`, uncompressed XML).
    pub fn set_xmp(&mut self, xml: impl Into<Vec<u8>>) {
        let dict = Dict::new()
            .with("Type", Object::name("Metadata"))
            .with("Subtype", Object::name("XML"));
        let stream = Object::Stream(Stream::with_dict(dict, xml.into()));
        let num = match self.metadata {
            Some(n) => {
                self.objects.insert(n, stream);
                n
            }
            None => {
                let n = self.allocate();
                self.objects.insert(n, stream);
                self.metadata = Some(n);
                n
            }
        };
        self.update_dict(self.catalog, |d| {
            d.set("Metadata", Reference::new(num));
        });
    }

    /// Read the raw XMP metadata bytes, if present.
    pub fn get_xmp(&self) -> Option<Vec<u8>> {
        let num = self.metadata?;
        match self.objects.get(&num)? {
            Object::Stream(s) => Some(s.data.clone()),
            _ => None,
        }
    }

    // ---- 6.6 overlay / watermark -----------------------------------------

    /// Append `content` (raw content-stream operators) on top of page `index`.
    /// The bytes are wrapped in `q`/`Q` so they cannot leak graphics state.
    /// `extra_resources` are merged into the page's `/Resources`.
    pub fn overlay_page(&mut self, index: usize, content: &[u8], extra_resources: Option<Dict>) {
        let Some(&page) = self.page_order.get(index) else {
            return;
        };
        let mut wrapped = b"q\n".to_vec();
        wrapped.extend_from_slice(content);
        wrapped.extend_from_slice(b"\nQ\n");
        let overlay_ref = self.allocate();
        self.objects
            .insert(overlay_ref, Object::Stream(Stream::new(wrapped)));

        // Build the new /Contents array: existing streams, then the overlay.
        let mut contents = match as_dict(self.objects.get(&page)).and_then(|d| d.get("Contents")) {
            Some(Object::Reference(r)) => vec![Object::Reference(*r)],
            Some(Object::Array(a)) => a.clone(),
            _ => Vec::new(),
        };
        contents.push(Object::Reference(Reference::new(overlay_ref)));

        if let Some(extra) = extra_resources {
            self.merge_resources(page, extra);
        }
        self.update_dict(page, |d| {
            d.set("Contents", Object::Array(contents.clone()));
        });
    }

    fn merge_resources(&mut self, page: u32, extra: Dict) {
        let mut res = as_dict(self.objects.get(&page))
            .and_then(|d| match d.get("Resources") {
                Some(Object::Dict(r)) => Some(r.clone()),
                _ => None,
            })
            .unwrap_or_default();
        for (k, v) in extra.iter() {
            res.set(k.clone(), v.clone());
        }
        self.update_dict(page, |d| {
            d.set("Resources", Object::Dict(res.clone()));
        });
    }

    // ---- 6.7 AcroForm fill (partial) -------------------------------------

    /// Set a text field's value by its `/T` name and request that viewers
    /// regenerate appearances (`NeedAppearances true`). Returns whether a field
    /// matched. Appearance-stream generation is not yet implemented, so the
    /// field renders via the viewer's appearance generator.
    pub fn fill_text_field(&mut self, field_name: &str, value: &str) -> bool {
        let Some(acro) = self.acroform_num() else {
            return false;
        };
        let fields = match as_dict(self.objects.get(&acro)).and_then(|d| d.get("Fields")) {
            Some(Object::Array(a)) => a.clone(),
            _ => return false,
        };

        let mut found = false;
        for field in &fields {
            if let Object::Reference(r) = field {
                let matches = as_dict(self.objects.get(&r.number))
                    .and_then(|d| d.get("T"))
                    .and_then(string_value)
                    .as_deref()
                    == Some(field_name);
                if matches {
                    let v = Object::String(PdfString::literal(value.as_bytes().to_vec()));
                    self.update_dict(r.number, |d| {
                        d.set("V", v.clone());
                    });
                    found = true;
                }
            }
        }
        if found {
            self.update_dict(acro, |d| {
                d.set("NeedAppearances", Object::Bool(true));
            });
        }
        found
    }

    fn acroform_num(&self) -> Option<u32> {
        let acro = as_dict(self.objects.get(&self.catalog))?.get("AcroForm")?;
        match acro {
            Object::Reference(r) => Some(r.number),
            // A direct AcroForm dict: promote it to an indirect object.
            _ => None,
        }
    }

    // ---- 6.8 optimize -----------------------------------------------------

    /// Opt into object streams + a cross-reference stream on the next
    /// `to_bytes`/`save` (Fase 6.8). Ignored while encryption is enabled.
    pub fn compact(&mut self, on: bool) {
        self.compress = on;
    }

    /// Remove unreferenced objects and Flate-compress uncompressed streams,
    /// compact the numbering, then emit object streams + a cross-reference
    /// stream on save for the smallest output (Fase 6.8).
    pub fn optimize(&mut self) {
        self.compress = true;
        // Finalize structure first so the catalog/pages reflect edits.
        let mut objects = self.finalize();

        // Deduplicate byte-identical objects (e.g. fonts/resources repeated after
        // a merge): point every reference at one canonical copy; the duplicates
        // become unreachable and are dropped below. Pages and the catalog/pages
        // tree are excluded so identical blank pages stay distinct.
        let protected: BTreeSet<u32> = self
            .page_order
            .iter()
            .copied()
            .chain([self.catalog, self.pages_root])
            .collect();
        let mut canonical: BTreeMap<Vec<u8>, u32> = BTreeMap::new();
        let mut dedup: BTreeMap<u32, u32> = BTreeMap::new();
        for (&num, obj) in &objects {
            if protected.contains(&num) {
                continue;
            }
            let mut bytes = Vec::new();
            obj.write_to(&mut bytes);
            match canonical.get(&bytes) {
                Some(&c) => {
                    dedup.insert(num, c);
                }
                None => {
                    canonical.insert(bytes, num);
                }
            }
        }
        if !dedup.is_empty() {
            for obj in objects.values_mut() {
                remap(obj, &dedup);
            }
            for dup in dedup.keys() {
                objects.remove(dup);
            }
        }

        // Reachability from the catalog (follow everything, including /Parent).
        let mut keep = BTreeSet::new();
        let mut stack = vec![self.catalog];
        if let Some(i) = self.info {
            stack.push(i);
        }
        while let Some(n) = stack.pop() {
            if !keep.insert(n) {
                continue;
            }
            if let Some(obj) = objects.get(&n) {
                let mut refs = Vec::new();
                collect_refs_skipping(obj, &[], &mut refs);
                stack.extend(refs);
            }
        }

        // Compress uncompressed streams (skip Metadata and image XObjects).
        for obj in objects.values_mut() {
            if let Object::Stream(s) = obj {
                let has_filter = s.dict.contains_key("Filter");
                let subtype = s.dict.get("Subtype").and_then(name).unwrap_or_default();
                let ty = s.dict.get("Type").and_then(name).unwrap_or_default();
                if !has_filter && subtype != "Image" && ty != "Metadata" && !s.data.is_empty() {
                    let compressed = images::flate_encode(&s.data);
                    if compressed.len() < s.data.len() {
                        s.data = compressed;
                        s.dict.set("Filter", Object::name("FlateDecode"));
                    }
                }
            }
        }

        // Compact renumber kept objects.
        let mut map = BTreeMap::new();
        let mut next = 1u32;
        for &n in &keep {
            map.insert(n, next);
            next += 1;
        }
        let mut compacted = BTreeMap::new();
        for &n in &keep {
            if let Some(obj) = objects.get(&n) {
                let mut copy = obj.clone();
                remap(&mut copy, &map);
                compacted.insert(map[&n], copy);
            }
        }

        self.catalog = map.get(&self.catalog).copied().unwrap_or(self.catalog);
        self.pages_root = map
            .get(&self.pages_root)
            .copied()
            .unwrap_or(self.pages_root);
        self.info = self.info.and_then(|n| map.get(&n).copied());
        self.metadata = self.metadata.and_then(|n| map.get(&n).copied());
        self.page_order = self
            .page_order
            .iter()
            .filter_map(|n| map.get(n).copied())
            .collect();
        self.next_num = next;
        self.objects = compacted;
    }

    // ---- 7.3 encryption ---------------------------------------------------

    /// Encrypt the document with AES-128 (V4/R4) on the next `to_bytes`/`save`.
    /// An empty `user_password` lets viewers open it while honoring `perms`.
    pub fn encrypt(&mut self, user_password: &str, owner_password: &str, perms: Permissions) {
        self.encrypt_with(Encryption::Aes128, user_password, owner_password, perms);
    }

    /// Encrypt with an explicit cipher (RC4-128, AES-128 or AES-256/R6).
    pub fn encrypt_with(
        &mut self,
        method: Encryption,
        user_password: &str,
        owner_password: &str,
        perms: Permissions,
    ) {
        self.encryption = Some(EncryptConfig {
            user: user_password.as_bytes().to_vec(),
            owner: owner_password.as_bytes().to_vec(),
            perms,
            method,
        });
    }

    // ---- serialization ----------------------------------------------------

    /// Serialize to PDF bytes.
    pub fn to_bytes(&self) -> Result<Vec<u8>, BuildError> {
        let mut objects = self.finalize();
        let mut encrypt_ref = None;
        let mut id = None;

        if let Some(cfg) = &self.encryption {
            crate::require(license::Feature::Encryption)?;
            let id0 = encrypt::derive_id(objects.len());
            let prepared = encrypt::prepare(cfg, id0.clone());
            // Encrypt every object's strings/streams (generation 0).
            let mut enc: BTreeMap<u32, Object> = BTreeMap::new();
            for (&num, obj) in &objects {
                enc.insert(num, prepared.encrypt_object(num, obj));
            }
            // The /Encrypt dict itself is added afterwards (never encrypted).
            let enc_num = objects.keys().copied().max().unwrap_or(0) + 1;
            enc.insert(enc_num, Object::Dict(prepared.dict.clone()));
            objects = enc;
            encrypt_ref = Some(enc_num);
            id = Some([id0.clone(), id0]);
        }

        let max = objects.keys().copied().max().unwrap_or(0);
        let mut w = WriterDoc::new(PdfVersion::V1_7);
        for _ in 0..max {
            w.reserve();
        }
        for n in 1..=max {
            w.assign(Reference::new(n), Object::Null);
        }
        for (&n, obj) in &objects {
            w.assign(Reference::new(n), normalize(obj));
        }
        w.set_root(Reference::new(self.catalog));
        // Object streams (no effect when encrypting; the writer guards on that).
        w.set_object_streams(self.compress);
        if let Some(info) = self.info {
            w.set_info(Reference::new(info));
        }
        if let Some(r) = encrypt_ref {
            w.set_encrypt(Reference::new(r));
        }
        if let Some(id) = id {
            w.set_id(id);
        }
        Ok(w.write()?)
    }

    /// Save to a file.
    pub fn save(&self, path: impl AsRef<std::path::Path>) -> std::io::Result<()> {
        let bytes = self
            .to_bytes()
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;
        std::fs::write(path, bytes)
    }

    /// Serialize as an **incremental update** appended to `original`: only the
    /// objects that differ from (or are new since) the original file are
    /// written, followed by a fresh classic cross-reference section chained to
    /// the previous one via `/Prev`. The original bytes are preserved verbatim,
    /// so existing signatures over them stay valid. Encryption/object-stream
    /// settings are ignored for the appended section (it is a classic update).
    pub fn to_bytes_incremental(&self, original: &[u8]) -> Result<Vec<u8>, BuildError> {
        let prev_startxref = find_last_startxref(original)
            .ok_or_else(|| BuildError::Parse("no startxref".into()))?;
        let reader =
            PdfReader::parse(original).map_err(|e| BuildError::Parse(format!("original: {e}")))?;

        // Serialized form of each object in the original, to diff against.
        let mut orig: BTreeMap<u32, Vec<u8>> = BTreeMap::new();
        for n in reader.object_numbers() {
            if let Some(o) = reader.get(n) {
                let mut b = Vec::new();
                normalize(o).write_to(&mut b);
                orig.insert(n, b);
            }
        }

        let current = self.finalize();
        // Changed or newly-added objects (by serialized bytes).
        let mut changed: Vec<(u32, Vec<u8>)> = Vec::new();
        for (&num, obj) in &current {
            let mut b = Vec::new();
            normalize(obj).write_to(&mut b);
            if orig.get(&num) != Some(&b) {
                changed.push((num, b));
            }
        }
        // Objects deleted since the original become free entries.
        let deleted: Vec<u32> = orig
            .keys()
            .copied()
            .filter(|n| !current.contains_key(n))
            .collect();

        let mut out = original.to_vec();
        if !out.ends_with(b"\n") {
            out.push(b'\n');
        }

        // Append each changed object, recording its new offset.
        let mut offsets: BTreeMap<u32, u64> = BTreeMap::new();
        for (num, _) in &changed {
            let obj = &current[num];
            offsets.insert(*num, out.len() as u64);
            out.extend_from_slice(format!("{num} 0 obj\n").as_bytes());
            normalize(obj).write_to(&mut out);
            out.extend_from_slice(b"\nendobj\n");
        }

        // Build the incremental classic xref: changed (type n) + deleted (free),
        // grouped into contiguous subsections.
        let mut entries: BTreeMap<u32, Option<u64>> = BTreeMap::new();
        for (num, off) in &offsets {
            entries.insert(*num, Some(*off));
        }
        for d in &deleted {
            entries.insert(*d, None);
        }
        let xref_offset = out.len();
        out.extend_from_slice(b"xref\n");
        let nums: Vec<u32> = entries.keys().copied().collect();
        let mut i = 0;
        while i < nums.len() {
            let start = nums[i];
            let mut j = i;
            while j + 1 < nums.len() && nums[j + 1] == nums[j] + 1 {
                j += 1;
            }
            let count = j - i + 1;
            out.extend_from_slice(format!("{start} {count}\n").as_bytes());
            for &n in &nums[i..=j] {
                match entries[&n] {
                    Some(off) => out.extend_from_slice(format!("{off:010} 00000 n\r\n").as_bytes()),
                    None => out.extend_from_slice(b"0000000000 65535 f\r\n"),
                }
            }
            i = j + 1;
        }

        // Trailer chained to the previous section.
        let orig_size = reader
            .trailer()
            .get("Size")
            .and_then(|o| match o {
                Object::Integer(n) => Some(*n as u32),
                _ => None,
            })
            .unwrap_or(0);
        let max_num = current.keys().copied().max().unwrap_or(0);
        let size = orig_size.max(max_num + 1);

        let mut trailer = Dict::new()
            .with("Size", size as i64)
            .with("Root", Reference::new(self.catalog))
            .with("Prev", Object::Integer(prev_startxref as i64));
        if let Some(info) = self.info {
            trailer.set("Info", Reference::new(info));
        }
        if let Some(id) = reader.trailer().get("ID") {
            trailer.set("ID", id.clone());
        }
        out.extend_from_slice(b"trailer\n");
        Object::Dict(trailer).write_to(&mut out);
        out.extend_from_slice(b"\nstartxref\n");
        out.extend_from_slice(format!("{xref_offset}").as_bytes());
        out.extend_from_slice(b"\n%%EOF\n");
        Ok(out)
    }

    /// Save an incremental update over the file at `original_path`.
    pub fn save_incremental(
        &self,
        original_path: impl AsRef<std::path::Path>,
        out_path: impl AsRef<std::path::Path>,
    ) -> std::io::Result<()> {
        let original = std::fs::read(original_path)?;
        let bytes = self
            .to_bytes_incremental(&original)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;
        std::fs::write(out_path, bytes)
    }

    /// Produce the finalized object map: rebuilt page tree, fixed catalog and
    /// per-page `/Parent`/`/Type`.
    fn finalize(&self) -> BTreeMap<u32, Object> {
        let mut objects = self.objects.clone();

        // Rebuild the pages root.
        let kids: Vec<Object> = self
            .page_order
            .iter()
            .map(|&p| Object::Reference(Reference::new(p)))
            .collect();
        let pages_dict = Dict::new()
            .with("Type", Object::name("Pages"))
            .with("Kids", Object::Array(kids))
            .with("Count", self.page_order.len() as i64);
        objects.insert(self.pages_root, Object::Dict(pages_dict));

        // Fix each page's Parent and Type.
        for &p in &self.page_order {
            if let Some(Object::Dict(mut d)) = objects.get(&p).cloned() {
                d.set("Type", Object::name("Page"));
                d.set("Parent", Reference::new(self.pages_root));
                objects.insert(p, Object::Dict(d));
            }
        }

        // Ensure the catalog is well-formed.
        let mut catalog = as_dict(objects.get(&self.catalog))
            .cloned()
            .unwrap_or_default();
        catalog.set("Type", Object::name("Catalog"));
        catalog.set("Pages", Reference::new(self.pages_root));
        if let Some(m) = self.metadata {
            catalog.set("Metadata", Reference::new(m));
        }
        objects.insert(self.catalog, Object::Dict(catalog));

        objects
    }

    /// Apply a mutation to a dictionary object in place.
    fn update_dict(&mut self, num: u32, f: impl FnOnce(&mut Dict)) {
        if let Some(Object::Dict(d)) = self.objects.get(&num).cloned().as_mut() {
            f(d);
            self.objects.insert(num, Object::Dict(d.clone()));
        } else if let Some(Object::Stream(s)) = self.objects.get(&num).cloned().as_mut() {
            f(&mut s.dict);
            self.objects.insert(num, Object::Stream(s.clone()));
        } else {
            // Object missing or not a dict: create a fresh dict.
            let mut d = Dict::new();
            f(&mut d);
            self.objects.insert(num, Object::Dict(d));
        }
    }
}

// ---- free helpers ----------------------------------------------------------

fn as_dict(obj: Option<&Object>) -> Option<&Dict> {
    match obj? {
        Object::Dict(d) => Some(d),
        Object::Stream(s) => Some(&s.dict),
        _ => None,
    }
}

fn reference_num(obj: Option<&Object>) -> Option<u32> {
    match obj? {
        Object::Reference(r) => Some(r.number),
        _ => None,
    }
}

fn int(o: &Object) -> Option<i64> {
    match o {
        Object::Integer(n) => Some(*n),
        _ => None,
    }
}

fn name(o: &Object) -> Option<String> {
    match o {
        Object::Name(n) => Some(n.as_str().to_string()),
        _ => None,
    }
}

fn string_value(o: &Object) -> Option<String> {
    match o {
        Object::String(s) => Some(String::from_utf8_lossy(s.as_bytes()).into_owned()),
        _ => None,
    }
}

/// Find the byte offset declared by the file's last `startxref` (the offset of
/// the most recent cross-reference section), for chaining an incremental `/Prev`.
fn find_last_startxref(bytes: &[u8]) -> Option<u64> {
    let needle = b"startxref";
    let pos = bytes.windows(needle.len()).rposition(|w| w == needle)?;
    let mut i = pos + needle.len();
    // Skip whitespace, then read the decimal offset.
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    let start = i;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    std::str::from_utf8(&bytes[start..i]).ok()?.parse().ok()
}

fn collect_pages(
    objects: &BTreeMap<u32, Object>,
    node: u32,
    out: &mut Vec<u32>,
    seen: &mut Vec<u32>,
) {
    if seen.contains(&node) {
        return;
    }
    seen.push(node);
    let Some(dict) = as_dict(objects.get(&node)) else {
        return;
    };
    match dict.get("Type").and_then(name).as_deref() {
        Some("Pages") => {
            if let Some(Object::Array(kids)) = dict.get("Kids") {
                for kid in kids {
                    if let Object::Reference(r) = kid {
                        collect_pages(objects, r.number, out, seen);
                    }
                }
            }
        }
        // A Page (or an untyped leaf): treat as a page.
        _ => out.push(node),
    }
}

/// Remap every indirect reference number in `obj` via `map`.
fn remap(obj: &mut Object, map: &BTreeMap<u32, u32>) {
    match obj {
        Object::Reference(r) => {
            if let Some(&n) = map.get(&r.number) {
                r.number = n;
            }
        }
        Object::Array(items) => {
            for it in items {
                remap(it, map);
            }
        }
        Object::Dict(d) => *d = remap_dict(d, map),
        Object::Stream(s) => s.dict = remap_dict(&s.dict, map),
        _ => {}
    }
}

fn remap_dict(dict: &Dict, map: &BTreeMap<u32, u32>) -> Dict {
    let mut out = Dict::new();
    for (k, v) in dict.iter() {
        let mut v = v.clone();
        remap(&mut v, map);
        out.set(k.clone(), v);
    }
    out
}

/// Collect referenced object numbers, skipping the named dict keys.
fn collect_refs_skipping(obj: &Object, skip_keys: &[&str], out: &mut Vec<u32>) {
    match obj {
        Object::Reference(r) => out.push(r.number),
        Object::Array(items) => {
            for it in items {
                collect_refs_skipping(it, skip_keys, out);
            }
        }
        Object::Dict(d) => {
            for (k, v) in d.iter() {
                if !skip_keys.contains(&k.as_str()) {
                    collect_refs_skipping(v, skip_keys, out);
                }
            }
        }
        Object::Stream(s) => {
            for (k, v) in s.dict.iter() {
                if !skip_keys.contains(&k.as_str()) {
                    collect_refs_skipping(v, skip_keys, out);
                }
            }
        }
        _ => {}
    }
}

/// Drop `/Length` so the writer recomputes it from the (possibly changed) data.
fn normalize(obj: &Object) -> Object {
    match obj {
        Object::Stream(s) => {
            let mut dict = Dict::new();
            for (k, v) in s.dict.iter() {
                if k.as_str() != "Length" {
                    dict.set(k.clone(), v.clone());
                }
            }
            Object::Stream(Stream {
                dict,
                data: s.data.clone(),
            })
        }
        other => other.clone(),
    }
}
