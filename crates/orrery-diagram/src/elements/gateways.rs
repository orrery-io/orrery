use crate::style::escape_xml;
use orrery::diagram::Bounds;

pub(crate) fn render_exclusive_gateway(
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
    let half = b.width.min(b.height) / 2.0;
    let arm = half * 0.35;
    let points = format!(
        "{},{} {},{} {},{} {},{}",
        cx,
        b.y,
        b.x + b.width,
        cy,
        cx,
        b.y + b.height,
        b.x,
        cy,
    );
    format!(
        "<polygon points=\"{points}\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"{sw}\" class=\"{shape_class}\"/>\n\
         <line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{stroke}\" stroke-width=\"2.5\" class=\"bpmn-ring\"/>\n\
         <line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{stroke}\" stroke-width=\"2.5\" class=\"bpmn-ring\"/>\n\
         <text x=\"{cx}\" y=\"{}\" text-anchor=\"middle\" font-size=\"9\" fill=\"#1e293b\" class=\"bpmn-text\">{}</text>\n",
        cx - arm, cy - arm, cx + arm, cy + arm,
        cx - arm, cy + arm, cx + arm, cy - arm,
        b.y + b.height + 14.0,
        escape_xml(label)
    )
}

pub(crate) fn render_parallel_gateway(
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
    let half = b.width.min(b.height) / 2.0;
    let arm = half * 0.35;
    let points = format!(
        "{},{} {},{} {},{} {},{}",
        cx,
        b.y,
        b.x + b.width,
        cy,
        cx,
        b.y + b.height,
        b.x,
        cy,
    );
    format!(
        "<polygon points=\"{points}\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"{sw}\" class=\"{shape_class}\"/>\n\
         <line x1=\"{cx}\" y1=\"{}\" x2=\"{cx}\" y2=\"{}\" stroke=\"{stroke}\" stroke-width=\"2.5\" class=\"bpmn-ring\"/>\n\
         <line x1=\"{}\" y1=\"{cy}\" x2=\"{}\" y2=\"{cy}\" stroke=\"{stroke}\" stroke-width=\"2.5\" class=\"bpmn-ring\"/>\n\
         <text x=\"{cx}\" y=\"{}\" text-anchor=\"middle\" font-size=\"9\" fill=\"#1e293b\" class=\"bpmn-text\">{}</text>\n",
        cy - arm, cy + arm,
        cx - arm, cx + arm,
        b.y + b.height + 14.0,
        escape_xml(label)
    )
}

pub(crate) fn render_inclusive_gateway(
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
    let half = b.width.min(b.height) / 2.0;
    let points = format!(
        "{},{} {},{} {},{} {},{}",
        cx,
        b.y,
        b.x + b.width,
        cy,
        cx,
        b.y + b.height,
        b.x,
        cy,
    );
    format!(
        "<polygon points=\"{points}\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"{sw}\" class=\"{shape_class}\"/>\n\
         <circle cx=\"{cx}\" cy=\"{cy}\" r=\"{}\" fill=\"none\" stroke=\"{stroke}\" stroke-width=\"2\" class=\"bpmn-ring\"/>\n\
         <text x=\"{cx}\" y=\"{}\" text-anchor=\"middle\" font-size=\"9\" fill=\"#1e293b\" class=\"bpmn-text\">{}</text>\n",
        half * 0.45,
        b.y + b.height + 14.0,
        escape_xml(label)
    )
}

pub(crate) fn render_event_based_gateway(
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
    let half = b.width.min(b.height) / 2.0;
    let points = format!(
        "{},{} {},{} {},{} {},{}",
        cx,
        b.y,
        b.x + b.width,
        cy,
        cx,
        b.y + b.height,
        b.x,
        cy,
    );
    let r_outer = half * 0.55;
    let r_inner = half * 0.40;
    let pent = (0..5)
        .map(|i| {
            let angle = std::f32::consts::FRAC_PI_2 + (i as f32) * 2.0 * std::f32::consts::PI / 5.0;
            let px = cx - r_inner * angle.cos();
            let py = cy - r_inner * angle.sin();
            format!("{px:.1},{py:.1}")
        })
        .collect::<Vec<_>>()
        .join(" ");
    format!(
        "<polygon points=\"{points}\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"{sw}\" class=\"{shape_class}\"/>\n\
         <circle cx=\"{cx}\" cy=\"{cy}\" r=\"{r_outer}\" fill=\"none\" stroke=\"{stroke}\" stroke-width=\"1.5\" class=\"bpmn-ring\"/>\n\
         <circle cx=\"{cx}\" cy=\"{cy}\" r=\"{r_inner}\" fill=\"none\" stroke=\"{stroke}\" stroke-width=\"1.5\" class=\"bpmn-ring\"/>\n\
         <polygon points=\"{pent}\" fill=\"none\" stroke=\"{stroke}\" stroke-width=\"1.5\" class=\"bpmn-ring\"/>\n\
         <text x=\"{cx}\" y=\"{}\" text-anchor=\"middle\" font-size=\"9\" fill=\"#1e293b\" class=\"bpmn-text\">{}</text>\n",
        b.y + b.height + 14.0,
        escape_xml(label)
    )
}
