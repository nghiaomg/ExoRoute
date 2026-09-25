#![allow(dead_code)]

pub const OPENAI_CODEX: &str = "openai_codex";
pub const COMMAND_CODE: &str = "command_code";
pub const CLINE: &str = "cline";
pub const KILOCODE: &str = "kilocode";

pub const KNOWN_PANELS: [&str; 4] = [OPENAI_CODEX, COMMAND_CODE, CLINE, KILOCODE];

/// Single capability contract shared by build-time validation in build.rs,
/// backend capability checks in provider_adapters, and the dashboard
/// auth-panel registry in web/src/features/providers/authPanelRegistry.ts.
pub fn is_known(panel: &str) -> bool {
    KNOWN_PANELS.contains(&panel)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_panels_are_unique_and_nonempty() {
        let mut seen = std::collections::HashSet::new();
        for panel in KNOWN_PANELS {
            assert!(!panel.is_empty());
            assert!(seen.insert(panel), "duplicate auth panel");
        }
        assert!(is_known(OPENAI_CODEX));
        assert!(is_known(KILOCODE));
        assert!(!is_known("unknown_panel"));
        assert!(!is_known("toString"));
    }
}
