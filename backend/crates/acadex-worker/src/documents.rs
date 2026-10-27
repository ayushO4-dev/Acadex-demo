//! Typst templates for the PDFs the worker makes. Each template gets its data
//! as JSON in `sys.inputs.data` (already formatted, set as text, never
//! parsed as markup) plus optional binary inputs such as images.

use serde_json::Value;
use typst::foundations::{Bytes, Dict, IntoValue};
use typst_as_lib::{TypstEngine, TypstTemplateMainFile};
use typst_layout::PagedDocument;

pub struct Template {
    name: &'static str,
    engine: TypstEngine<TypstTemplateMainFile>,
}

impl Template {
    pub fn new(name: &'static str, source: &'static str) -> Self {
        Self {
            name,
            engine: TypstEngine::builder()
                .main_file(source)
                .fonts(typst_assets::fonts())
                .build(),
        }
    }

    /// CPU-bound; call from a blocking thread.
    pub fn render(&self, data: &Value, binaries: Vec<(&str, Vec<u8>)>) -> anyhow::Result<Vec<u8>> {
        let mut inputs = Dict::new();
        inputs.insert("data".into(), data.to_string().into_value());
        for (key, bytes) in binaries {
            inputs.insert(key.into(), Bytes::new(bytes).into_value());
        }
        let document: PagedDocument = self
            .engine
            .compile_with_input(inputs)
            .output
            .map_err(|err| anyhow::anyhow!("{} template: {err:?}", self.name))?;
        typst_pdf::pdf(&document, &Default::default())
            .map_err(|err| anyhow::anyhow!("{} PDF export: {err:?}", self.name))
    }
}

/// Every template, loaded once at start-up.
pub struct Documents {
    pub exam_form: Template,
    pub hall_ticket: Template,
    pub grade_card: Template,
}

impl Default for Documents {
    fn default() -> Self {
        Self {
            exam_form: Template::new("exam form", include_str!("../templates/exam_form.typ")),
            hall_ticket: Template::new("hall ticket", include_str!("../templates/hall_ticket.typ")),
            grade_card: Template::new("grade card", include_str!("../templates/grade_card.typ")),
        }
    }
}

/// A QR code for `text`, as SVG bytes Typst can place as an image.
pub fn qr_svg(text: &str) -> anyhow::Result<Vec<u8>> {
    let code = qrcode::QrCode::with_error_correction_level(text, qrcode::EcLevel::M)?;
    Ok(code
        .render::<qrcode::render::svg::Color>()
        .min_dimensions(200, 200)
        .quiet_zone(true)
        .build()
        .into_bytes())
}
