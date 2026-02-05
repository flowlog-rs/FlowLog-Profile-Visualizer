//! Report rendering (HTML).

use crate::Result;
use crate::view::ReportData;

use anyhow::Context;
use serde_json::to_string;
use std::fs;

/// Render a self-contained HTML report (data embedded as JSON).
pub fn render_html_report(data: &ReportData) -> Result<String> {
    let json = to_string(data)?; // embedded as JS object literal
    let template = fs::read_to_string("templates/report.html")
        .with_context(|| "read templates/report.html".to_string())?;

    Ok(template.replace("__DATA__", &json))
}
