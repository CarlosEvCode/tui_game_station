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
    /// Prepare search query string with ES-DE specific quirks:
    /// - Trim leading/trailing whitespace
    /// - Convert underscores to spaces
    /// - If empty, fallback to "zzzzzz" (avoids ScreenScraper malformed URL errors)
    /// - Strip trailing '+' characters if present
    /// - Handle "THE " prefix and " THE" suffix
    /// - Return (query, is_single_search) where is_single_search is true if length < 4 or arcade
    pub fn prepare_search_query_esde(raw_name: &str, is_arcade: bool) -> (String, bool) {
        let mut clean = Self::clean_title(raw_name);

        if clean.is_empty() {
            clean = "zzzzzz".to_string();
        }

        let mut single_search = is_arcade || clean.len() < 4;

        if !single_search && clean.ends_with('+') {
            let trimmed = clean.trim_end_matches('+');
            if trimmed.len() < 4 {
                single_search = true;
            }
        }

        if !single_search {
            let upper = clean.to_uppercase();
            let mut remove_the = upper.replace("THE ", "");
            remove_the = remove_the.trim_start().to_string();
            if remove_the.len() > 4 && remove_the.ends_with(" THE") {
                remove_the = remove_the[..remove_the.len() - 4].to_string();
            }
            if remove_the.len() < 4 {
                single_search = true;
            }
        }

        (clean, single_search)
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

    #[test]
    fn test_esde_search_query_preparation() {
        let (query, single) = TitleCleaner::prepare_search_query_esde("The_Sims.zip", false);
        assert_eq!(query, "The Sims");
        assert!(!single); // "SIMS" is 4 chars, so not single search

        let (query_short, single_short) =
            TitleCleaner::prepare_search_query_esde("1942.zip", false);
        assert_eq!(query_short, "1942");
        assert!(!single_short);

        let (query_arcade, single_arcade) =
            TitleCleaner::prepare_search_query_esde("sf2.zip", true);
        assert_eq!(query_arcade, "sf 2");
        assert!(single_arcade);

        let (query_empty, single_empty) = TitleCleaner::prepare_search_query_esde("", false);
        assert_eq!(query_empty, "zzzzzz");
        assert!(!single_empty);
    }
}
