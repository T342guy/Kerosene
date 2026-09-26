// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! Computed style: what every property means.
//!
//! [`css`](crate::css) turns text into declarations; this turns declarations
//! into a [`Style`], one per panel, which is everything layout and drawing
//! need to know. Unknown properties and unreadable values are ignored and
//! reported once through [`Style::apply`]'s return value, so a typo shows up in
//! the console rather than silently doing nothing.
//!
//! # Units
//!
//! Lengths are in **UI pixels**: a 1080-pixel-tall reference screen, scaled
//! to the real one. A HUD laid out for 1080p is the same shape at 720p and at
//! 4K -- Panorama's rule, and the only sane one for something that has to sit
//! over a 3D view of any size. `vw`/`vh` are fractions of the screen, and `%`
//! is the parent, as on the web.
//!
//! # Kerosene extensions
//!
//! Two properties a game HUD needs that CSS never grew:
//!
//! * `-kero-fill: radial(0.25)` / `horizontal(0.5)` / `vertical(0.8)` clips a
//!   panel to a fraction of itself -- a cooldown sweep, a health bar -- without
//!   an image per step.
//! * `-kero-blend: additive` adds a panel onto what is behind it, for glows,
//!   hit markers and muzzle-flash overlays.
//!
//! `-kero-tint` multiplies an image by a colour, so one white crosshair can
//! turn red when it is over an enemy.

use crate::css::{self, Decl};

/// A straight-alpha sRGB colour, each channel 0..=1.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Color(pub [f32; 4]);

impl Color {
    pub const TRANSPARENT: Color = Color([0.0; 4]);
    pub const WHITE: Color = Color([1.0; 4]);
    pub const BLACK: Color = Color([0.0, 0.0, 0.0, 1.0]);

    pub fn a(self) -> f32 {
        self.0[3]
    }

    pub fn with_alpha(self, a: f32) -> Color {
        Color([self.0[0], self.0[1], self.0[2], a])
    }

    pub fn is_visible(self) -> bool {
        self.0[3] > 0.0
    }

    pub fn lerp(self, other: Color, t: f32) -> Color {
        let mut out = [0.0; 4];
        for (i, v) in out.iter_mut().enumerate() {
            *v = self.0[i] + (other.0[i] - self.0[i]) * t;
        }
        Color(out)
    }

    pub fn parse(text: &str) -> Option<Color> {
        let t = text.trim().to_ascii_lowercase();
        if let Some(hex) = t.strip_prefix('#') {
            let digit = |i: usize| u8::from_str_radix(hex.get(i..i + 1)?, 16).ok();
            let byte = |i: usize| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok();
            let rgba: [u8; 4] = match hex.len() {
                3 | 4 => {
                    let mut c = [255u8; 4];
                    for (i, v) in c.iter_mut().enumerate().take(hex.len()) {
                        *v = digit(i)? * 17;
                    }
                    c
                }
                6 | 8 => {
                    let mut c = [255u8; 4];
                    for (i, v) in c.iter_mut().enumerate().take(hex.len() / 2) {
                        *v = byte(i * 2)?;
                    }
                    c
                }
                _ => return None,
            };
            return Some(Color(rgba.map(|v| f32::from(v) / 255.0)));
        }
        if let Some(args) = t
            .strip_prefix("rgba(")
            .or_else(|| t.strip_prefix("rgb("))
            .and_then(|a| a.strip_suffix(')'))
        {
            let parts: Vec<&str> = args
                .split(|c: char| c == ',' || c == '/' || c.is_whitespace())
                .filter(|p| !p.is_empty())
                .collect();
            if parts.len() < 3 {
                return None;
            }
            let channel = |p: &str| -> Option<f32> {
                match p.strip_suffix('%') {
                    Some(pc) => pc.parse::<f32>().ok().map(|v| v / 100.0),
                    None => p.parse::<f32>().ok().map(|v| v / 255.0),
                }
            };
            let alpha = match parts.get(3) {
                Some(p) => match p.strip_suffix('%') {
                    Some(pc) => pc.parse::<f32>().ok()? / 100.0,
                    None => p.parse::<f32>().ok()?,
                },
                None => 1.0,
            };
            return Some(Color([
                channel(parts[0])?.clamp(0.0, 1.0),
                channel(parts[1])?.clamp(0.0, 1.0),
                channel(parts[2])?.clamp(0.0, 1.0),
                alpha.clamp(0.0, 1.0),
            ]));
        }
        let named: [u8; 4] = match t.as_str() {
            "transparent" | "none" => return Some(Color::TRANSPARENT),
            "white" => [255, 255, 255, 255],
            "black" => [0, 0, 0, 255],
            "red" => [255, 0, 0, 255],
            "green" => [0, 128, 0, 255],
            "lime" => [0, 255, 0, 255],
            "blue" => [0, 0, 255, 255],
            "yellow" => [255, 255, 0, 255],
            "orange" => [255, 165, 0, 255],
            "cyan" | "aqua" => [0, 255, 255, 255],
            "magenta" | "fuchsia" => [255, 0, 255, 255],
            "gray" | "grey" => [128, 128, 128, 255],
            "silver" => [192, 192, 192, 255],
            _ => return None,
        };
        Some(Color(named.map(|v| f32::from(v) / 255.0)))
    }
}

