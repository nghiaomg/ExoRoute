//! Instruction assembly: deterministic English prompt text for a
//! validated style selection.
//!
//! The gateway injects the result exactly once into the canonical
//! system/developer context; provider responses are never rewritten.

use super::catalog::{
    MAX_INSTRUCTION_BYTES, OUTPUT_STYLE_MARKER, OutputStyleSelection, SHARED_BOUNDARIES, level_text,
};

pub(super) fn build_instruction(styles: &[OutputStyleSelection]) -> Result<String, String> {
    let mut instruction = String::from(OUTPUT_STYLE_MARKER);
    instruction.push('\n');
    for (index, style) in styles.iter().enumerate() {
        if index > 0 {
            instruction.push('\n');
        }
        instruction.push_str(level_text(style.id, style.level));
    }
    instruction.push(' ');
    instruction.push_str(SHARED_BOUNDARIES);
    if instruction.len() > MAX_INSTRUCTION_BYTES {
        return Err("output style instruction exceeds the supported size".to_owned());
    }
    Ok(instruction)
}
