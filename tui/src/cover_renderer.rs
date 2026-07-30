use image::DynamicImage;
use image::ImageReader;
use ratatui_image::picker::Picker;
use ratatui_image::protocol::StatefulProtocol;
use std::path::Path;

#[derive(Clone)]
pub struct CoverManager {
    pub picker: Picker,
    pub halfblocks_picker: Picker,
}

impl CoverManager {
    pub fn new() -> Self {
        let picker = Picker::from_query_stdio().unwrap_or_else(|_| {
            Picker::from_fontsize((8, 16))
        });
        let halfblocks_picker = Picker::from_fontsize((8, 16));

        Self { picker, halfblocks_picker }
    }

    pub fn load_native_protocol_from_file(&self, path: &Path) -> Option<StatefulProtocol> {
        if !path.exists() {
            return None;
        }

        let dyn_img: DynamicImage = ImageReader::open(path)
            .ok()?
            .with_guessed_format()
            .ok()?
            .decode()
            .ok()?;
        let mut picker = self.picker.clone();
        Some(picker.new_resize_protocol(dyn_img))
    }

    pub fn load_halfblocks_protocol_from_file(&self, path: &Path) -> Option<StatefulProtocol> {
        if !path.exists() {
            return None;
        }

        let dyn_img: DynamicImage = ImageReader::open(path)
            .ok()?
            .with_guessed_format()
            .ok()?
            .decode()
            .ok()?;
        let mut picker = self.halfblocks_picker.clone();
        Some(picker.new_resize_protocol(dyn_img))
    }

    pub fn load_protocol_from_file(&self, path: &Path) -> Option<StatefulProtocol> {
        self.load_native_protocol_from_file(path)
    }
}
