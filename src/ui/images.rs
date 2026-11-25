use std::collections::{HashMap, HashSet};

use image::DynamicImage;
use ratatui_image::picker::Picker;
use ratatui_image::protocol::StatefulProtocol;

pub struct ImageStore {
    picker: Picker,
    protocols: HashMap<String, StatefulProtocol>,
    dims: HashMap<String, (u32, u32)>,
    font_size: (u16, u16),
    pending: HashSet<String>,
    failed: HashSet<String>,
    enabled: bool,
}

impl ImageStore {
    pub fn new(picker: Picker, enabled: bool) -> Self {
        let fs = picker.font_size();
        Self {
            picker,
            protocols: HashMap::new(),
            dims: HashMap::new(),
            font_size: (fs.width.max(1), fs.height.max(1)),
            pending: HashSet::new(),
            failed: HashSet::new(),
            enabled,
        }
    }

    /// Number of terminal rows an image should occupy when fit to `cols` columns.
    pub fn rows_for(&self, key: &str, cols: u16) -> u16 {
        const PLACEHOLDER_ROWS: u16 = 8;
        let Some(&(w, h)) = self.dims.get(key) else {
            return PLACEHOLDER_ROWS;
        };
        if w == 0 || h == 0 || cols == 0 {
            return PLACEHOLDER_ROWS;
        }
        let (fw, fh) = self.font_size;
        let target_px_w = f32::from(cols) * f32::from(fw);
        let scale = target_px_w / f32::from(w as u16);
        let px_h = f32::from(h as u16) * scale;
        let rows = (px_h / f32::from(fh)).ceil() as u16;
        rows.max(1)
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn has(&self, key: &str) -> bool {
        self.protocols.contains_key(key)
    }

    pub fn is_pending(&self, key: &str) -> bool {
        self.pending.contains(key)
    }

    pub fn is_failed(&self, key: &str) -> bool {
        self.failed.contains(key)
    }

    pub fn mark_pending(&mut self, key: &str) {
        self.pending.insert(key.to_string());
    }

    pub fn mark_failed(&mut self, key: &str) {
        self.pending.remove(key);
        self.failed.insert(key.to_string());
    }

    pub fn insert(&mut self, key: &str, image: DynamicImage) {
        self.pending.remove(key);
        self.failed.remove(key);
        self.dims
            .insert(key.to_string(), (image.width(), image.height()));
        let protocol = self.picker.new_resize_protocol(image);
        self.protocols.insert(key.to_string(), protocol);
    }

    pub fn get(&mut self, key: &str) -> Option<&mut StatefulProtocol> {
        self.protocols.get_mut(key)
    }

    /// Drop cached protocols for keys no longer referenced, bounding memory.
    pub fn retain(&mut self, keep: &HashSet<String>) {
        self.protocols.retain(|k, _| keep.contains(k));
    }
}
