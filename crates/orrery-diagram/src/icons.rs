use orrery::model::EventSubProcessTrigger;

/// Raw SVG envelope icon centered at (cx, cy), sized relative to circle radius r.
/// Used for circular message events (start, end, intermediate, boundary).
pub(crate) fn clock_icon(cx: f32, cy: f32, r: f32, stroke: &str) -> String {
    let cr = r * 0.4;
    format!(
        "<circle cx=\"{cx}\" cy=\"{cy}\" r=\"{cr}\" fill=\"none\" stroke=\"{stroke}\" stroke-width=\"1.5\" class=\"bpmn-ring\"/>\n\
         <line x1=\"{cx}\" y1=\"{cy}\" x2=\"{cx}\" y2=\"{}\" stroke=\"{stroke}\" stroke-width=\"1.5\" class=\"bpmn-ring\"/>\n\
         <line x1=\"{cx}\" y1=\"{cy}\" x2=\"{}\" y2=\"{cy}\" stroke=\"{stroke}\" stroke-width=\"1.5\" class=\"bpmn-ring\"/>",
        cy - cr * 0.5,
        cx + cr * 0.4,
    )
}

pub(crate) fn envelope_icon(cx: f32, cy: f32, r: f32, stroke: &str) -> String {
    let ew = r * 0.45;
    let eh = r * 0.30;
    format!(
        "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"none\" stroke=\"{stroke}\" stroke-width=\"1.5\" class=\"bpmn-ring\"/>\n\
         <polyline points=\"{},{} {},{} {},{}\" fill=\"none\" stroke=\"{stroke}\" stroke-width=\"1.5\" class=\"bpmn-ring\"/>",
        cx - ew, cy - eh, ew * 2.0, eh * 2.0,
        cx - ew, cy - eh, cx, cy, cx + ew, cy - eh
    )
}

/// Small SVG envelope icon for task markers at top-left corner of a rect.
pub(crate) fn envelope_task_icon(x: f32, y: f32, stroke: &str) -> String {
    let w: f32 = 12.0;
    let h: f32 = 8.0;
    format!(
        "<rect x=\"{}\" y=\"{}\" width=\"{w}\" height=\"{h}\" fill=\"none\" stroke=\"{stroke}\" stroke-width=\"1\" class=\"bpmn-muted\"/>\n\
         <polyline points=\"{},{} {},{} {},{}\" fill=\"none\" stroke=\"{stroke}\" stroke-width=\"1\" class=\"bpmn-muted\"/>",
        x, y,
        x, y, x + w / 2.0, y + h * 0.6, x + w, y
    )
}

/// Raw SVG clock icon centered at (cx, cy), sized relative to circle radius r.
/// Draws a small circle with hour+minute hands. Used for timer events.
pub(crate) fn timer_icon(cx: f32, cy: f32, r: f32, stroke: &str) -> String {
    let cr = r * 0.35; // clock face radius
    let hour_len = cr * 0.55;
    let min_len = cr * 0.8;
    format!(
        "<circle cx=\"{cx}\" cy=\"{cy}\" r=\"{cr}\" fill=\"none\" stroke=\"{stroke}\" stroke-width=\"1.2\" class=\"bpmn-ring\"/>\n\
         <line x1=\"{cx}\" y1=\"{cy}\" x2=\"{cx}\" y2=\"{}\" stroke=\"{stroke}\" stroke-width=\"1.5\" class=\"bpmn-ring\"/>\n\
         <line x1=\"{cx}\" y1=\"{cy}\" x2=\"{}\" y2=\"{cy}\" stroke=\"{stroke}\" stroke-width=\"1.2\" class=\"bpmn-ring\"/>",
        cy - hour_len,
        cx + min_len
    )
}

/// Raw SVG gear icon at (x, y) top-left corner of a task rect.
/// Draws a circle with 6 radiating teeth. Used for external service tasks.
pub(crate) fn gear_task_icon(x: f32, y: f32, size: f32, stroke: &str) -> String {
    let cx = x + size / 2.0;
    let cy = y + size / 2.0;
    let inner_r = size * 0.25;
    let outer_r = size * 0.38;
    let mut lines = format!(
        "<circle cx=\"{cx}\" cy=\"{cy}\" r=\"{inner_r}\" fill=\"none\" stroke=\"{stroke}\" stroke-width=\"1\" class=\"bpmn-muted\"/>"
    );
    // 6 teeth at 60-degree intervals
    for i in 0..6 {
        let angle = std::f32::consts::PI / 3.0 * i as f32;
        let x1 = cx + inner_r * angle.cos();
        let y1 = cy + inner_r * angle.sin();
        let x2 = cx + outer_r * angle.cos();
        let y2 = cy + outer_r * angle.sin();
        lines.push_str(&format!(
            "\n<line x1=\"{x1}\" y1=\"{y1}\" x2=\"{x2}\" y2=\"{y2}\" stroke=\"{stroke}\" stroke-width=\"1.5\" class=\"bpmn-muted\"/>"
        ));
    }
    lines
}

