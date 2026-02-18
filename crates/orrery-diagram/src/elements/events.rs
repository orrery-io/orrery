use crate::icons;
use crate::style::escape_xml;
use orrery::diagram::Bounds;
use orrery::model::EventSubProcessTrigger;

pub(crate) fn render_start_event(
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
    let r = b.width.min(b.height) / 2.0;
    format!(
        "<circle cx=\"{cx}\" cy=\"{cy}\" r=\"{r}\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"{sw}\" class=\"{shape_class}\"/>\n\
         <text x=\"{cx}\" y=\"{}\" text-anchor=\"middle\" font-size=\"10\" fill=\"#1e293b\" class=\"bpmn-text\">{}</text>\n",
        cy + r + 14.0,
        escape_xml(label)
    )
}

pub(crate) fn render_timer_start_event(
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
    let r = b.width.min(b.height) / 2.0;
    let clock = icons::timer_icon(cx, cy, r, stroke);
    format!(
        "<circle cx=\"{cx}\" cy=\"{cy}\" r=\"{r}\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"{sw}\" class=\"{shape_class}\"/>\n\
         {clock}\n\
         <text x=\"{cx}\" y=\"{}\" text-anchor=\"middle\" font-size=\"10\" fill=\"#1e293b\" class=\"bpmn-text\">{}</text>\n",
        cy + r + 14.0,
        escape_xml(label)
    )
}

pub(crate) fn render_message_start_event(
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
    let r = b.width.min(b.height) / 2.0;
    let env = icons::envelope_icon(cx, cy, r, stroke);
    format!(
        "<circle cx=\"{cx}\" cy=\"{cy}\" r=\"{r}\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"{sw}\" class=\"{shape_class}\"/>\n\
         {env}\n\
         <text x=\"{cx}\" y=\"{}\" text-anchor=\"middle\" font-size=\"10\" fill=\"#1e293b\" class=\"bpmn-text\">{}</text>\n",
        cy + r + 14.0,
        escape_xml(label)
    )
}

pub(crate) fn render_signal_start_event(
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
    let r = b.width.min(b.height) / 2.0;
    let sig = icons::signal_icon(cx, cy, r, stroke);
    format!(
        "<circle cx=\"{cx}\" cy=\"{cy}\" r=\"{r}\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"{sw}\" class=\"{shape_class}\"/>\n\
         {sig}\n\
         <text x=\"{cx}\" y=\"{}\" text-anchor=\"middle\" font-size=\"10\" fill=\"#1e293b\" class=\"bpmn-text\">{}</text>\n",
        cy + r + 14.0,
        escape_xml(label)
    )
}

pub(crate) fn render_end_event(
    name: Option<&str>,
    eid: &str,
    b: &Bounds,
    cx: f32,
    cy: f32,
    fill: &str,
    stroke: &str,
    _sw: &str,
    shape_class: &str,
) -> String {
    let label = name.unwrap_or(eid);
    let r = b.width.min(b.height) / 2.0;
    format!(
        "<circle cx=\"{cx}\" cy=\"{cy}\" r=\"{r}\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"3\" class=\"{shape_class}\"/>\n\
         <text x=\"{cx}\" y=\"{}\" text-anchor=\"middle\" font-size=\"10\" fill=\"#1e293b\" class=\"bpmn-text\">{}</text>\n",
        cy + r + 14.0,
        escape_xml(label)
    )
}

pub(crate) fn render_error_end_event(
    name: Option<&str>,
    eid: &str,
    b: &Bounds,
    cx: f32,
    cy: f32,
    fill: &str,
    stroke: &str,
    _sw: &str,
    shape_class: &str,
) -> String {
    let label = name.unwrap_or(eid);
    let r = b.width.min(b.height) / 2.0;
    let bolt = format!(
        "<path d=\"M{},{} L{},{} L{},{} L{},{} L{},{} L{},{} Z\" fill=\"{stroke}\" stroke=\"none\"/>",
        cx - 3.0, cy - 7.0, cx + 2.0, cy - 1.0, cx - 1.0, cy - 1.0,
        cx + 3.0, cy + 7.0, cx - 2.0, cy + 1.0, cx + 1.0, cy + 1.0
    );
    format!(
        "<circle cx=\"{cx}\" cy=\"{cy}\" r=\"{r}\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"3\" class=\"{shape_class}\"/>\n\
         {bolt}\n\
         <text x=\"{cx}\" y=\"{}\" text-anchor=\"middle\" font-size=\"9\" fill=\"#1e293b\" class=\"bpmn-text\">{}</text>\n",
        cy + r + 14.0, escape_xml(label)
    )
}