/// A length before layout resolves it.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub enum Dim {
    #[default]
    Auto,
    /// UI pixels (see the module docs).
    Px(f32),
    /// Of the parent, 0..=1.
    Percent(f32),
}

impl Dim {
    /// Read a length. `vw`/`vh` need the screen, in UI pixels.
    pub fn parse(text: &str, viewport: (f32, f32)) -> Option<Dim> {
        let t = text.trim();
        if t == "auto" {
            return Some(Dim::Auto);
        }
        if let Some(v) = t.strip_suffix('%') {
            return v
                .trim()
                .parse::<f32>()
                .ok()
                .map(|v| Dim::Percent(v / 100.0));
        }
        if let Some(v) = t.strip_suffix("vw") {
            return v
                .trim()
                .parse::<f32>()
                .ok()
                .map(|v| Dim::Px(v / 100.0 * viewport.0));
        }
        if let Some(v) = t.strip_suffix("vh") {
            return v
                .trim()
                .parse::<f32>()
                .ok()
                .map(|v| Dim::Px(v / 100.0 * viewport.1));
        }
        let v = t.strip_suffix("px").unwrap_or(t);
        v.trim().parse::<f32>().ok().map(Dim::Px)
    }

    /// The length in UI pixels, given what a percentage is of.
    pub fn resolve(self, of: f32) -> Option<f32> {
        match self {
            Dim::Auto => None,
            Dim::Px(v) => Some(v),
            Dim::Percent(p) => Some(p * of),
        }
    }

