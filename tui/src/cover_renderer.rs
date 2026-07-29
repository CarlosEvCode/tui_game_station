use image::DynamicImage;
use image::ImageReader;
use ratatui_image::picker::Picker;
use ratatui_image::protocol::StatefulProtocol;
use std::path::Path;

#[derive(Clone)]
pub struct CoverManager {
    pub picker: Picker,
}

impl CoverManager {
    pub fn new() -> Self {
        let picker = Picker::from_query_stdio().unwrap_or_else(|_| {
            Picker::from_fontsize((8, 16))
        });

        Self { picker }
    }

    pub fn with_picker(picker: Picker) -> Self {
        Self { picker }
    }

    pub fn load_protocol_from_file(&self, path: &Path) -> Option<StatefulProtocol> {
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
}
