// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! Images a producer names by path and the renderer loads.

use std::collections::HashMap;

/// Every image the UI has asked for, by id.
///
/// The UI names images by path and never loads them; the renderer asks
/// [`Images::path`] for each id it has not seen and reports the size back
/// with [`Images::set_size`], which `contain`/`cover` fitting needs.
#[derive(Default, Debug)]
pub struct Images {
    paths: Vec<String>,
    index: HashMap<String, u32>,
    sizes: Vec<Option<(u32, u32)>>,
}

impl Images {
    pub fn intern(&mut self, path: &str) -> u32 {
        if let Some(&id) = self.index.get(path) {
            return id;
        }
        let id = self.paths.len() as u32;
        self.paths.push(path.to_string());
        self.sizes.push(None);
        self.index.insert(path.to_string(), id);
        id
    }

    pub fn path(&self, id: u32) -> Option<&str> {
        self.paths.get(id as usize).map(String::as_str)
    }

    pub fn size(&self, id: u32) -> Option<(u32, u32)> {
        self.sizes.get(id as usize).copied().flatten()
    }

    pub fn set_size(&mut self, id: u32, width: u32, height: u32) {
        if let Some(s) = self.sizes.get_mut(id as usize) {
            *s = Some((width, height));
        }
    }

    pub fn len(&self) -> usize {
        self.paths.len()
    }

    pub fn is_empty(&self) -> bool {
        self.paths.is_empty()
    }
}