pub(crate) fn render_terminate_end_event(
    name: Option<&str>,
    b: &Bounds,
    cx: f32,
    cy: f32,
    fill: &str,
    stroke: &str,
    _sw: &str,
    shape_class: &str,
) -> String {
    let label = name.unwrap_or("Terminate");
    let r = b.width.min(b.height) / 2.0;
    let inner_r = r * 0.6;
    format!(
        "<circle cx=\"{cx}\" cy=\"{cy}\" r=\"{r}\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"3\" class=\"{shape_class}\"/>\n\
         <circle cx=\"{cx}\" cy=\"{cy}\" r=\"{inner_r}\" fill=\"{stroke}\" stroke=\"none\"/>\n\
         <text x=\"{cx}\" y=\"{}\" text-anchor=\"middle\" font-size=\"10\" fill=\"#1e293b\" class=\"bpmn-text\">{}</text>\n",
        cy + r + 14.0,
        escape_xml(label)
    )
}

pub(crate) fn render_message_end_event(
    name: Option<&str>,
    eid: &str,
    b: &Bounds,
    cx: f32,
    cy: f32,
    fill: &str,
    stroke: &str,
    _sw: &str,
    shape_class: &str,
) -> String {
    let label = name.unwrap_or(eid);
    let r = b.width.min(b.height) / 2.0;
    let env = icons::envelope_icon(cx, cy, r, stroke);
    format!(
        "<circle cx=\"{cx}\" cy=\"{cy}\" r=\"{r}\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"3\" class=\"{shape_class}\"/>\n\
         {env}\n\
         <text x=\"{cx}\" y=\"{}\" text-anchor=\"middle\" font-size=\"10\" fill=\"#1e293b\" class=\"bpmn-text\">{}</text>\n",
        cy + r + 14.0,
        escape_xml(label)
    )
}

pub(crate) fn render_signal_end_event(
    name: Option<&str>,
    eid: &str,
    b: &Bounds,
    cx: f32,
    cy: f32,
    fill: &str,
    stroke: &str,
    _sw: &str,
    shape_class: &str,
) -> String {
    let label = name.unwrap_or(eid);
    let r = b.width.min(b.height) / 2.0;
    let sig = icons::signal_icon_filled(cx, cy, r, stroke);
    format!(
        "<circle cx=\"{cx}\" cy=\"{cy}\" r=\"{r}\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"3\" class=\"{shape_class}\"/>\n\
         {sig}\n\
         <text x=\"{cx}\" y=\"{}\" text-anchor=\"middle\" font-size=\"9\" fill=\"#1e293b\" class=\"bpmn-text\">{}</text>\n",
        cy + r + 14.0, escape_xml(label)
    )
}

pub(crate) fn render_escalation_end_event(
    name: Option<&str>,
    eid: &str,
    b: &Bounds,
    cx: f32,
    cy: f32,
    fill: &str,
    stroke: &str,
    _sw: &str,
    shape_class: &str,
) -> String {
    let label = name.unwrap_or(eid);
    let r = b.width.min(b.height) / 2.0;
    format!(
        "<circle cx=\"{cx}\" cy=\"{cy}\" r=\"{r}\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"3\" class=\"{shape_class}\"/>\n\
         <text x=\"{cx}\" y=\"{}\" text-anchor=\"middle\" font-size=\"12\" fill=\"{stroke}\" class=\"bpmn-ring\">&#x2B06;</text>\n\
         <text x=\"{cx}\" y=\"{}\" text-anchor=\"middle\" font-size=\"9\" fill=\"#1e293b\" class=\"bpmn-text\">{}</text>\n",
        cy + 4.0,
        cy + r + 14.0, escape_xml(label)
    )
}

pub(crate) fn render_timer_intermediate_event(
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
    let r = b.width.min(b.height) / 2.0;
    let r_inner = r * 0.8;
    let clock = icons::timer_icon(cx, cy, r, stroke);
    format!(
        "<circle cx=\"{cx}\" cy=\"{cy}\" r=\"{r}\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"{sw}\" class=\"{shape_class}\"/>\n\
         <circle cx=\"{cx}\" cy=\"{cy}\" r=\"{r_inner}\" fill=\"none\" stroke=\"{stroke}\" stroke-width=\"1\" class=\"bpmn-ring\"/>\n\
         {clock}\n\
         <text x=\"{cx}\" y=\"{}\" text-anchor=\"middle\" font-size=\"9\" fill=\"#1e293b\" class=\"bpmn-text\">{}</text>\n",
        cy + r + 14.0,
        escape_xml(label)
    )
}

