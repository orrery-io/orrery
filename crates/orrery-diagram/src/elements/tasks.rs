use crate::icons;
use crate::style::escape_xml;
use orrery::diagram::Bounds;

pub(crate) fn render_service_task(
    name: Option<&str>,
    eid: &str,
    b: &Bounds,
    cx: f32,
    cy: f32,
    fill: &str,
    stroke: &str,
    sw: &str,
    shape_class: &str,
    has_topic: bool,
) -> String {
    let label = name.unwrap_or(eid);
    let icon = if has_topic {
        let sz = b.height.min(b.width) * 0.30;
        icons::gear_task_icon(b.x + 3.0, b.y + 2.0, sz, "#64748b")
    } else {
        String::new()
    };
    format!(
        "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"4\" \
         fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"{sw}\" class=\"{shape_class}\"/>\n\
         {icon}\n<text x=\"{cx}\" y=\"{}\" text-anchor=\"middle\" font-size=\"11\" fill=\"#1e293b\" class=\"bpmn-text\">{}</text>\n",
        b.x, b.y, b.width, b.height,
        cy + 4.0,
        escape_xml(label)
    )
}

pub(crate) fn render_multi_instance_task(
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
    let mi = icons::multi_instance_task_icon(b.x + 4.0, b.y + 4.0, "#64748b");
    format!(
        "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"4\" \
         fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"{sw}\" class=\"{shape_class}\"/>\n\
         {mi}\n\
         <text x=\"{cx}\" y=\"{}\" text-anchor=\"middle\" font-size=\"11\" fill=\"#1e293b\" class=\"bpmn-text\">{}</text>\n",
        b.x, b.y, b.width, b.height,
        cy + 4.0,
        escape_xml(label)
    )
}

pub(crate) fn render_script_task(
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
    let icon = icons::script_task_icon(b.x + 4.0, b.y + 4.0, "#64748b");
    format!(
        "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"4\" \
         fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"{sw}\" class=\"{shape_class}\"/>\n\
         {icon}\n\
         <text x=\"{cx}\" y=\"{}\" text-anchor=\"middle\" font-size=\"11\" fill=\"#1e293b\" class=\"bpmn-text\">{}</text>\n",
        b.x, b.y, b.width, b.height,
        cy + 4.0,
        escape_xml(label)
    )
}

pub(crate) fn render_receive_task(
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
    let env = icons::envelope_task_icon(b.x + 4.0, b.y + 4.0, "#64748b");
    format!(
        "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"4\" \
         fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"{sw}\" class=\"{shape_class}\"/>\n\
         {env}\n\
         <text x=\"{cx}\" y=\"{}\" text-anchor=\"middle\" font-size=\"11\" fill=\"#1e293b\" class=\"bpmn-text\">{}</text>\n",
        b.x, b.y, b.width, b.height,
        cy + 4.0,
        escape_xml(label)
    )
}
