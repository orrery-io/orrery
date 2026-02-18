use crate::style::escape_xml;
use orrery::diagram::Bounds;

pub(crate) fn render_subprocess(
    name: Option<&str>,
    eid: &str,
    b: &Bounds,
    cx: f32,
    cy: f32,
    fill: &str,
    stroke: &str,
    sw: &str,
    shape_class: &str,
) -> String {
    let label = name.unwrap_or(eid);
    format!(
        "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"6\" \
         fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"{sw}\" stroke-dasharray=\"4,2\" class=\"{shape_class}\"/>\n\
         <text x=\"{cx}\" y=\"{}\" text-anchor=\"middle\" font-size=\"11\" fill=\"#1e293b\" class=\"bpmn-text\">{}</text>\n",
        b.x, b.y, b.width, b.height,
        cy + 4.0,
        escape_xml(label)
    )
}

pub(crate) fn render_event_subprocess(
    name: Option<&str>,
    eid: &str,
    b: &Bounds,
    cx: f32,
    cy: f32,
    fill: &str,
    stroke: &str,
    sw: &str,
    shape_class: &str,
) -> String {
    let label = name.unwrap_or(eid);
    format!(
        "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"4\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"{sw}\" stroke-dasharray=\"4 2\" class=\"{shape_class}\"/>\n\
         <text x=\"{cx}\" y=\"{}\" text-anchor=\"middle\" font-size=\"11\" fill=\"#1e293b\" class=\"bpmn-text\">{}</text>\n",
        b.x, b.y, b.width, b.height, cy + 4.0, escape_xml(label)
    )
}