pub(crate) fn render_message_intermediate_catch_event(
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
    let r = b.width.min(b.height) / 2.0;
    let r_inner = r * 0.8;
    let env = icons::envelope_icon(cx, cy, r, stroke);
    format!(
        "<circle cx=\"{cx}\" cy=\"{cy}\" r=\"{r}\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"{sw}\" class=\"{shape_class}\"/>\n\
         <circle cx=\"{cx}\" cy=\"{cy}\" r=\"{r_inner}\" fill=\"none\" stroke=\"{stroke}\" stroke-width=\"1\" class=\"bpmn-ring\"/>\n\
         {env}\n\
         <text x=\"{cx}\" y=\"{}\" text-anchor=\"middle\" font-size=\"9\" fill=\"#1e293b\" class=\"bpmn-text\">{}</text>\n",
        cy + r + 14.0,
        escape_xml(label)
    )
}

pub(crate) fn render_signal_intermediate_catch_event(
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
    let r = b.width.min(b.height) / 2.0;
    let r_inner = r * 0.8;
    let sig = icons::signal_icon(cx, cy, r, stroke);
    format!(
        "<circle cx=\"{cx}\" cy=\"{cy}\" r=\"{r}\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"{sw}\" class=\"{shape_class}\"/>\n\
         <circle cx=\"{cx}\" cy=\"{cy}\" r=\"{r_inner}\" fill=\"none\" stroke=\"{stroke}\" stroke-width=\"1\" class=\"bpmn-ring\"/>\n\
         {sig}\n\
         <text x=\"{cx}\" y=\"{}\" text-anchor=\"middle\" font-size=\"9\" fill=\"#1e293b\" class=\"bpmn-text\">{}</text>\n",
        cy + r + 14.0,
        escape_xml(label)
    )
}

pub(crate) fn render_intermediate_throw_event(
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
    let r = b.width.min(b.height) / 2.0;
    let r_inner = r * 0.8;
    format!(
        "<circle cx=\"{cx}\" cy=\"{cy}\" r=\"{r}\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"{sw}\" class=\"{shape_class}\"/>\n\
         <circle cx=\"{cx}\" cy=\"{cy}\" r=\"{r_inner}\" fill=\"{stroke}\" fill-opacity=\"0.15\" stroke=\"{stroke}\" stroke-width=\"1\" class=\"bpmn-ring\"/>\n\
         <text x=\"{cx}\" y=\"{}\" text-anchor=\"middle\" font-size=\"9\" fill=\"#1e293b\" class=\"bpmn-text\">{}</text>\n",
        cy + r + 14.0,
        escape_xml(label)
    )
}

pub(crate) fn render_message_intermediate_throw_event(
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
    let r = b.width.min(b.height) / 2.0;
    let r_inner = r * 0.8;
    let env = icons::envelope_icon(cx, cy, r, stroke);
    format!(
        "<circle cx=\"{cx}\" cy=\"{cy}\" r=\"{r}\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"{sw}\" class=\"{shape_class}\"/>\n\
         <circle cx=\"{cx}\" cy=\"{cy}\" r=\"{r_inner}\" fill=\"{stroke}\" fill-opacity=\"0.15\" stroke=\"{stroke}\" stroke-width=\"1\" class=\"bpmn-ring\"/>\n\
         {env}\n\
         <text x=\"{cx}\" y=\"{}\" text-anchor=\"middle\" font-size=\"9\" fill=\"#1e293b\" class=\"bpmn-text\">{}</text>\n",
        cy + r + 14.0,
        escape_xml(label)
    )
}

pub(crate) fn render_signal_intermediate_throw_event(
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
    let r = b.width.min(b.height) / 2.0;
    let r_inner = r * 0.8;
    let sig = icons::signal_icon_filled(cx, cy, r, stroke);
    format!(
        "<circle cx=\"{cx}\" cy=\"{cy}\" r=\"{r}\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"{sw}\" class=\"{shape_class}\"/>\n\
         <circle cx=\"{cx}\" cy=\"{cy}\" r=\"{r_inner}\" fill=\"none\" stroke=\"{stroke}\" stroke-width=\"1\" class=\"bpmn-ring\"/>\n\
         {sig}\n\
         <text x=\"{cx}\" y=\"{}\" text-anchor=\"middle\" font-size=\"9\" fill=\"#1e293b\" class=\"bpmn-text\">{}</text>\n",
        cy + r + 14.0, escape_xml(label)
    )
}

