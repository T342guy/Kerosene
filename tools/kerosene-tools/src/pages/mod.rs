// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! The toolset's own pages: the tabs that are not an embedded tool.
//!
//! Each page draws itself and returns what the person asked for as an
//! [`Action`](crate::toolset::Action), which the toolset carries out after
//! the frame -- so a page never reaches into another page, and the palette,
//! the sidebar and a button on a page all mean the same thing by "build".

pub mod archive;
pub mod assets;
pub mod build;
pub mod home;
pub mod start;
