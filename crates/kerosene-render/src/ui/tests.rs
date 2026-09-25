use super::*;
use kerosene_ui::Quad;

fn quad(texture: TextureRef, additive: bool) -> DrawItem {
    DrawItem::Quad(Quad {
        texture,
        additive,
        ..Default::default()
    })
}

#[test]
fn solid_quads_and_glyphs_share_a_batch() {
    let list = DisplayList {
        items: vec![
            quad(TextureRef::None, false),
            quad(TextureRef::Glyphs, false),
            quad(TextureRef::None, false),
        ],
        size: (100, 100),
    };
    let (quads, batches) = pack(&list, |_| true);
    assert_eq!(quads.len(), 3);
    assert_eq!(batches.len(), 1);
    assert_eq!(quads[1].modes[0], 1);
}

#[test]
fn images_blend_modes_and_clips_split_batches() {
    let list = DisplayList {
        items: vec![
            quad(TextureRef::None, false),
            quad(TextureRef::Image(3), false),
            quad(TextureRef::Image(4), false),
            quad(TextureRef::None, true),
            DrawItem::Clip(Some([0, 0, 10, 10])),
            quad(TextureRef::None, true),
        ],
        size: (100, 100),
    };
    let (_, batches) = pack(&list, |_| true);
    let shape: Vec<(u32, u32, Option<u32>, bool, bool)> = batches
        .iter()
        .map(|b| (b.start, b.end, b.image, b.additive, b.scissor.is_some()))
        .collect();
    assert_eq!(
        shape,
        vec![
            (0, 2, Some(3), false, false),
            (2, 3, Some(4), false, false),
            (3, 4, None, true, false),
            (4, 5, None, true, true),
        ]
    );
}

#[test]
fn a_missing_image_takes_only_itself_out() {
    let list = DisplayList {
        items: vec![
            quad(TextureRef::None, false),
            quad(TextureRef::Image(9), false),
            quad(TextureRef::Glyphs, false),
        ],
        size: (100, 100),
    };
    let (quads, batches) = pack(&list, |id| id != 9);
    assert_eq!(quads.len(), 2);
    assert_eq!(batches.len(), 1);
    assert_eq!(batches[0].image, None);
}
