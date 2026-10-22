//! Fallout 1 CE INI-style configuration reader.
//!
//! Source reference: src/game/config.cc; default installation values from
//! src/game/gconfig.cc. This handles read-only key/value parsing, not saving.

use std::collections::BTreeMap;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Config {
    sections: BTreeMap<String, BTreeMap<String, String>>,
}

impl Config {
    pub fn parse(source: &str) -> Self {
        let mut config = Self::default();
        let mut active = String::from("unknown");

        for original in source.lines() {
            // CE truncates on the first ';' even in a value.
            let line = original.split(';').next().unwrap_or("");
            if let Some(start) = line.find('[') {
                if let Some(end) = line[start + 1..].find(']') {
                    active = line[start + 1..start + 1 + end].trim().to_ascii_lowercase();
                    continue;
                }
            }
            if let Some((key, value)) = line.split_once('=') {
                config.set(&active, key.trim(), value.trim());
            }
        }
        config
    }

    pub fn set(&mut self, section: &str, key: &str, value: &str) {
        self.sections
            .entry(section.to_ascii_lowercase())
            .or_default()
            .insert(key.to_ascii_lowercase(), value.to_owned());
    }

    pub fn get(&self, section: &str, key: &str) -> Option<&str> {
        self.sections
            .get(&section.to_ascii_lowercase())?
            .get(&key.to_ascii_lowercase())
            .map(String::as_str)
    }

    pub fn fallout1_defaults() -> Self {
        let mut result = Self::default();
        result.set("system", "master_dat", "master.dat");
        result.set("system", "critter_dat", "critter.dat");
        result.set("system", "master_patches", "data");
        result.set("system", "critter_patches", "data");
        result.set("system", "language", "english");
        result
    }

    pub fn apply(&mut self, values: &Self) {
        for (section, fields) in &values.sections {
            for (key, value) in fields {
                self.set(section, key, value);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Config;

    #[test]
    fn parses_sections_comments_and_duplicates() {
        let config = Config::parse(
            "; Fallout settings\n[System]\nMaster_Dat = master.dat ; trailing\nmaster_dat=MASTER.DAT\n[Sound]\nmusic = 0\n",
        );
        assert_eq!(config.get("SYSTEM", "MASTER_DAT"), Some("MASTER.DAT"));
        assert_eq!(config.get("sound", "music"), Some("0"));
        assert_eq!(config.get("missing", "x"), None);
    }

    #[test]
    fn overrides_ce_defaults() {
        let mut config = Config::fallout1_defaults();
        config.apply(&Config::parse("[system]\ncritter_dat = CRITTER.DAT\n"));
        assert_eq!(config.get("system", "critter_dat"), Some("CRITTER.DAT"));
        assert_eq!(config.get("system", "master_patches"), Some("data"));
        assert_eq!(config.get("system", "language"), Some("english"));
    }
}
