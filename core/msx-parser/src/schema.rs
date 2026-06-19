// core/msx-parser/src/schema.rs
use dixscript::Runtime::{DixData, ExpectedValueType, SchemaBuilder};

/// Validate the minimal required shape of an `.msx` file before attempting
/// full element parsing. `data.validate_schema` collects every problem
/// rather than failing on the first one, so a malformed file gets one
/// readable report instead of a chain of fixes-and-rerun.
pub fn validate(data: &DixData) -> Result<(), String> {
    let report = data.validate_schema(
        SchemaBuilder::new()
            .require_double("scene.width")
            .with_description("Canvas width in user units")
            .require_double("scene.height")
            .with_description("Canvas height in user units")
            .optional("scene.background", ExpectedValueType::Any)
            .with_description("Background fill — hex color or named color")
            .optional_array("elements")
            .with_description("Top-level scene element array")
            .optional_array("defs")
            .with_description("Gradient / pattern definitions"),
    );

    if report.is_valid() {
        Ok(())
    } else {
        Err(format!("MSX schema validation failed:\n{}", report))
    }
  }
