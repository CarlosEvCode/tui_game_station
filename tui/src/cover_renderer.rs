use image::DynamicImage;
use image::ImageReader;
use ratatui_image::picker::Picker;
use ratatui_image::protocol::StatefulProtocol;
use std::path::Path;

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

    pub fn load_protocol_from_file(&mut self, path: &Path) -> Option<StatefulProtocol> {
        if !path.exists() {
            return None;
        }

        let dyn_img: DynamicImage = ImageReader::open(path)
            .ok()?
            .with_guessed_format()
            .ok()?
            .decode()
            .ok()?;
        Some(self.picker.new_resize_protocol(dyn_img))
    }
}