pub(crate) fn render_escalation_intermediate_throw_event(
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
    let r = b.width.min(b.height) / 2.0;
    let r_inner = r * 0.8;
    format!(
        "<circle cx=\"{cx}\" cy=\"{cy}\" r=\"{r}\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"{sw}\" class=\"{shape_class}\"/>\n\
         <circle cx=\"{cx}\" cy=\"{cy}\" r=\"{r_inner}\" fill=\"{stroke}\" fill-opacity=\"0.15\" stroke=\"{stroke}\" stroke-width=\"1\" class=\"bpmn-ring\"/>\n\
         <text x=\"{cx}\" y=\"{}\" text-anchor=\"middle\" font-size=\"12\" fill=\"{stroke}\" class=\"bpmn-ring\">&#x2B06;</text>\n\
         <text x=\"{cx}\" y=\"{}\" text-anchor=\"middle\" font-size=\"9\" fill=\"#1e293b\" class=\"bpmn-text\">{}</text>\n",
        cy + 4.0,
        cy + r + 14.0, escape_xml(label)
    )
}

pub(crate) fn render_link_intermediate_throw_event(
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
    let r = b.width.min(b.height) / 2.0;
    let r_inner = r * 0.8;
    format!(
        "<circle cx=\"{cx}\" cy=\"{cy}\" r=\"{r}\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"{sw}\" class=\"{shape_class}\"/>\n\
         <circle cx=\"{cx}\" cy=\"{cy}\" r=\"{r_inner}\" fill=\"{stroke}\" fill-opacity=\"0.15\" stroke=\"{stroke}\" stroke-width=\"1\" class=\"bpmn-ring\"/>\n\
         <text x=\"{cx}\" y=\"{}\" text-anchor=\"middle\" font-size=\"12\" fill=\"{stroke}\" class=\"bpmn-ring\">&#x27A1;</text>\n\
         <text x=\"{cx}\" y=\"{}\" text-anchor=\"middle\" font-size=\"9\" fill=\"#1e293b\" class=\"bpmn-text\">{}</text>\n",
        cy + 4.0,
        cy + r + 14.0, escape_xml(label)
    )
}

pub(crate) fn render_link_intermediate_catch_event(
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
    let r = b.width.min(b.height) / 2.0;
    let r_inner = r * 0.8;
    format!(
        "<circle cx=\"{cx}\" cy=\"{cy}\" r=\"{r}\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"{sw}\" class=\"{shape_class}\"/>\n\
         <circle cx=\"{cx}\" cy=\"{cy}\" r=\"{r_inner}\" fill=\"none\" stroke=\"{stroke}\" stroke-width=\"1\" class=\"bpmn-ring\"/>\n\
         <text x=\"{cx}\" y=\"{}\" text-anchor=\"middle\" font-size=\"12\" fill=\"{stroke}\" class=\"bpmn-ring\">&#x27A1;</text>\n\
         <text x=\"{cx}\" y=\"{}\" text-anchor=\"middle\" font-size=\"9\" fill=\"#1e293b\" class=\"bpmn-text\">{}</text>\n",
        cy + 4.0,
        cy + r + 14.0, escape_xml(label)
    )
}

pub(crate) fn render_boundary_event(
    name: Option<&str>,
    b: &Bounds,
    cx: f32,
    cy: f32,
    fill: &str,
    stroke: &str,
    sw: &str,
    shape_class: &str,
) -> String {
    let label = name.unwrap_or("⚠");
    let r = b.width.min(b.height) / 2.0;
    format!(
        "<circle cx=\"{cx}\" cy=\"{cy}\" r=\"{r}\" fill=\"{fill}\" stroke=\"{stroke}\" \
         stroke-width=\"{sw}\" stroke-dasharray=\"3,2\" class=\"{shape_class}\"/>\n\
         <text x=\"{cx}\" y=\"{}\" text-anchor=\"middle\" font-size=\"9\" fill=\"#1e293b\" class=\"bpmn-text\">{}</text>\n",
        cy + 3.0,
        escape_xml(label)
    )
}

pub(crate) fn render_message_boundary_event(
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
    let r = b.width.min(b.height) / 2.0;
    let env = icons::envelope_icon(cx, cy, r, stroke);
    format!(
        "<circle cx=\"{cx}\" cy=\"{cy}\" r=\"{r}\" fill=\"{fill}\" stroke=\"{stroke}\" \
         stroke-width=\"{sw}\" stroke-dasharray=\"3,2\" class=\"{shape_class}\"/>\n\
         {env}\n\
         <text x=\"{cx}\" y=\"{}\" text-anchor=\"middle\" font-size=\"9\" fill=\"#1e293b\" class=\"bpmn-text\">{}</text>\n",
        cy + r + 10.0,
        escape_xml(label)
    )
}

