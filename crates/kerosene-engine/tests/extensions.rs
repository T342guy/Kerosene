// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! Every crate's copy of an extension says what `kerosene_vfs::ext` says.
//!
//! The asset, audio, script and config crates sit below the VFS and cannot
//! name its table, so they keep their own constant. This is the one place
//! that sees all of them, and so the one place a drift can be caught.

use kerosene_vfs::ext;

#[test]
fn every_copy_of_an_extension_agrees_with_the_table() {
    assert_eq!(kerosene_asset::ext::TEXTURE, ext::TEXTURE);
    assert_eq!(kerosene_asset::ext::MATERIAL, ext::MATERIAL);
    assert_eq!(
        kerosene_asset::ext::MATERIAL_COMPILED,
        ext::MATERIAL_COMPILED
    );
    assert_eq!(kerosene_asset::ext::MODEL, ext::MODEL);
    assert_eq!(kerosene_asset::ext::MAP_SOURCE, ext::MAP);
    assert_eq!(kerosene_asset::ext::MAP_COMPILED, ext::BSP);
    assert_eq!(kerosene_asset::ext::ARCHIVE, ext::ARCHIVE);
    assert_eq!(kerosene_audio::compiled::EXTENSION, ext::AUDIO);
    assert_eq!(kerosene_audio::SCRIPT_EXTENSION, ext::SOUNDSCRIPT);
    assert_eq!(kerosene_script::EXTENSION, ext::SCRIPT);
    assert_eq!(kerosene_engine::save::EXTENSION, ext::SAVE);
    assert_eq!(kerosene_vfs::project::EXTENSION, ext::PROJECT);
    assert_eq!(kerosene_ui::LAYOUT_EXTENSION, ext::UI_LAYOUT);
    assert_eq!(kerosene_ui::STYLE_EXTENSION, ext::UI_STYLE);
    let config = format!(".{}", ext::CONFIG);
    assert!(kerosene_config::FILENAME.ends_with(&config));
    assert!(kerosene_asset::textureset::CONFIG_FILENAME.ends_with(&config));
}
