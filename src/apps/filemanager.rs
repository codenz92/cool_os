use font8x8::UnicodeFonts;

extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;

use crate::fat32::DirEntryInfo;
use crate::framebuffer::{BLACK, WHITE};
use crate::wm::window::{Window, TITLE_H};

include!("filemanager/model.rs");

mod actions;
mod drawing;
mod layout_hit_test;
mod modals;
