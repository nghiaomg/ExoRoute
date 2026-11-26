//! Output-style catalog: ids, levels, selection normalization, and
//! instruction text.
//!
//! Pure data plus validation; no request inspection or mutation.

use serde::{Deserialize, Serialize};

pub const MAX_STYLE_SELECTIONS: usize = 3;
pub const MAX_STYLES_PAYLOAD_BYTES: usize = 4 * 1024;
pub const MAX_INSTRUCTION_BYTES: usize = 16 * 1024;
pub const OUTPUT_STYLE_FORMAT_VERSION: i64 = 1;
pub const OUTPUT_STYLE_MARKER: &str = "[ExoRoute Output Styles]";
pub const SHARED_BOUNDARIES: &str = "Code blocks, file paths, commands, errors, URLs: keep exact. Security warnings, irreversible action confirmations, multi-step ordered sequences: write normal. Resume terse style after. Active every response until user asks for normal mode.";

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub enum OutputStyleId {
    #[serde(rename = "terse-prose")]
    TerseProse,
    #[serde(rename = "less-code")]
    LessCode,
    #[serde(rename = "ponytail")]
    Ponytail,
}

impl OutputStyleId {
    pub const ALL: [Self; MAX_STYLE_SELECTIONS] =
        [Self::TerseProse, Self::LessCode, Self::Ponytail];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::TerseProse => "terse-prose",
            Self::LessCode => "less-code",
            Self::Ponytail => "ponytail",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "terse-prose" => Ok(Self::TerseProse),
            "less-code" => Ok(Self::LessCode),
            "ponytail" => Ok(Self::Ponytail),
            _ => Err(format!("unknown output style '{value}'")),
        }
    }

    pub(crate) const fn index(self) -> usize {
        match self {
            Self::TerseProse => 0,
            Self::LessCode => 1,
            Self::Ponytail => 2,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OutputStyleLevel {
    Lite,
    Full,
    Ultra,
}

impl OutputStyleLevel {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Lite => "lite",
            Self::Full => "full",
            Self::Ultra => "ultra",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "lite" => Ok(Self::Lite),
            "full" => Ok(Self::Full),
            "ultra" => Ok(Self::Ultra),
            _ => Err(format!("unknown output style level '{value}'")),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct OutputStyleSelection {
    pub id: OutputStyleId,
    pub level: OutputStyleLevel,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct OutputStylesSnapshot {
    pub styles: Vec<OutputStyleSelection>,
    pub revision: i64,
    pub overridden: bool,
}

pub fn normalize_styles(
    styles: &[OutputStyleSelection],
) -> Result<Vec<OutputStyleSelection>, String> {
    if styles.len() > MAX_STYLE_SELECTIONS {
        return Err(format!(
            "at most {MAX_STYLE_SELECTIONS} output styles may be enabled"
        ));
    }
    let mut selected = [None; MAX_STYLE_SELECTIONS];
    for style in styles {
        let index = style.id.index();
        if selected[index].is_some() {
            return Err(format!(
                "output style '{}' is selected more than once",
                style.id.as_str()
            ));
        }
        selected[index] = Some(style.level);
    }
    let normalized = OutputStyleId::ALL
        .into_iter()
        .enumerate()
        .filter_map(|(index, id)| selected[index].map(|level| OutputStyleSelection { id, level }))
        .collect::<Vec<_>>();
    let encoded = bincode::serialize(&normalized)
        .map_err(|_| "output styles could not be encoded".to_owned())?;
    if encoded.len() > MAX_STYLES_PAYLOAD_BYTES {
        return Err("output styles configuration is too large".to_owned());
    }
    Ok(normalized)
}

pub fn validate_styles(
    styles: &[OutputStyleSelection],
) -> Result<Vec<OutputStyleSelection>, String> {
    let normalized = normalize_styles(styles)?;
    if !normalized.is_empty() {
        let _ = super::instruction::build_instruction(&normalized)?;
    }
    Ok(normalized)
}

pub(super) fn level_text(id: OutputStyleId, level: OutputStyleLevel) -> &'static str {
    match (id, level) {
        (OutputStyleId::TerseProse, OutputStyleLevel::Lite) => {
            "Respond concise. Drop filler, pleasantries, hedging. Keep full sentences, technical terms, code, errors, URLs, and identifiers exact."
        }
        (OutputStyleId::TerseProse, OutputStyleLevel::Full) => {
            "Respond terse like smart caveman. Drop articles (a/an/the), filler (just/really/basically/actually/simply), pleasantries, hedging. Fragments OK. Short synonyms (big not extensive, fix not implement). Keep all technical substance, code, errors, URLs, identifiers exact."
        }
        (OutputStyleId::TerseProse, OutputStyleLevel::Ultra) => {
            "Respond ultra terse. Maximum compression. Telegraphic. Abbreviate (DB/auth/config/req/res/fn/impl), strip conjunctions, arrows for causality (X → Y). One word when one word enough. Never abbreviate code symbols, API names, error strings, URLs, or identifiers."
        }
        (OutputStyleId::LessCode, OutputStyleLevel::Lite) => {
            "Write the smallest change that satisfies the request. Skip speculative abstractions."
        }
        (OutputStyleId::LessCode, OutputStyleLevel::Full) => {
            "Act like a lazy senior dev applying YAGNI. Smallest working change only. No unrequested abstractions, no premature generalization, no extra layers, no defensive scaffolding the request did not ask for. Reuse existing code over adding new code."
        }
        (OutputStyleId::LessCode, OutputStyleLevel::Ultra) => {
            "Minimal diff discipline. Touch the fewest lines that make it work. Zero new files, classes, or config unless strictly required. Inline over abstract. No \"while we're here\" extras."
        }
        (OutputStyleId::Ponytail, OutputStyleLevel::Lite) => {
            "# Ponytail (lite)\nBefore writing code: does it need to exist? Does it already exist here? Does the stdlib or an installed dep cover it? Only then: write the minimum. Reuse over rewrite."
        }
        (OutputStyleId::Ponytail, OutputStyleLevel::Full) => {
            "# Ponytail — lazy senior dev\n\nYou are a lazy senior developer. Lazy = efficient, not careless. The best code is the code never written.\n\nBefore writing any code, stop at the first rung that holds:\n1. Does this need to exist? (YAGNI)\n2. Does it already exist in this codebase? Reuse it.\n3. Does the stdlib do this? Use it.\n4. Does a platform feature or installed dep cover it? Use it.\n5. Can it be one line? Make it one line.\n6. Only then: write the minimum that works.\n\nBug fix = root cause, not symptom. Grep every caller of the function you touch; fix the shared function once — one guard there is a smaller diff than one per caller.\n\nRules:\n- No unrequested abstractions. No new deps. No boilerplate.\n- Deletion over addition. Boring over clever. Fewest files.\n- Shortest working diff wins — but only after you understand the problem.\n- Question complex asks: \"Do you need X, or does Y cover it?\"\n- When two solutions tie, pick the edge-case-correct one."
        }
        (OutputStyleId::Ponytail, OutputStyleLevel::Ultra) => {
            "# Ponytail (ultra)\nLazy senior dev. Best code = code never written. Before any code: YAGNI → reuse → stdlib → platform → installed dep → one line → minimum that works. Fix root cause not symptom: grep every caller, patch shared function once. No unrequested abstractions, no new deps, no boilerplate. Deletion > addition. Fewest files. Shortest working diff, only after understanding the problem. Question complex asks. Edge-case-correct when tied."
        }
    }
}
