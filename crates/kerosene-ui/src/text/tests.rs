use super::*;
use crate::style::TextAlign;

fn params(size: f32) -> TextParams<'static> {
    TextParams {
        family: "",
        bold: false,
        size,
        letter_spacing: 0.0,
        line_height: 1.2,
        wrap: true,
        align: TextAlign::Left,
    }
}

#[test]
fn text_wraps_at_spaces() {
    let fonts = Fonts::new();
    let one = fonts.layout("hello world", params(20.0), None);
    let wrapped = fonts.layout("hello world", params(20.0), Some(one.width * 0.7));
    assert!((wrapped.height - one.height * 2.0).abs() < 0.01);
    assert!(wrapped.width < one.width);
    // The second line starts back at the left edge.
    let w = wrapped
        .glyphs
        .iter()
        .position(|g| g.baseline > wrapped.glyphs[0].baseline)
        .unwrap();
    assert!(wrapped.glyphs[w].x < 1.0);
}

#[test]
fn centred_text_centres_in_its_box() {
    let fonts = Fonts::new();
    let mut p = params(20.0);
    p.align = TextAlign::Center;
    let l = fonts.layout("ab", p, Some(200.0));
    let left = l.glyphs[0].x;
    assert!((left - (200.0 - l.width) / 2.0).abs() < 0.5);
}

#[test]
fn glyphs_are_rasterised_once_per_size() {
    let mut fonts = Fonts::new();
    let id = fonts.layout("A", params(20.0), None).glyphs[0].id;
    let a = fonts.glyph(0, id, 20.0).unwrap();
    fonts.atlas.dirty = false;
    let b = fonts.glyph(0, id, 20.0).unwrap();
    assert_eq!(a, b);
    assert!(
        !fonts.atlas.dirty,
        "a cached glyph does not touch the atlas"
    );
    let big = fonts.glyph(0, id, 40.0).unwrap();
    assert!(big.size.1 > a.size.1);
    assert!(fonts.atlas.dirty);
}

#[test]
fn spaces_have_no_ink() {
    let mut fonts = Fonts::new();
    let id = fonts.layout(" ", params(20.0), None).glyphs[0].id;
    assert!(fonts.glyph(0, id, 20.0).is_none());
}
