//! Pictures of image nodes. The host decodes them (the browser, Android's
//! BitmapFactory), since only it can fetch every kind of address and
//! decode every format; layout reports the sources it still needs.

use std::collections::{BTreeSet, HashMap, HashSet};

use skia_safe::Image;

#[derive(Default)]
pub(crate) struct Images {
    /// Decoded images and their pixels per CSS pixel.
    loaded: HashMap<String, (Image, f32)>,
    failed: HashSet<String>,
    /// Sources laid out without an image, not yet reported.
    pub missing: BTreeSet<String>,
    /// Sources reported and not answered yet.
    requested: HashSet<String>,
}

impl Images {
    pub fn add(&mut self, src: &str, image: Image, scale: f32) {
        self.failed.remove(src);
        self.requested.remove(src);
        self.missing.remove(src);
        self.loaded.insert(src.to_owned(), (image, scale.max(0.01)));
    }

    /// Forgets the pictures of sources not in `live` (images deleted from
    /// the document, or a document replaced).
    pub fn retain(&mut self, live: &HashSet<String>) {
        self.loaded.retain(|src, _| live.contains(src));
        self.failed.retain(|src| live.contains(src));
        self.requested.retain(|src| live.contains(src));
        self.missing.retain(|src| live.contains(src));
    }

    /// Forgets that `src` failed, so layout asks for it again; false when
    /// it had not failed.
    pub fn retry(&mut self, src: &str) -> bool {
        self.failed.remove(src)
    }

    pub fn fail(&mut self, src: &str) {
        self.requested.remove(src);
        self.missing.remove(src);
        self.failed.insert(src.to_owned());
    }

    /// Size in CSS pixels of a loaded image.
    pub fn size(&self, src: &str) -> Option<(f32, f32)> {
        let (image, scale) = self.loaded.get(src)?;
        let (w, h) = (image.width() as f32, image.height() as f32);
        (w > 0.0 && h > 0.0).then(|| (w / scale, h / scale))
    }

    /// The image for `src` and whether it failed; an unknown source is
    /// noted as missing.
    pub fn lookup(&mut self, src: &str) -> (Option<Image>, bool) {
        if let Some((image, _)) = self.loaded.get(src) {
            return (Some(image.clone()), false);
        }
        if self.failed.contains(src) {
            return (None, true);
        }
        if self.requested.insert(src.to_owned()) {
            self.missing.insert(src.to_owned());
        }
        (None, false)
    }
}
