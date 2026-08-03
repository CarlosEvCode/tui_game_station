use regex::Regex;

pub struct TitleCleaner;

impl TitleCleaner {
    /// Clean raw game title or filename for optimal search querying on SteamGridDB
    pub fn clean_title(raw_name: &str) -> String {
        let mut clean = raw_name.to_string();

        // 1. Remove file extension if present (e.g. .3ds, .iso, .z64, .zip)
        if let Some(pos) = clean.rfind('.') {
            if pos > 0 && clean.len() - pos <= 5 {
                clean = clean[..pos].to_string();
            }
        }

        // 2. Replace underscores and dots with spaces
        clean = clean.replace(['_', '.'], " ");

        // 3. Separate camelCase (e.g. "BloodyRoar" -> "Bloody Roar")
        if let Ok(re_camel) = Regex::new(r"([a-z])([A-Z])") {
            clean = re_camel.replace_all(&clean, "$1 $2").to_string();
        }

        // 4. Separate numbers attached to words (e.g. "kof2002" -> "kof 2002")
        if let Ok(re_num) = Regex::new(r"([a-zA-Z])(\d+)") {
            clean = re_num.replace_all(&clean, "$1 $2").to_string();
        }

        // 5. Remove content in parentheses e.g. (USA), (Europe), (v1.0), (En,Es)
        if let Ok(re_paren) = Regex::new(r"\(.*?\)") {
            clean = re_paren.replace_all(&clean, "").to_string();
        }

        // 6. Remove content in square brackets e.g. [!], [b1], [USA]
        if let Ok(re_bracket) = Regex::new(r"\[.*?\]") {
            clean = re_bracket.replace_all(&clean, "").to_string();
        }

        // 7. Collapse multiple whitespace and trim
        if let Ok(re_space) = Regex::new(r"\s+") {
            clean = re_space.replace_all(&clean, " ").to_string();
        }

        clean.trim().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_title_cleaning() {
        assert_eq!(
            TitleCleaner::clean_title("The_Legend_of_Zelda_-_Ocarina_of_Time_(USA)_(v1.1).z64"),
            "The Legend of Zelda - Ocarina of Time"
        );
        assert_eq!(
            TitleCleaner::clean_title("SuperMario64_[USA]_[!].n64"),
            "Super Mario 64"
        );
    }
}