pub(crate) fn render_timer_boundary_event(
    name: Option<&str>,
    eid: &str,
    b: &Bounds,
    cx: f32,
    cy: f32,
    fill: &str,
    stroke: &str,
    sw: &str,
    shape_class: &str,
    is_interrupting: bool,
) -> String {
    let label = name.unwrap_or(eid);
    let r = b.width.min(b.height) / 2.0;
    let dash = if is_interrupting {
        ""
    } else {
        " stroke-dasharray=\"3,2\""
    };
    let clk = icons::clock_icon(cx, cy, r, stroke);
    format!(
        "<circle cx=\"{cx}\" cy=\"{cy}\" r=\"{r}\" fill=\"{fill}\" stroke=\"{stroke}\" \
         stroke-width=\"{sw}\"{dash} class=\"{shape_class}\"/>\n\
         {clk}\n\
         <text x=\"{cx}\" y=\"{}\" text-anchor=\"middle\" font-size=\"9\" fill=\"#1e293b\" class=\"bpmn-text\">{}</text>\n",
        cy + r + 10.0,
        escape_xml(label)
    )
}

pub(crate) fn render_signal_boundary_event(
    name: Option<&str>,
    eid: &str,
    b: &Bounds,
    cx: f32,
    cy: f32,
    fill: &str,
    stroke: &str,
    sw: &str,
    shape_class: &str,
    is_interrupting: bool,
) -> String {
    let label = name.unwrap_or(eid);
    let r = b.width.min(b.height) / 2.0;
    let dash = if is_interrupting {
        ""
    } else {
        " stroke-dasharray=\"3,2\""
    };
    let sig = icons::signal_icon(cx, cy, r, stroke);
    format!(
        "<circle cx=\"{cx}\" cy=\"{cy}\" r=\"{r}\" fill=\"{fill}\" stroke=\"{stroke}\" \
         stroke-width=\"{sw}\"{dash} class=\"{shape_class}\"/>\n\
         {sig}\n\
         <text x=\"{cx}\" y=\"{}\" text-anchor=\"middle\" font-size=\"9\" fill=\"#1e293b\" class=\"bpmn-text\">{}</text>\n",
        cy + r + 10.0,
        escape_xml(label)
    )
}

pub(crate) fn render_escalation_boundary_event(
    name: Option<&str>,
    eid: &str,
    b: &Bounds,
    cx: f32,
    cy: f32,
    fill: &str,
    stroke: &str,
    sw: &str,
    shape_class: &str,
    is_interrupting: bool,
) -> String {
    let label = name.unwrap_or(eid);
    let r = b.width.min(b.height) / 2.0;
    let dash = if is_interrupting {
        ""
    } else {
        " stroke-dasharray=\"3,2\""
    };
    format!(
        "<circle cx=\"{cx}\" cy=\"{cy}\" r=\"{r}\" fill=\"{fill}\" stroke=\"{stroke}\" \
         stroke-width=\"{sw}\"{dash} class=\"{shape_class}\"/>\n\
         <text x=\"{cx}\" y=\"{}\" text-anchor=\"middle\" font-size=\"12\" fill=\"{stroke}\" class=\"bpmn-ring\">&#x2B06;</text>\n\
         <text x=\"{cx}\" y=\"{}\" text-anchor=\"middle\" font-size=\"9\" fill=\"#1e293b\" class=\"bpmn-text\">{}</text>\n",
        cy + 4.0,
        cy + r + 10.0, escape_xml(label)
    )
}

pub(crate) fn render_esp_start_event(
    name: Option<&str>,
    trigger: &EventSubProcessTrigger,
    b: &Bounds,
    cx: f32,
    cy: f32,
    fill: &str,
    stroke: &str,
    sw: &str,
    shape_class: &str,
) -> String {
    let label = name.unwrap_or("");
    let r = b.width.min(b.height) / 2.0;
    let icon = icons::esp_trigger_icon(trigger, cx, cy, r, stroke);
    format!(
        "<circle cx=\"{cx}\" cy=\"{cy}\" r=\"{r}\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"{sw}\" stroke-dasharray=\"2 1\" class=\"{shape_class}\"/>\n\
         {icon}\n\
         <text x=\"{cx}\" y=\"{}\" text-anchor=\"middle\" font-size=\"9\" fill=\"#1e293b\" class=\"bpmn-text\">{}</text>\n",
        cy + r + 14.0, escape_xml(label)
    )
}
