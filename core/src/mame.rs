use crate::dat_parser::DatParser;

/// Well-known MAME BIOS/device ROM sets: they are required by games but are
/// never playable themselves, so importing them would fill the library with
/// `neogeo`, `pgm`, `naomi`, ... entries.
///
/// Ported from the legacy Dart launcher
/// (lib/core/injector/dat_resolver.dart `_commonMameBios`).
pub const COMMON_MAME_BIOS: &[&str] = &[
    "neogeo",
    "qsound",
    "qsound_hle",
    "pgm",
    "naomi",
    "naomi2",
    "awbios",
    "cpzn1",
    "cpzn2",
    "decocass",
    "konamih",
    "playch10",
    "skns",
    "stvbios",
    "taito_h",
    "taitofx1",
    "tpgm",
    "triforce",
    "ym2608",
    "bios",
    "hikaru",
    "islands",
    "hng64",
    "maxaflex",
    "megaplay",
    "megatech",
    "nss",
    "nps",
    "namcoc74",
    "namcoc75",
    "namcoc76",
    "suprnova",
    "sys24",
    "sys573",
    "taito_g",
    "taitotz",
    "cchip",
    "k052109",
    "k053246",
    "k053260",
    "k055555",
    "kondev",
    "namcoc148",
    "namcops2",
    "namcopsx",
    "stv",
    "sys141b",
    "sys142b",
    "sys246",
    "sys256",
    "t5182",
    "taito68705",
    "taitosjsecmcu",
    "tourvis",
    "v4bios",
    "ymf281",
    "z8671",
    "z8682",
    "zorba_kbd",
    "zorro_a2091",
    "zorro_a2232",
    "zorro_a590",
    "zorro_ar1",
    "zorro_ar2",
    "zorro_ar3",
    "zorro_buddha",
    "cvs2gd",
];

pub fn is_known_bios_slug(slug: &str) -> bool {
    COMMON_MAME_BIOS
        .iter()
        .any(|bios| bios.eq_ignore_ascii_case(slug))
}

/// Human-readable title for the rare arcade ROMs whose slug never appears in
/// the MAME DAT (only their GD-ROM `cvs2gd` set does).
pub fn special_game_title(slug: &str) -> Option<&'static str> {
    match slug {
        "cvs2" => Some("Capcom Vs. SNK 2 Millionaire Fighting 2001"),
        "cvs" => Some("Capcom Vs. SNK Millenium Fight 2000"),
        _ => None,
    }
}

/// Decide whether a MAME/Arcade ROM slug is a real, playable game.
///
/// - `cvs2`/`cvs` are allowlisted exceptions (their GD-ROM only set is indexed
///   under `cvs2gd` in the DAT).
/// - Known BIOS slugs are never games.
/// - Otherwise a slug is a game iff it appears as a rom slug in the MAME DAT.
/// - Without a DAT we keep the file (fail-open), matching the legacy app.
pub fn is_mame_game(slug: &str, dat_parser: Option<&DatParser>) -> bool {
    let lower = slug.to_lowercase();
    if lower == "cvs2" || lower == "cvs" {
        return true;
    }
    if is_known_bios_slug(&lower) {
        return false;
    }
    match dat_parser {
        Some(parser) => parser.rom_slug_to_name.contains_key(&lower),
        None => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_bios_slugs_are_flagged() {
        for bios in ["neogeo", "pgm", "naomi", "qsound", "triforce", "stvbios"] {
            assert!(is_known_bios_slug(bios), "{bios} is a BIOS");
        }
        assert!(is_known_bios_slug("NeoGeo"), "case-insensitive");
        assert!(!is_known_bios_slug("pacman"));
        assert!(!is_known_bios_slug("1943u"));
    }

    #[test]
    fn bios_slugs_are_rejected_even_without_dat() {
        assert!(!is_mame_game("neogeo", None));
        assert!(!is_mame_game("neogeo", Some(&DatParser::default())));
    }

    #[test]
    fn cvs_exceptions_are_always_games() {
        assert!(is_mame_game("cvs2", None));
        assert!(is_mame_game("cvs", Some(&DatParser::default())));
        assert_eq!(
            special_game_title("cvs2"),
            Some("Capcom Vs. SNK 2 Millionaire Fighting 2001")
        );
    }

    #[test]
    fn dat_membership_decides_games_and_fails_open_without_dat() {
        let mut parser = DatParser::default();
        parser.rom_slug_to_name.insert(
            "1943u".to_string(),
            "1943: The Battle of Midway".to_string(),
        );

        assert!(is_mame_game("1943u", Some(&parser)));
        assert!(!is_mame_game("notaparent", Some(&parser)));
        // No DAT loaded -> keep the file rather than dropping real games.
        assert!(is_mame_game("notaparent", None));
    }
}