/// Raw SVG triangle (signal) centered at (cx, cy), sized relative to circle radius r.
/// Used for signal intermediate catch events.
pub(crate) fn signal_icon(cx: f32, cy: f32, r: f32, stroke: &str) -> String {
    let h = r * 0.7; // triangle height
    let hw = r * 0.5; // half-width at base
                      // Triangle pointing up, centered
    let top_y = cy - h * 0.55;
    let base_y = cy + h * 0.45;
    format!(
        "<polygon points=\"{cx},{top_y} {},{base_y} {},{base_y}\" fill=\"none\" stroke=\"{stroke}\" stroke-width=\"1.5\" class=\"bpmn-ring\"/>",
        cx - hw, cx + hw
    )
}

/// Filled signal triangle for throw/end events.
pub(crate) fn signal_icon_filled(cx: f32, cy: f32, r: f32, stroke: &str) -> String {
    let h = r * 0.7;
    let hw = r * 0.5;
    let top_y = cy - h * 0.55;
    let base_y = cy + h * 0.45;
    format!(
        "<polygon points=\"{cx},{top_y} {},{base_y} {},{base_y}\" fill=\"{stroke}\" stroke=\"{stroke}\" stroke-width=\"1.5\" class=\"bpmn-ring\"/>",
        cx - hw, cx + hw
    )
}

/// Returns the inner icon for an EventSubProcessStartEvent based on its trigger type.
pub(crate) fn esp_trigger_icon(
    trigger: &EventSubProcessTrigger,
    cx: f32,
    cy: f32,
    r: f32,
    stroke: &str,
) -> String {
    match trigger {
        EventSubProcessTrigger::Timer { .. } => timer_icon(cx, cy, r, stroke),
        EventSubProcessTrigger::Message { .. } => envelope_icon(cx, cy, r, stroke),
        EventSubProcessTrigger::Signal { .. } => signal_icon(cx, cy, r, stroke),
        EventSubProcessTrigger::Error { .. } => format!(
            "<path d=\"M{},{} L{},{} L{},{} L{},{} L{},{} L{},{} Z\" fill=\"{stroke}\" stroke=\"none\"/>",
            cx - r * 0.3, cy - r * 0.65,
            cx + r * 0.2, cy - r * 0.05,
            cx - r * 0.05, cy - r * 0.05,
            cx + r * 0.3, cy + r * 0.65,
            cx - r * 0.2, cy + r * 0.05,
            cx + r * 0.05, cy + r * 0.05,
        ),
        EventSubProcessTrigger::Escalation { .. } => format!(
            "<text x=\"{cx}\" y=\"{}\" text-anchor=\"middle\" font-size=\"{}\" fill=\"{stroke}\" class=\"bpmn-ring\">&#x2B06;</text>",
            cy + r * 0.35, r * 1.0
        ),
    }
}

/// Raw SVG three horizontal lines for multi-instance marker at (x, y) top-left.
pub(crate) fn multi_instance_task_icon(x: f32, y: f32, stroke: &str) -> String {
    let w: f32 = 10.0;
    let gap: f32 = 3.0;
    format!(
        "<line x1=\"{x}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{stroke}\" stroke-width=\"1.5\" class=\"bpmn-muted\"/>\n\
         <line x1=\"{x}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{stroke}\" stroke-width=\"1.5\" class=\"bpmn-muted\"/>\n\
         <line x1=\"{x}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{stroke}\" stroke-width=\"1.5\" class=\"bpmn-muted\"/>",
        y, x + w, y,
        y + gap, x + w, y + gap,
        y + gap * 2.0, x + w, y + gap * 2.0
    )
}

/// Raw SVG document/script icon at (x, y) top-left.
/// Draws a small page with a folded corner and two text lines inside.
pub(crate) fn script_task_icon(x: f32, y: f32, stroke: &str) -> String {
    let w: f32 = 10.0;
    let h: f32 = 12.0;
    let fold: f32 = 3.0;
    // Page outline with folded top-right corner
    let path = format!(
        "M{x},{y} L{},{y} L{},{} L{},{} Z",
        x + w - fold, // top-right minus fold
        x + w,
        y + fold, // fold endpoint
        x + w,
        y + h, // bottom-right
    );
    // Two text lines inside the page
    let l1y = y + 5.0;
    let l2y = y + 8.0;
    format!(
        "<path d=\"{path} M{},{y} L{},{}\" fill=\"none\" stroke=\"{stroke}\" stroke-width=\"1\" class=\"bpmn-muted\"/>\n\
         <line x1=\"{}\" y1=\"{l1y}\" x2=\"{}\" y2=\"{l1y}\" stroke=\"{stroke}\" stroke-width=\"0.8\" class=\"bpmn-muted\"/>\n\
         <line x1=\"{}\" y1=\"{l2y}\" x2=\"{}\" y2=\"{l2y}\" stroke=\"{stroke}\" stroke-width=\"0.8\" class=\"bpmn-muted\"/>",
        x + w - fold, x + w - fold, y + fold,   // fold diagonal
        x + 2.0, x + w - 2.0,                    // line 1
        x + 2.0, x + w - 3.0,                    // line 2 (slightly shorter)
    )
}