    fn lerp(self, other: Dim, t: f32) -> Dim {
        match (self, other) {
            (Dim::Px(a), Dim::Px(b)) => Dim::Px(a + (b - a) * t),
            (Dim::Percent(a), Dim::Percent(b)) => Dim::Percent(a + (b - a) * t),
            // Different units have nothing sensible in between without
            // layout; snap at the midpoint, as browsers do for `auto`.
            _ if t < 0.5 => self,
            _ => other,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum FlexDirection {
    Row,
    #[default]
    Column,
    RowReverse,
    ColumnReverse,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Align {
    #[default]
    Start,
    Center,
    End,
    Stretch,
    SpaceBetween,
    SpaceAround,
    SpaceEvenly,
}

impl Align {
    fn parse(text: &str) -> Option<Align> {
        Some(match text.trim() {
            "start" | "flex-start" | "left" | "top" => Align::Start,
            "center" | "middle" => Align::Center,
            "end" | "flex-end" | "right" | "bottom" => Align::End,
            "stretch" => Align::Stretch,
            "space-between" => Align::SpaceBetween,
            "space-around" => Align::SpaceAround,
            "space-evenly" => Align::SpaceEvenly,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum TextAlign {
    #[default]
    Left,
    Center,
    Right,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Fit {
    /// Stretch to the box.
    #[default]
    Fill,
    /// Largest that fits, keeping the aspect.
    Contain,
    /// Smallest that covers, keeping the aspect (cropped).
    Cover,
}

/// `-kero-fill`: show only part of a panel.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub enum Fill {
    #[default]
    None,
    /// Clockwise from twelve o'clock.
    Radial(f32),
    /// Left to right.
    Horizontal(f32),
    /// Bottom to top.
    Vertical(f32),
}

impl Fill {
    pub fn parse(text: &str) -> Option<Fill> {
        let t = text.trim();
        if t == "none" {
            return Some(Fill::None);
        }
        let (kind, arg) = t.split_once('(')?;
        let v = arg.strip_suffix(')')?.trim();
        let v = match v.strip_suffix('%') {
            Some(p) => p.trim().parse::<f32>().ok()? / 100.0,
            None => v.parse::<f32>().ok()?,
        }
        .clamp(0.0, 1.0);
        Some(match kind.trim() {
            "radial" => Fill::Radial(v),
            "horizontal" => Fill::Horizontal(v),
            "vertical" => Fill::Vertical(v),
            _ => return None,
        })
    }

    /// `(kind, amount)` as the shader reads it: 0 none, 1 radial, 2
    /// horizontal, 3 vertical.
    pub fn encode(self) -> (u32, f32) {
        match self {
            Fill::None => (0, 1.0),
            Fill::Radial(v) => (1, v),
            Fill::Horizontal(v) => (2, v),
            Fill::Vertical(v) => (3, v),
        }
    }

    fn lerp(self, other: Fill, t: f32) -> Fill {
        let mix = |a: f32, b: f32| a + (b - a) * t;
        match (self, other) {
            (Fill::Radial(a), Fill::Radial(b)) => Fill::Radial(mix(a, b)),
            (Fill::Horizontal(a), Fill::Horizontal(b)) => Fill::Horizontal(mix(a, b)),
            (Fill::Vertical(a), Fill::Vertical(b)) => Fill::Vertical(mix(a, b)),
            _ if t < 0.5 => self,
            _ => other,
        }
    }
}

/// A 2D transform, applied about the centre of the panel after layout.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Transform {
    pub translate: (Dim, Dim),
    pub scale: (f32, f32),
    /// Degrees, clockwise.
    pub rotate: f32,
}

impl Default for Transform {
    fn default() -> Self {
        Transform {
            translate: (Dim::Px(0.0), Dim::Px(0.0)),
            scale: (1.0, 1.0),
            rotate: 0.0,
        }
    }
}

impl Transform {
    pub fn is_identity(&self) -> bool {
        *self == Transform::default()
    }

    fn parse(text: &str, viewport: (f32, f32)) -> Option<Transform> {
        let mut out = Transform::default();
        if text.trim() == "none" {
            return Some(out);
        }
        let mut rest = text.trim();
        while !rest.is_empty() {
            let open = rest.find('(')?;
            let close = rest.find(')')?;
            let name = rest[..open].trim();
            let args: Vec<&str> = rest[open + 1..close]
                .split(',')
                .map(str::trim)
                .filter(|a| !a.is_empty())
                .collect();
            let num = |i: usize| {
                args.get(i)
                    .and_then(|a| a.trim_end_matches("deg").trim().parse::<f32>().ok())
            };
            match name {
                "translate" => {
                    let x = Dim::parse(args.first()?, viewport)?;
                    let y = match args.get(1) {
                        Some(a) => Dim::parse(a, viewport)?,
                        None => Dim::Px(0.0),
                    };
                    out.translate = (x, y);
                }
                "translateX" | "translatex" => {
                    out.translate.0 = Dim::parse(args.first()?, viewport)?
                }
                "translateY" | "translatey" => {
                    out.translate.1 = Dim::parse(args.first()?, viewport)?
                }
                "scale" => {
                    let x = num(0)?;
                    out.scale = (x, num(1).unwrap_or(x));
                }
                "scaleX" | "scalex" => out.scale.0 = num(0)?,
                "scaleY" | "scaley" => out.scale.1 = num(0)?,
                "rotate" | "rotateZ" | "rotatez" => out.rotate = num(0)?,
                _ => return None,
            }
            rest = rest[close + 1..].trim_start();
        }
        Some(out)
    }

    fn lerp(self, other: Transform, t: f32) -> Transform {
        let mix = |a: f32, b: f32| a + (b - a) * t;
        Transform {
            translate: (
                self.translate.0.lerp(other.translate.0, t),
                self.translate.1.lerp(other.translate.1, t),
            ),
            scale: (
                mix(self.scale.0, other.scale.0),
                mix(self.scale.1, other.scale.1),
            ),
            rotate: mix(self.rotate, other.rotate),
        }
    }
}

/// An easing curve.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub enum Timing {
    Linear,
    #[default]
    Ease,
    EaseIn,
    EaseOut,
    EaseInOut,
    /// Jump to the end at the end: what a `steps(1)` blink wants.
    Step,
}

impl Timing {
    fn parse(text: &str) -> Option<Timing> {
        Some(match text {
            "linear" => Timing::Linear,
            "ease" => Timing::Ease,
            "ease-in" => Timing::EaseIn,
            "ease-out" => Timing::EaseOut,
            "ease-in-out" => Timing::EaseInOut,
            "step-end" | "steps(1)" => Timing::Step,
            _ => return None,
        })
    }

    pub fn apply(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Timing::Linear => t,
            // Close to the cubic-beziers CSS names, without solving one.
            Timing::Ease => {
                let s = t * t * (3.0 - 2.0 * t);
                s * 0.8 + (1.0 - (1.0 - t) * (1.0 - t)) * 0.2
            }
            Timing::EaseIn => t * t * t,
            Timing::EaseOut => 1.0 - (1.0 - t).powi(3),
            Timing::EaseInOut => t * t * (3.0 - 2.0 * t),
            Timing::Step => {
                if t >= 1.0 {
                    1.0
                } else {
                    0.0
                }
            }
        }
    }
}

/// Properties that can be animated. Anything else changes instantly.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AnimProp {
    Opacity,
    BackgroundColor,
    Color,
    BorderColor,
    Tint,
    Transform,
    Fill,
    Width,
    Height,
    Left,
    Top,
    Right,
    Bottom,
}

impl AnimProp {
    pub const ALL: [AnimProp; 13] = [
        AnimProp::Opacity,
        AnimProp::BackgroundColor,
        AnimProp::Color,
        AnimProp::BorderColor,
        AnimProp::Tint,
        AnimProp::Transform,
        AnimProp::Fill,
        AnimProp::Width,
        AnimProp::Height,
        AnimProp::Left,
        AnimProp::Top,
        AnimProp::Right,
        AnimProp::Bottom,
    ];

    pub fn parse(name: &str) -> Option<AnimProp> {
        Some(match name {
            "opacity" => AnimProp::Opacity,
            "background-color" => AnimProp::BackgroundColor,
            "color" => AnimProp::Color,
            "border-color" => AnimProp::BorderColor,
            "-kero-tint" => AnimProp::Tint,
            "transform" => AnimProp::Transform,
            "-kero-fill" => AnimProp::Fill,
            "width" => AnimProp::Width,
            "height" => AnimProp::Height,
            "left" => AnimProp::Left,
            "top" => AnimProp::Top,
            "right" => AnimProp::Right,
            "bottom" => AnimProp::Bottom,
            _ => return None,
        })
    }

    /// Whether a change to it means laying out again.
    pub fn affects_layout(self) -> bool {
        matches!(
            self,
            AnimProp::Width
                | AnimProp::Height
                | AnimProp::Left
                | AnimProp::Top
                | AnimProp::Right
                | AnimProp::Bottom
        )
    }

    /// Copy this one property, blended, from `from`/`to` into `out`.
    pub fn blend(self, out: &mut Style, from: &Style, to: &Style, t: f32) {
        match self {
            AnimProp::Opacity => out.opacity = from.opacity + (to.opacity - from.opacity) * t,
            AnimProp::BackgroundColor => {
                out.background_color = from.background_color.lerp(to.background_color, t)
            }
            AnimProp::Color => out.color = from.color.lerp(to.color, t),
            AnimProp::BorderColor => out.border_color = from.border_color.lerp(to.border_color, t),
            AnimProp::Tint => out.tint = from.tint.lerp(to.tint, t),
            AnimProp::Transform => out.transform = from.transform.lerp(to.transform, t),
            AnimProp::Fill => out.fill = from.fill.lerp(to.fill, t),
            AnimProp::Width => out.width = from.width.lerp(to.width, t),
            AnimProp::Height => out.height = from.height.lerp(to.height, t),
            AnimProp::Left => out.inset[0] = from.inset[0].lerp(to.inset[0], t),
            AnimProp::Top => out.inset[1] = from.inset[1].lerp(to.inset[1], t),
            AnimProp::Right => out.inset[2] = from.inset[2].lerp(to.inset[2], t),
            AnimProp::Bottom => out.inset[3] = from.inset[3].lerp(to.inset[3], t),
        }
    }

    /// Whether the property differs between two styles.
    pub fn differs(self, a: &Style, b: &Style) -> bool {
        match self {
            AnimProp::Opacity => a.opacity != b.opacity,
            AnimProp::BackgroundColor => a.background_color != b.background_color,
            AnimProp::Color => a.color != b.color,
            AnimProp::BorderColor => a.border_color != b.border_color,
            AnimProp::Tint => a.tint != b.tint,
            AnimProp::Transform => a.transform != b.transform,
            AnimProp::Fill => a.fill != b.fill,
            AnimProp::Width => a.width != b.width,
            AnimProp::Height => a.height != b.height,
            AnimProp::Left => a.inset[0] != b.inset[0],
            AnimProp::Top => a.inset[1] != b.inset[1],
            AnimProp::Right => a.inset[2] != b.inset[2],
            AnimProp::Bottom => a.inset[3] != b.inset[3],
        }
    }
}

/// `transition: opacity 0.2s ease-out 0s`.
#[derive(Clone, PartialEq, Debug)]
pub struct TransitionSpec {
    /// `None` is `all`.
    pub prop: Option<AnimProp>,
    pub duration: f32,
    pub timing: Timing,
    pub delay: f32,
}

/// `animation: pulse 1s ease-in-out infinite alternate`.
#[derive(Clone, PartialEq, Debug)]
pub struct AnimationSpec {
    pub name: String,
    pub duration: f32,
    pub timing: Timing,
    pub delay: f32,
    /// `None` is `infinite`.
    pub iterations: Option<f32>,
    pub alternate: bool,
}

fn parse_time(text: &str) -> Option<f32> {
    let t = text.trim();
    if let Some(ms) = t.strip_suffix("ms") {
        return ms.parse::<f32>().ok().map(|v| v / 1000.0);
    }
    t.strip_suffix('s')?.parse::<f32>().ok()
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Position {
    #[default]
    Relative,
    Absolute,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Blend {
    #[default]
    Normal,
    Additive,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct TextShadow {
    pub offset: (f32, f32),
    pub color: Color,
}

/// Everything about how one panel looks and sits.
#[derive(Clone, PartialEq, Debug)]
pub struct Style {
    pub display: bool,
    pub visible: bool,
    pub position: Position,
    pub flex_direction: FlexDirection,
    pub flex_wrap: bool,
    pub justify_content: Align,
    pub align_items: Align,
    pub align_self: Option<Align>,
    pub flex_grow: f32,
    pub flex_shrink: f32,
    pub flex_basis: Dim,
    pub width: Dim,
    pub height: Dim,
    pub min_width: Dim,
    pub min_height: Dim,
    pub max_width: Dim,
    pub max_height: Dim,
    /// Left, top, right, bottom -- the order CSS writes them in is
    /// top-right-bottom-left, and [`apply`](Style::apply) converts.
    pub margin: [Dim; 4],
    pub padding: [Dim; 4],
    pub inset: [Dim; 4],
    pub gap: (Dim, Dim),
    pub overflow_hidden: bool,
    pub z_index: i32,
    pub pointer_events: bool,

    pub background_color: Color,
    /// A second colour and whether the gradient runs top-to-bottom (else
    /// left-to-right).
    pub gradient: Option<(Color, bool)>,
    pub background_image: Option<String>,
    pub background_fit: Fit,
    pub tint: Color,
    pub border_width: f32,
    pub border_color: Color,
    pub border_radius: f32,
    pub opacity: f32,
    pub fill: Fill,
    pub blend: Blend,
    pub transform: Transform,
    pub box_shadow: Option<(f32, Color)>,

    // Inherited.
    pub color: Color,
    pub font_family: String,
    pub font_size: f32,
    pub bold: bool,
    pub text_align: TextAlign,
    pub vertical_align: Align,
    pub text_shadow: Option<TextShadow>,
    pub letter_spacing: f32,
    pub line_height: f32,
    pub uppercase: bool,
    pub wrap: bool,

    pub transitions: Vec<TransitionSpec>,
    pub animation: Option<AnimationSpec>,
}

impl Default for Style {
    fn default() -> Self {
        Style {
            display: true,
            visible: true,
            position: Position::Relative,
            flex_direction: FlexDirection::Column,
            flex_wrap: false,
            justify_content: Align::Start,
            align_items: Align::Stretch,
            align_self: None,
            flex_grow: 0.0,
            flex_shrink: 1.0,
            flex_basis: Dim::Auto,
            width: Dim::Auto,
            height: Dim::Auto,
            min_width: Dim::Auto,
            min_height: Dim::Auto,
            max_width: Dim::Auto,
            max_height: Dim::Auto,
            margin: [Dim::Px(0.0); 4],
            padding: [Dim::Px(0.0); 4],
            inset: [Dim::Auto; 4],
            gap: (Dim::Px(0.0), Dim::Px(0.0)),
            overflow_hidden: false,
            z_index: 0,
            pointer_events: true,
            background_color: Color::TRANSPARENT,
            gradient: None,
            background_image: None,
            background_fit: Fit::Fill,
            tint: Color::WHITE,
            border_width: 0.0,
            border_color: Color::TRANSPARENT,
            border_radius: 0.0,
            opacity: 1.0,
            fill: Fill::None,
            blend: Blend::Normal,
            transform: Transform::default(),
            box_shadow: None,
            color: Color::WHITE,
            font_family: String::new(),
            font_size: 24.0,
            bold: false,
            text_align: TextAlign::Left,
            vertical_align: Align::Start,
            text_shadow: None,
            letter_spacing: 0.0,
            line_height: 1.2,
            uppercase: false,
            wrap: true,
            transitions: Vec::new(),
            animation: None,
        }
    }
}

impl Style {
    /// A fresh style for a child: inherited properties from the parent,
    /// everything else at its initial value.
    pub fn inherit(parent: &Style) -> Style {
        Style {
            color: parent.color,
            font_family: parent.font_family.clone(),
            font_size: parent.font_size,
            bold: parent.bold,
            text_align: parent.text_align,
            vertical_align: parent.vertical_align,
            text_shadow: parent.text_shadow,
            letter_spacing: parent.letter_spacing,
            line_height: parent.line_height,
            uppercase: parent.uppercase,
            wrap: parent.wrap,
            visible: parent.visible,
            ..Style::default()
        }
    }

    /// Apply a list of declarations, collecting what could not be read.
    pub fn apply_all(&mut self, decls: &[Decl], viewport: (f32, f32), errors: &mut Vec<String>) {
        for d in decls {
            if !self.apply(&d.name, &d.value, viewport) {
                errors.push(format!("{}: {}", d.name, d.value));
            }
        }
    }

    /// Apply one declaration. `false` if the name or the value is not
    /// understood.
    pub fn apply(&mut self, name: &str, value: &str, viewport: (f32, f32)) -> bool {
        let v = value.trim();
        let dim = |t: &str| Dim::parse(t, viewport);
        let num = |t: &str| t.trim().parse::<f32>().ok();
        let px = |t: &str| match Dim::parse(t, viewport) {
            Some(Dim::Px(v)) => Some(v),
            _ => None,
        };
        macro_rules! set {
            ($field:expr, $parsed:expr) => {{
                match $parsed {
                    Some(value) => {
                        $field = value;
                        true
                    }
                    None => false,
                }
            }};
        }
        match name {
            "display" => set!(
                self.display,
                match v {
                    "none" => Some(false),
                    "flex" | "block" | "inline" | "inline-block" | "inline-flex" => Some(true),
                    _ => None,
                }
            ),
            "visibility" => set!(
                self.visible,
                match v {
                    "visible" => Some(true),
                    "hidden" | "collapse" => Some(false),
                    _ => None,
                }
            ),
            "position" => set!(
                self.position,
                match v {
                    "absolute" | "fixed" => Some(Position::Absolute),
                    "relative" | "static" => Some(Position::Relative),
                    _ => None,
                }
            ),
            "flex-direction" => set!(
                self.flex_direction,
                match v {
                    "row" => Some(FlexDirection::Row),
                    "column" => Some(FlexDirection::Column),
                    "row-reverse" => Some(FlexDirection::RowReverse),
                    "column-reverse" => Some(FlexDirection::ColumnReverse),
                    _ => None,
                }
            ),
            // Panorama's spelling, for anyone coming from there.
            "flow-children" => set!(
                self.flex_direction,
                match v {
                    "right" => Some(FlexDirection::Row),
                    "down" => Some(FlexDirection::Column),
                    "left" => Some(FlexDirection::RowReverse),
                    "up" => Some(FlexDirection::ColumnReverse),
                    "none" => Some(self.flex_direction),
                    _ => None,
                }
            ),
            "flex-wrap" => set!(
                self.flex_wrap,
                match v {
                    "wrap" => Some(true),
                    "nowrap" => Some(false),
                    _ => None,
                }
            ),
            "justify-content" => set!(self.justify_content, Align::parse(v)),
            "align-items" => set!(self.align_items, Align::parse(v)),
            "align-self" => set!(
                self.align_self,
                if v == "auto" {
                    Some(None)
                } else {
                    Align::parse(v).map(Some)
                }
            ),
            "flex-grow" => set!(self.flex_grow, num(v)),
            "flex-shrink" => set!(self.flex_shrink, num(v)),
            "flex-basis" => set!(self.flex_basis, dim(v)),
            "flex" => {
                let parts: Vec<&str> = v.split_whitespace().collect();
                match parts.as_slice() {
                    ["none"] => {
                        self.flex_grow = 0.0;
                        self.flex_shrink = 0.0;
                        true
                    }
                    [g] => {
                        let ok = set!(self.flex_grow, num(g));
                        if ok {
                            self.flex_basis = Dim::Px(0.0);
                        }
                        ok
                    }
                    [g, s] => set!(self.flex_grow, num(g)) && set!(self.flex_shrink, num(s)),
                    [g, s, b] => {
                        set!(self.flex_grow, num(g))
                            && set!(self.flex_shrink, num(s))
                            && set!(self.flex_basis, dim(b))
                    }
                    _ => false,
                }
            }
            "width" => set!(self.width, dim(v)),
            "height" => set!(self.height, dim(v)),
            "min-width" => set!(self.min_width, dim(v)),
            "min-height" => set!(self.min_height, dim(v)),
            "max-width" => set!(self.max_width, dim(v)),
            "max-height" => set!(self.max_height, dim(v)),
            "margin" => set!(self.margin, four(v, viewport)),
            "padding" => set!(self.padding, four(v, viewport)),
            "inset" => set!(self.inset, four(v, viewport)),
            "margin-left" => set!(self.margin[0], dim(v)),
            "margin-top" => set!(self.margin[1], dim(v)),
            "margin-right" => set!(self.margin[2], dim(v)),
            "margin-bottom" => set!(self.margin[3], dim(v)),
            "padding-left" => set!(self.padding[0], dim(v)),
            "padding-top" => set!(self.padding[1], dim(v)),
            "padding-right" => set!(self.padding[2], dim(v)),
            "padding-bottom" => set!(self.padding[3], dim(v)),
            "left" | "x" => set!(self.inset[0], dim(v)),
            "top" | "y" => set!(self.inset[1], dim(v)),
            "right" => set!(self.inset[2], dim(v)),
            "bottom" => set!(self.inset[3], dim(v)),
            "gap" => {
                let parts: Vec<&str> = v.split_whitespace().collect();
                match parts.as_slice() {
                    [a] => dim(a).map(|d| self.gap = (d, d)).is_some(),
                    [r, c] => match (dim(r), dim(c)) {
                        // CSS writes row-gap first; the tuple is (x, y).
                        (Some(r), Some(c)) => {
                            self.gap = (c, r);
                            true
                        }
                        _ => false,
                    },
                    _ => false,
                }
            }
            "row-gap" => set!(self.gap.1, dim(v)),
            "column-gap" => set!(self.gap.0, dim(v)),
            "overflow" => set!(
                self.overflow_hidden,
                match v {
                    "hidden" | "clip" | "scroll" | "auto" => Some(true),
                    "visible" => Some(false),
                    _ => None,
                }
            ),
            "z-index" => set!(self.z_index, v.parse::<i32>().ok()),
            "pointer-events" => set!(
                self.pointer_events,
                match v {
                    "none" => Some(false),
                    "auto" | "all" => Some(true),
                    _ => None,
                }
            ),
            "background-color" => set!(self.background_color, Color::parse(v)),
            "background" => self.apply_background(v),
            "background-image" => self.apply_background(v),
            "background-size" | "-kero-fit" | "object-fit" => set!(
                self.background_fit,
                match v {
                    "contain" => Some(Fit::Contain),
                    "cover" => Some(Fit::Cover),
                    "fill" | "100% 100%" | "stretch" => Some(Fit::Fill),
                    _ => None,
                }
            ),
            "-kero-tint" | "wash-color" => set!(self.tint, Color::parse(v)),
            "border" => {
                let mut ok = true;
                for part in css::split_top_level(v, ' ')
                    .into_iter()
                    .filter(|p| !p.is_empty())
                {
                    if let Some(w) = px(part) {
                        self.border_width = w;
                    } else if let Some(c) = Color::parse(part) {
                        self.border_color = c;
                    } else if !matches!(part, "solid" | "none") {
                        ok = false;
                    }
                    if part == "none" {
                        self.border_width = 0.0;
                    }
                }
                ok
            }
            "border-width" => set!(self.border_width, px(v)),
            "border-color" => set!(self.border_color, Color::parse(v)),
            "border-style" => true,
            "border-radius" => {
                if let Some(p) = v.strip_suffix('%') {
                    // A 50% radius is a circle, which is what it is always
                    // used for; resolved against the box when drawn.
                    set!(
                        self.border_radius,
                        p.trim().parse::<f32>().ok().map(|p| -p / 100.0)
                    )
                } else {
                    set!(self.border_radius, v.split_whitespace().next().and_then(px))
                }
            }
            "opacity" => set!(self.opacity, num(v).map(|o| o.clamp(0.0, 1.0))),
            "-kero-fill" => set!(self.fill, Fill::parse(v)),
            "-kero-blend" | "mix-blend-mode" => set!(
                self.blend,
                match v {
                    "additive" | "plus-lighter" | "screen" => Some(Blend::Additive),
                    "normal" => Some(Blend::Normal),
                    _ => None,
                }
            ),
            "transform" => set!(self.transform, Transform::parse(v, viewport)),
            "box-shadow" => {
                if v == "none" {
                    self.box_shadow = None;
                    return true;
                }
                let parts = css::split_top_level(v, ' ');
                let mut blur = None;
                let mut color = None;
                let mut lengths = Vec::new();
                for p in parts.iter().filter(|p| !p.is_empty()) {
                    if let Some(c) = Color::parse(p) {
                        color = Some(c);
                    } else if let Some(l) = px(p) {
                        lengths.push(l);
                    }
                }
                // Offsets are not drawn: a shadow here is a soft glow
                // around the box, which is what HUDs use one for.
                if let Some(b) = lengths.get(2).or(lengths.last()) {
                    blur = Some(*b);
                }
                match (blur, color) {
                    (Some(b), Some(c)) => {
                        self.box_shadow = Some((b, c));
                        true
                    }
                    _ => false,
                }
            }
            "color" => set!(self.color, Color::parse(v)),
            "font-family" => {
                self.font_family = css::unquote(v.split(',').next().unwrap_or(v)).to_string();
                true
            }
            "font-size" => set!(self.font_size, px(v)),
            "font-weight" => {
                self.bold = css::is_bold(v);
                true
            }
            "font" => {
                let mut ok = false;
                for part in v.split_whitespace() {
                    if let Some(s) = px(part) {
                        self.font_size = s;
                        ok = true;
                    } else if css::is_bold(part) {
                        self.bold = true;
                    } else {
                        self.font_family = css::unquote(part).to_string();
                    }
                }
                ok
            }
            "text-align" | "horizontal-align" => set!(
                self.text_align,
                match v {
                    "left" | "start" => Some(TextAlign::Left),
                    "center" | "middle" => Some(TextAlign::Center),
                    "right" | "end" => Some(TextAlign::Right),
                    _ => None,
                }
            ),
            "vertical-align" => set!(self.vertical_align, Align::parse(v)),
            "text-shadow" => {
                if v == "none" {
                    self.text_shadow = None;
                    return true;
                }
                let mut color = Color::BLACK.with_alpha(0.8);
                let mut lengths = Vec::new();
                for p in css::split_top_level(v, ' ')
                    .iter()
                    .filter(|p| !p.is_empty())
                {
                    if let Some(c) = Color::parse(p) {
                        color = c;
                    } else if let Some(l) = px(p) {
                        lengths.push(l);
                    }
                }
                if lengths.len() < 2 {
                    return false;
                }
                self.text_shadow = Some(TextShadow {
                    offset: (lengths[0], lengths[1]),
                    color,
                });
                true
            }
            "letter-spacing" => set!(self.letter_spacing, px(v)),
            "line-height" => set!(
                self.line_height,
                num(v).or_else(|| px(v).map(|l| l / self.font_size.max(1.0)))
            ),
            "text-transform" => set!(
                self.uppercase,
                match v {
                    "uppercase" => Some(true),
                    "none" => Some(false),
                    _ => None,
                }
            ),
            "white-space" => set!(
                self.wrap,
                match v {
                    "nowrap" | "pre" => Some(false),
                    "normal" | "pre-wrap" => Some(true),
                    _ => None,
                }
            ),
            "transition" => {
                let mut specs = Vec::new();
                for part in css::split_top_level(v, ',') {
                    let mut spec = TransitionSpec {
                        prop: None,
                        duration: 0.0,
                        timing: Timing::Ease,
                        delay: 0.0,
                    };
                    let mut times = 0;
                    for word in part.split_whitespace() {
                        if let Some(t) = parse_time(word) {
                            if times == 0 {
                                spec.duration = t;
                            } else {
                                spec.delay = t;
                            }
                            times += 1;
                        } else if let Some(t) = Timing::parse(word) {
                            spec.timing = t;
                        } else if word == "all" {
                            spec.prop = None;
                        } else if let Some(p) = AnimProp::parse(word) {
                            spec.prop = Some(p);
                        } else if word == "none" {
                            self.transitions.clear();
                            return true;
                        } else {
                            return false;
                        }
                    }
                    specs.push(spec);
                }
                self.transitions = specs;
                true
            }
            "transition-duration" => match parse_time(v) {
                Some(t) => {
                    if self.transitions.is_empty() {
                        self.transitions.push(TransitionSpec {
                            prop: None,
                            duration: t,
                            timing: Timing::Ease,
                            delay: 0.0,
                        });
                    } else {
                        for s in &mut self.transitions {
                            s.duration = t;
                        }
                    }
                    true
                }
                None => false,
            },
            "animation" => {
                if v == "none" {
                    self.animation = None;
                    return true;
                }
                let mut spec = AnimationSpec {
                    name: String::new(),
                    duration: 0.0,
                    timing: Timing::Ease,
                    delay: 0.0,
                    iterations: Some(1.0),
                    alternate: false,
                };
                let mut times = 0;
                for word in v.split_whitespace() {
                    if let Some(t) = parse_time(word) {
                        if times == 0 {
                            spec.duration = t;
                        } else {
                            spec.delay = t;
                        }
                        times += 1;
                    } else if let Some(t) = Timing::parse(word) {
                        spec.timing = t;
                    } else if word == "infinite" {
                        spec.iterations = None;
                    } else if word == "alternate" {
                        spec.alternate = true;
                    } else if matches!(word, "normal" | "forwards" | "both" | "none" | "backwards")
                    {
                    } else if let Ok(n) = word.parse::<f32>() {
                        spec.iterations = Some(n);
                    } else {
                        spec.name = word.to_string();
                    }
                }
                if spec.name.is_empty() {
                    return false;
                }
                self.animation = Some(spec);
                true
            }
            _ => false,
        }
    }

    fn apply_background(&mut self, v: &str) -> bool {
        if v == "none" {
            self.background_image = None;
            self.gradient = None;
            self.background_color = Color::TRANSPARENT;
            return true;
        }
        if let Some(args) = v
            .strip_prefix("linear-gradient(")
            .and_then(|a| a.strip_suffix(')'))
        {
            let parts: Vec<&str> = css::split_top_level(args, ',')
                .into_iter()
                .map(str::trim)
                .collect();
            let (vertical, colors) = match parts.first() {
                Some(&"to right") => (false, &parts[1..]),
                Some(&"to bottom") => (true, &parts[1..]),
                Some(&"to left") => {
                    // Reversed so it reads as the same two colours swapped.
                    let (Some(a), Some(b)) = (
                        parts.get(1).and_then(|c| Color::parse(c)),
                        parts.get(2).and_then(|c| Color::parse(c)),
                    ) else {
                        return false;
                    };
                    self.background_color = b;
                    self.gradient = Some((a, false));
                    return true;
                }
                Some(&"to top") => {
                    let (Some(a), Some(b)) = (
                        parts.get(1).and_then(|c| Color::parse(c)),
                        parts.get(2).and_then(|c| Color::parse(c)),
                    ) else {
                        return false;
                    };
                    self.background_color = b;
                    self.gradient = Some((a, true));
                    return true;
                }
                _ => (true, &parts[..]),
            };
            let (Some(a), Some(b)) = (
                colors.first().and_then(|c| Color::parse(c)),
                colors.last().and_then(|c| Color::parse(c)),
            ) else {
                return false;
            };
            self.background_color = a;
            self.gradient = Some((b, vertical));
            return true;
        }
        let mut ok = false;
        for part in css::split_top_level(v, ' ')
            .into_iter()
            .filter(|p| !p.is_empty())
        {
            if part.starts_with("url(") {
                self.background_image = Some(css::url_or_text(part).to_string());
                ok = true;
            } else if let Some(c) = Color::parse(part) {
                self.background_color = c;
                ok = true;
            }
        }
        ok
    }
}

/// `a`, `a b`, `a b c` or `a b c d`, CSS order, into left-top-right-bottom.
fn four(text: &str, viewport: (f32, f32)) -> Option<[Dim; 4]> {
    let parts: Vec<Dim> = text
        .split_whitespace()
        .map(|p| Dim::parse(p, viewport))
        .collect::<Option<_>>()?;
    let (t, r, b, l) = match parts.as_slice() {
        [a] => (*a, *a, *a, *a),
        [v, h] => (*v, *h, *v, *h),
        [t, h, b] => (*t, *h, *b, *h),
        [t, r, b, l] => (*t, *r, *b, *l),
        _ => return None,
    };
    Some([l, t, r, b])
}

#[cfg(test)]
mod tests;
