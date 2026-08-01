use std::collections::HashMap;

#[derive(Debug, Default, Clone)]
pub struct DatParser {
    pub serial_to_name: HashMap<String, String>,
    pub hash_to_name: HashMap<String, String>,
    pub rom_slug_to_name: HashMap<String, String>,
}

impl DatParser {
    pub fn parse(content: &str) -> Self {
        let mut parser = DatParser::default();
        let mut current_game_name: Option<String> = None;

        for line in content.lines() {
            let trimmed = line.trim();

            if trimmed.starts_with("game (") {
                current_game_name = None;
            } else if trimmed.starts_with("name \"") {
                if let Some(first_q) = trimmed.find('"') {
                    if let Some(last_q) = trimmed[first_q + 1..].rfind('"') {
                        let raw_name = &trimmed[first_q + 1..first_q + 1 + last_q];
                        current_game_name = Some(Self::clean_dat_title(raw_name));
                    }
                }
            } else if trimmed.starts_with("serial \"") {
                if let Some(ref game_name) = current_game_name {
                    if let Some(first_q) = trimmed.find('"') {
                        if let Some(last_q) = trimmed[first_q + 1..].rfind('"') {
                            let serial = &trimmed[first_q + 1..first_q + 1 + last_q];
                            let norm_serial = Self::normalize_serial(serial);
                            if !norm_serial.is_empty() {
                                parser.serial_to_name.insert(norm_serial, game_name.clone());
                            }
                        }
                    }
                }
            } else if trimmed.starts_with("rom (") {
                if let Some(ref game_name) = current_game_name {
                    Self::extract_hash(trimmed, "crc", &mut parser.hash_to_name, game_name);
                    Self::extract_hash(trimmed, "md5", &mut parser.hash_to_name, game_name);
                    Self::extract_hash(trimmed, "sha1", &mut parser.hash_to_name, game_name);

                    if let Some(s_pos) = trimmed.find("serial \"") {
                        if let Some(last_q) = trimmed[s_pos + 8..].find('"') {
                            let serial = &trimmed[s_pos + 8..s_pos + 8 + last_q];
                            let norm = Self::normalize_serial(serial);
                            if !norm.is_empty() {
                                parser.serial_to_name.insert(norm, game_name.clone());
                            }
                        }
                    }
                }
            }
        }

        parser
    }

    fn extract_hash(line: &str, hash_type: &str, map: &mut HashMap<String, String>, game_name: &str) {
        let key = format!("{} ", hash_type);
        if let Some(pos) = line.find(&key) {
            let rest = line[pos + key.len()..].trim_start();
            let hash_val: String = rest.chars().take_while(|c| c.is_alphanumeric()).collect();
            if !hash_val.is_empty() {
                map.insert(hash_val.to_lowercase(), game_name.to_string());
            }
        }
    }

    pub fn normalize_serial(serial: &str) -> String {
        serial.chars().filter(|c| c.is_alphanumeric()).collect::<String>().to_lowercase()
    }

    pub fn clean_dat_title(name: &str) -> String {
        let mut clean = name.to_string();

        while let Some(start) = clean.find('(') {
            if let Some(end) = clean[start..].find(')') {
                clean.replace_range(start..=start + end, "");
            } else {
                break;
            }
        }

        while let Some(start) = clean.find('[') {
            if let Some(end) = clean[start..].find(']') {
                clean.replace_range(start..=start + end, "");
            } else {
                break;
            }
        }

        let mut clean_str = clean.trim().to_string();

        for article in ["The", "A", "An", "El", "La", "Los", "Las"] {
            let pattern = format!(", {}", article);
            if let Some(pos) = clean_str.find(&pattern) {
                let prefix = &clean_str[..pos];
                let suffix = &clean_str[pos + pattern.len()..];
                clean_str = format!("{} {}{}", article, prefix, suffix);
                break;
            }
        }

        clean_str.replace(" - ", ": ").trim().to_string()
    }

    pub fn resolve_by_serial(&self, serial: &str) -> Option<&String> {
        let norm = Self::normalize_serial(serial);
        self.serial_to_name.get(&norm)
    }

    pub fn resolve_by_hash(&self, hash: &str) -> Option<&String> {
        let norm = hash.to_lowercase();
        self.hash_to_name.get(&norm)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dat_parser_and_cleaning() {
        let dat = r#"
game (
        name "Legend of Zelda, The - Phantom Hourglass (USA) (En,Fr,Es)"
        serial "AZEE"
        rom ( name "zelda.nds" crc 8B431C41 md5 745E372BDE611C2CEB3EC5DB2BBEB77A serial "AZEE" )
)
        "#;

        let parser = DatParser::parse(dat);
        let title = parser.resolve_by_serial("AZEE");
        assert_eq!(title, Some(&"The Legend of Zelda: Phantom Hourglass".to_string()));
    }
}
