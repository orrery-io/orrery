use crate::style::{escape_xml, BPMN_STYLE};
use orrery::model::{FlowElement, ProcessDefinition};

pub(crate) fn render_svg_linear(
    definition: &ProcessDefinition,
    active_ids: &[String],
    failed_ids: &[String],
) -> String {
    use crate::icons::{
        clock_icon, envelope_icon, envelope_task_icon, esp_trigger_icon, gear_task_icon,
        multi_instance_task_icon, script_task_icon, signal_icon, signal_icon_filled, timer_icon,
    };

    let elements = &definition.elements;
    let n = elements.len();

    // Simple left-to-right linear layout
    // Events: circle r=25, tasks: rect 120x50
    // Each slot is 160px wide, 110px tall canvas
    let slot_w = 160usize;
    let elem_h = 50usize;
    let cy = 55usize; // vertical centre
    let total_w = n * slot_w + 40;
    let total_h = 110usize;

    // Build index: element id -> slot index
    let id_to_idx: std::collections::HashMap<&str, usize> = elements
        .iter()
        .enumerate()
        .map(|(i, e)| (e.id(), i))
        .collect();

    let mut shapes = String::new();
    let mut arrows = String::new();

    // Draw elements
    for (i, element) in elements.iter().enumerate() {
        let cx = i * slot_w + slot_w / 2; // centre x of slot
        let eid = element.id();
        let is_active = active_ids.contains(&eid.to_string());
        let is_failed = failed_ids.contains(&eid.to_string());
        let fill = if is_failed {
            "#fee2e2"
        } else if is_active {
            "#f59e0b"
        } else {
            "#e2e8f0"
        };
        let stroke = if is_failed {
            "#ef4444"
        } else if is_active {
            "#d97706"
        } else {
            "#94a3b8"
        };
        let shape_class = if is_failed {
            "bpmn-shape bpmn-failed"
        } else if is_active {
            "bpmn-shape bpmn-active"
        } else {
            "bpmn-shape"
        };

        match element {
            FlowElement::StartEvent(e) => {
                let label = e.name.as_deref().unwrap_or("Start");
                shapes.push_str(&format!(
                    "<circle cx=\"{cx}\" cy=\"{cy}\" r=\"25\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"2\" class=\"{shape_class}\"/>\n\
                     <text x=\"{cx}\" y=\"{}\" text-anchor=\"middle\" font-size=\"10\" fill=\"#1e293b\" class=\"bpmn-text\">{}</text>\n",
                    cy + 38, escape_xml(label)
                ));
            }
            FlowElement::TimerStartEvent(e) => {
                let label = e.name.as_deref().unwrap_or("Start");
                let clock = timer_icon(cx as f32, cy as f32, 25.0, stroke);
                shapes.push_str(&format!(
                    "<circle cx=\"{cx}\" cy=\"{cy}\" r=\"25\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"2\" class=\"{shape_class}\"/>\n\
                     {clock}\n\
                     <text x=\"{cx}\" y=\"{}\" text-anchor=\"middle\" font-size=\"10\" fill=\"#1e293b\" class=\"bpmn-text\">{}</text>\n",
                    cy + 38, escape_xml(label)
                ));
            }
            FlowElement::EndEvent(e) => {
                let label = e.name.as_deref().unwrap_or("End");
                shapes.push_str(&format!(
                    "<circle cx=\"{cx}\" cy=\"{cy}\" r=\"25\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"4\" class=\"{shape_class}\"/>\n\
                     <text x=\"{cx}\" y=\"{}\" text-anchor=\"middle\" font-size=\"10\" fill=\"#1e293b\" class=\"bpmn-text\">{}</text>\n",
                    cy + 38, escape_xml(label)
                ));
            }
            FlowElement::ErrorEndEvent(e) => {
                let label = e.name.as_deref().unwrap_or("Error");
                let bolt = format!(
                    "<path d=\"M{},{} L{},{} L{},{} L{},{} L{},{} L{},{} Z\" fill=\"{stroke}\" stroke=\"none\"/>",
                    cx as f32 - 4.0, cy as f32 - 10.0, cx as f32 + 3.0, cy as f32 - 2.0, cx as f32 - 1.0, cy as f32 - 2.0,
                    cx as f32 + 4.0, cy as f32 + 10.0, cx as f32 - 3.0, cy as f32 + 2.0, cx as f32 + 1.0, cy as f32 + 2.0
                );
                shapes.push_str(&format!(
                    "<circle cx=\"{cx}\" cy=\"{cy}\" r=\"25\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"4\" class=\"{shape_class}\"/>\n\
                     {bolt}\n\
                     <text x=\"{cx}\" y=\"{}\" text-anchor=\"middle\" font-size=\"10\" fill=\"#1e293b\" class=\"bpmn-text\">{}</text>\n",
                    cy + 38, escape_xml(label)
                ));
            }
            FlowElement::TerminateEndEvent(e) => {
                let label = e.name.as_deref().unwrap_or("Terminate");
                shapes.push_str(&format!(
                    "<circle cx=\"{cx}\" cy=\"{cy}\" r=\"25\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"4\" class=\"{shape_class}\"/>\n\
                     <circle cx=\"{cx}\" cy=\"{cy}\" r=\"15\" fill=\"{stroke}\" stroke=\"none\"/>\n\
                     <text x=\"{cx}\" y=\"{}\" text-anchor=\"middle\" font-size=\"10\" fill=\"#1e293b\" class=\"bpmn-text\">{}</text>\n",
                    cy + 38, escape_xml(label)
                ));
            }
            FlowElement::ServiceTask(e) => {
                let label = e.name.as_deref().unwrap_or(eid);
                let rx = cx - 60;
                let ry = cy - elem_h / 2;
                let icon = if e.topic.is_some() {
                    let sz = elem_h as f32 * 0.30;
                    gear_task_icon((rx + 3) as f32, (ry + 2) as f32, sz, "#64748b")
                } else {
                    String::new()
                };
                shapes.push_str(&format!(
                    "<rect x=\"{rx}\" y=\"{ry}\" width=\"120\" height=\"{elem_h}\" rx=\"4\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"2\" class=\"{shape_class}\"/>\n\
                     {icon}\n<text x=\"{cx}\" y=\"{}\" text-anchor=\"middle\" font-size=\"11\" fill=\"#1e293b\" class=\"bpmn-text\">{}</text>\n",
                    cy + 4, escape_xml(label)
                ));
            }
            FlowElement::ExclusiveGateway(e) => {
                let label = e.name.as_deref().unwrap_or(eid);
                let half = 28i32;
                let arm = (half as f32 * 0.35).round() as i32;
                let cx = cx as i32;
                let cy = cy as i32;
                let points = format!(
                    "{},{} {},{} {},{} {},{}",
                    cx,
                    cy - half,
                    cx + half,
                    cy,
                    cx,
                    cy + half,
                    cx - half,
                    cy,
                );
                shapes.push_str(&format!(
                    "<polygon points=\"{points}\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"2\" class=\"{shape_class}\"/>\n\
                     <line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{stroke}\" stroke-width=\"2.5\" class=\"bpmn-ring\"/>\n\
                     <line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{stroke}\" stroke-width=\"2.5\" class=\"bpmn-ring\"/>\n\
                     <text x=\"{cx}\" y=\"{}\" text-anchor=\"middle\" font-size=\"9\" fill=\"#1e293b\" class=\"bpmn-text\">{}</text>\n",
                    cx - arm, cy - arm, cx + arm, cy + arm,
                    cx - arm, cy + arm, cx + arm, cy - arm,
                    cy + 44, escape_xml(label)
                ));
            }
            FlowElement::TimerIntermediateEvent(e) => {
                let label = e.name.as_deref().unwrap_or(eid);
                let r_lin = 25;
                let clock = timer_icon(cx as f32, cy as f32, r_lin as f32, stroke);
                shapes.push_str(&format!(
                    "<circle cx=\"{cx}\" cy=\"{cy}\" r=\"{r_lin}\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"2\" class=\"{shape_class}\"/>\n\
                     <circle cx=\"{cx}\" cy=\"{cy}\" r=\"20\" fill=\"none\" stroke=\"{stroke}\" stroke-width=\"1\" class=\"bpmn-ring\"/>\n\
                     {clock}\n\
                     <text x=\"{cx}\" y=\"{}\" text-anchor=\"middle\" font-size=\"9\" fill=\"#1e293b\" class=\"bpmn-text\">{}</text>\n",
                    cy + 38, escape_xml(label)
                ));
            }
            FlowElement::SubProcess(e) => {
                let label = e.name.as_deref().unwrap_or(eid);
                let rx = cx - 70;
                let ry = cy - elem_h / 2 - 5;
                let h = elem_h + 10;
                shapes.push_str(&format!(
                    "<rect x=\"{rx}\" y=\"{ry}\" width=\"140\" height=\"{h}\" rx=\"6\" \
                     fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"2\" stroke-dasharray=\"4,2\" class=\"{shape_class}\"/>\n\
                     <text x=\"{cx}\" y=\"{}\" text-anchor=\"middle\" font-size=\"11\" fill=\"#1e293b\" class=\"bpmn-text\">{}</text>\n\
                     <text x=\"{cx}\" y=\"{}\" text-anchor=\"middle\" font-size=\"9\" fill=\"#64748b\" class=\"bpmn-muted\">[sub]</text>\n",
                    cy + 4, escape_xml(label), ry + h - 4
                ));
            }
            FlowElement::MultiInstanceTask(e) => {
                let label = e.name.as_deref().unwrap_or(eid);
                let rx = cx - 60;
                let ry = cy - elem_h / 2;
                let mi = multi_instance_task_icon((rx + 4) as f32, (ry + 4) as f32, "#64748b");
                shapes.push_str(&format!(
                    "<rect x=\"{rx}\" y=\"{ry}\" width=\"120\" height=\"{elem_h}\" rx=\"4\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"2\" class=\"{shape_class}\"/>\n\
                     {mi}\n\
                     <text x=\"{cx}\" y=\"{}\" text-anchor=\"middle\" font-size=\"11\" fill=\"#1e293b\" class=\"bpmn-text\">{}</text>\n",
                    cy + 4, escape_xml(label)
                ));
            }
            FlowElement::MessageIntermediateCatchEvent(e) => {
                let label = e.name.as_deref().unwrap_or(eid);
                let r_lin = 25;
                let env = envelope_icon(cx as f32, cy as f32, r_lin as f32, stroke);
                shapes.push_str(&format!(
                    "<circle cx=\"{cx}\" cy=\"{cy}\" r=\"{r_lin}\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"2\" class=\"{shape_class}\"/>\n\
                     <circle cx=\"{cx}\" cy=\"{cy}\" r=\"20\" fill=\"none\" stroke=\"{stroke}\" stroke-width=\"1\" class=\"bpmn-ring\"/>\n\
                     {env}\n\
                     <text x=\"{cx}\" y=\"{}\" text-anchor=\"middle\" font-size=\"9\" fill=\"#1e293b\" class=\"bpmn-text\">{}</text>\n",
                    cy + 38, escape_xml(label)
                ));
            }
            FlowElement::SignalIntermediateCatchEvent(e) => {
                let label = e.name.as_deref().unwrap_or(eid);
                let r_lin = 25;
                let sig = signal_icon(cx as f32, cy as f32, r_lin as f32, stroke);
                shapes.push_str(&format!(
                    "<circle cx=\"{cx}\" cy=\"{cy}\" r=\"{r_lin}\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"2\" class=\"{shape_class}\"/>\n\
                     <circle cx=\"{cx}\" cy=\"{cy}\" r=\"20\" fill=\"none\" stroke=\"{stroke}\" stroke-width=\"1\" class=\"bpmn-ring\"/>\n\
                     {sig}\n\
                     <text x=\"{cx}\" y=\"{}\" text-anchor=\"middle\" font-size=\"9\" fill=\"#1e293b\" class=\"bpmn-text\">{}</text>\n",
                    cy + 38, escape_xml(label)
                ));
            }
            FlowElement::IntermediateThrowEvent(e) => {
                let label = e.name.as_deref().unwrap_or(eid);
                shapes.push_str(&format!(
                    "<circle cx=\"{cx}\" cy=\"{cy}\" r=\"25\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"2\" class=\"{shape_class}\"/>\n\
                     <circle cx=\"{cx}\" cy=\"{cy}\" r=\"20\" fill=\"{stroke}\" fill-opacity=\"0.15\" stroke=\"{stroke}\" stroke-width=\"1\" class=\"bpmn-ring\"/>\n\
                     <text x=\"{cx}\" y=\"{}\" text-anchor=\"middle\" font-size=\"9\" fill=\"#1e293b\" class=\"bpmn-text\">{}</text>\n",
                    cy + 38, escape_xml(label)
                ));
            }
            FlowElement::BoundaryEvent(e) => {
                let label = e.name.as_deref().unwrap_or("⚠");
                // Render as a small dashed circle
                shapes.push_str(&format!(
                    "<circle cx=\"{}\" cy=\"{}\" r=\"14\" fill=\"{fill}\" stroke=\"{stroke}\" \
                     stroke-width=\"2\" stroke-dasharray=\"3,2\" class=\"{shape_class}\"/>\n\
                     <text x=\"{}\" y=\"{}\" text-anchor=\"middle\" font-size=\"9\" fill=\"#1e293b\" class=\"bpmn-text\">{}</text>\n",
                    cx + 50, cy + 30, cx + 50, cy + 34, escape_xml(label)
                ));
            }
            FlowElement::MessageStartEvent(e) => {
                let label = e.name.as_deref().unwrap_or("Start");
                let env = envelope_icon(cx as f32, cy as f32, 25.0, stroke);
                shapes.push_str(&format!(
                    "<circle cx=\"{cx}\" cy=\"{cy}\" r=\"25\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"2\" class=\"{shape_class}\"/>\n\
                     {env}\n\
                     <text x=\"{cx}\" y=\"{}\" text-anchor=\"middle\" font-size=\"9\" fill=\"#1e293b\" class=\"bpmn-text\">{}</text>\n",
                    cy + 38, escape_xml(label)
                ));
            }
            FlowElement::SignalStartEvent(e) => {
                let label = e.name.as_deref().unwrap_or("Start");
                let sig = signal_icon(cx as f32, cy as f32, 25.0, stroke);
                shapes.push_str(&format!(
                    "<circle cx=\"{cx}\" cy=\"{cy}\" r=\"25\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"2\" class=\"{shape_class}\"/>\n\
                     {sig}\n\
                     <text x=\"{cx}\" y=\"{}\" text-anchor=\"middle\" font-size=\"9\" fill=\"#1e293b\" class=\"bpmn-text\">{}</text>\n",
                    cy + 38, escape_xml(label)
                ));
            }
            FlowElement::SignalIntermediateThrowEvent(e) => {
                let label = e.name.as_deref().unwrap_or(eid);
                let sig = signal_icon_filled(cx as f32, cy as f32, 25.0, stroke);
                shapes.push_str(&format!(
                    "<circle cx=\"{cx}\" cy=\"{cy}\" r=\"25\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"2\" class=\"{shape_class}\"/>\n\
                     <circle cx=\"{cx}\" cy=\"{cy}\" r=\"20\" fill=\"none\" stroke=\"{stroke}\" stroke-width=\"1\" class=\"bpmn-ring\"/>\n\
                     {sig}\n\
                     <text x=\"{cx}\" y=\"{}\" text-anchor=\"middle\" font-size=\"9\" fill=\"#1e293b\" class=\"bpmn-text\">{}</text>\n",
                    cy + 38, escape_xml(label)
                ));
            }
            FlowElement::SignalEndEvent(e) => {
                let label = e.name.as_deref().unwrap_or("End");
                let sig = signal_icon_filled(cx as f32, cy as f32, 25.0, stroke);
                shapes.push_str(&format!(
                    "<circle cx=\"{cx}\" cy=\"{cy}\" r=\"25\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"3\" class=\"{shape_class}\"/>\n\
                     {sig}\n\
                     <text x=\"{cx}\" y=\"{}\" text-anchor=\"middle\" font-size=\"9\" fill=\"#1e293b\" class=\"bpmn-text\">{}</text>\n",
                    cy + 38, escape_xml(label)
                ));
            }
            FlowElement::MessageBoundaryEvent(e) => {
                let label = e.name.as_deref().unwrap_or("Msg");
                let bcx = (cx + 50) as f32;
                let bcy = (cy + 30) as f32;
                let env = envelope_icon(bcx, bcy, 14.0, stroke);
                shapes.push_str(&format!(
                    "<circle cx=\"{}\" cy=\"{}\" r=\"14\" fill=\"{fill}\" stroke=\"{stroke}\" \
                     stroke-width=\"2\" stroke-dasharray=\"3,2\" class=\"{shape_class}\"/>\n\
                     {env}\n\
                     <text x=\"{}\" y=\"{}\" text-anchor=\"middle\" font-size=\"9\" fill=\"#1e293b\" class=\"bpmn-text\">{}</text>\n",
                    cx + 50, cy + 30,
                    cx + 50, cy + 50, escape_xml(label)
                ));
            }
            FlowElement::TimerBoundaryEvent(e) => {
                let label = e.name.as_deref().unwrap_or("Timer");
                let bcx = (cx + 50) as f32;
                let bcy = (cy + 30) as f32;
                let dash = if e.is_interrupting {
                    ""
                } else {
                    " stroke-dasharray=\"3,2\""
                };
                let clk = clock_icon(bcx, bcy, 14.0, stroke);
                shapes.push_str(&format!(
                    "<circle cx=\"{}\" cy=\"{}\" r=\"14\" fill=\"{fill}\" stroke=\"{stroke}\" \
                     stroke-width=\"2\"{dash} class=\"{shape_class}\"/>\n\
                     {clk}\n\
                     <text x=\"{}\" y=\"{}\" text-anchor=\"middle\" font-size=\"9\" fill=\"#1e293b\" class=\"bpmn-text\">{}</text>\n",
                    cx + 50, cy + 30,
                    cx + 50, cy + 50, escape_xml(label)
                ));
            }
            FlowElement::SignalBoundaryEvent(e) => {
                let label = e.name.as_deref().unwrap_or("Signal");
                let bcx = (cx + 50) as f32;
                let bcy = (cy + 30) as f32;
                let dash = if e.is_interrupting {
                    ""
                } else {
                    " stroke-dasharray=\"3,2\""
                };
                let sig = signal_icon(bcx, bcy, 14.0, stroke);
                shapes.push_str(&format!(
                    "<circle cx=\"{}\" cy=\"{}\" r=\"14\" fill=\"{fill}\" stroke=\"{stroke}\" \
                     stroke-width=\"2\"{dash} class=\"{shape_class}\"/>\n\
                     {sig}\n\
                     <text x=\"{}\" y=\"{}\" text-anchor=\"middle\" font-size=\"9\" fill=\"#1e293b\" class=\"bpmn-text\">{}</text>\n",
                    cx + 50, cy + 30,
                    cx + 50, cy + 50, escape_xml(label)
                ));
            }
            FlowElement::MessageEndEvent(e) => {
                let label = e.name.as_deref().unwrap_or("End");
                let env = envelope_icon(cx as f32, cy as f32, 25.0, stroke);
                shapes.push_str(&format!(
                    "<circle cx=\"{cx}\" cy=\"{cy}\" r=\"25\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"4\" class=\"{shape_class}\"/>\n\
                     {env}\n\
                     <text x=\"{cx}\" y=\"{}\" text-anchor=\"middle\" font-size=\"9\" fill=\"#1e293b\" class=\"bpmn-text\">{}</text>\n",
                    cy + 38, escape_xml(label)
                ));
            }
            FlowElement::MessageIntermediateThrowEvent(e) => {
                let label = e.name.as_deref().unwrap_or(eid);
                let env = envelope_icon(cx as f32, cy as f32, 25.0, stroke);
                shapes.push_str(&format!(
                    "<circle cx=\"{cx}\" cy=\"{cy}\" r=\"25\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"2\" class=\"{shape_class}\"/>\n\
                     <circle cx=\"{cx}\" cy=\"{cy}\" r=\"20\" fill=\"{stroke}\" fill-opacity=\"0.15\" stroke=\"{stroke}\" stroke-width=\"1\" class=\"bpmn-ring\"/>\n\
                     {env}\n\
                     <text x=\"{cx}\" y=\"{}\" text-anchor=\"middle\" font-size=\"9\" fill=\"#1e293b\" class=\"bpmn-text\">{}</text>\n",
                    cy + 38, escape_xml(label)
                ));
            }
            FlowElement::ReceiveTask(e) => {
                let label = e.name.as_deref().unwrap_or(eid);
                let rx = cx - 60;
                let ry = cy - elem_h / 2;
                let env = envelope_task_icon((rx + 4) as f32, (ry + 4) as f32, "#64748b");
                shapes.push_str(&format!(
                    "<rect x=\"{rx}\" y=\"{ry}\" width=\"120\" height=\"{elem_h}\" rx=\"4\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"2\" class=\"{shape_class}\"/>\n\
                     {env}\n\
                     <text x=\"{cx}\" y=\"{}\" text-anchor=\"middle\" font-size=\"11\" fill=\"#1e293b\" class=\"bpmn-text\">{}</text>\n",
                    cy + 4, escape_xml(label)
                ));
            }
            FlowElement::ScriptTask(e) => {
                let label = e.name.as_deref().unwrap_or(eid);
                let rx = cx - 60;
                let ry = cy - elem_h / 2;
                let icon = script_task_icon((rx + 4) as f32, (ry + 4) as f32, "#64748b");
                shapes.push_str(&format!(
                    "<rect x=\"{rx}\" y=\"{ry}\" width=\"120\" height=\"{elem_h}\" rx=\"4\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"2\" class=\"{shape_class}\"/>\n\
                     {icon}\n\
                     <text x=\"{cx}\" y=\"{}\" text-anchor=\"middle\" font-size=\"11\" fill=\"#1e293b\" class=\"bpmn-text\">{}</text>\n",
                    cy + 4, escape_xml(label)
                ));
            }
            FlowElement::ParallelGateway(e) => {
                let label = e.name.as_deref().unwrap_or(eid);
                let half = 28i32;
                let arm = (half as f32 * 0.35).round() as i32;
                let cx = cx as i32;
                let cy = cy as i32;
                let points = format!(
                    "{},{} {},{} {},{} {},{}",
                    cx,
                    cy - half,
                    cx + half,
                    cy,
                    cx,
                    cy + half,
                    cx - half,
                    cy,
                );
                shapes.push_str(&format!(
                    "<polygon points=\"{points}\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"2\" class=\"{shape_class}\"/>\n\
                     <line x1=\"{cx}\" y1=\"{}\" x2=\"{cx}\" y2=\"{}\" stroke=\"{stroke}\" stroke-width=\"2.5\" class=\"bpmn-ring\"/>\n\
                     <line x1=\"{}\" y1=\"{cy}\" x2=\"{}\" y2=\"{cy}\" stroke=\"{stroke}\" stroke-width=\"2.5\" class=\"bpmn-ring\"/>\n\
                     <text x=\"{cx}\" y=\"{}\" text-anchor=\"middle\" font-size=\"9\" fill=\"#1e293b\" class=\"bpmn-text\">{}</text>\n",
                    cy - arm, cy + arm,
                    cx - arm, cx + arm,
                    cy + 44, escape_xml(label)
                ));
            }
            FlowElement::InclusiveGateway(e) => {
                let label = e.name.as_deref().unwrap_or(eid);
                let half = 28i32;
                let cx = cx as i32;
                let cy = cy as i32;
                let points = format!(
                    "{},{} {},{} {},{} {},{}",
                    cx,
                    cy - half,
                    cx + half,
                    cy,
                    cx,
                    cy + half,
                    cx - half,
                    cy,
                );
                shapes.push_str(&format!(
                    "<polygon points=\"{points}\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"2\" class=\"{shape_class}\"/>\n\
                     <circle cx=\"{cx}\" cy=\"{cy}\" r=\"12\" fill=\"none\" stroke=\"{stroke}\" stroke-width=\"2\" class=\"bpmn-ring\"/>\n\
                     <text x=\"{cx}\" y=\"{}\" text-anchor=\"middle\" font-size=\"9\" fill=\"#1e293b\" class=\"bpmn-text\">{}</text>\n",
                    cy + 44, escape_xml(label)
                ));
            }
            FlowElement::EventBasedGateway(e) => {
                let label = e.name.as_deref().unwrap_or(eid);
                let half = 28i32;
                let cx = cx as i32;
                let cy = cy as i32;
                let points = format!(
                    "{},{} {},{} {},{} {},{}",
                    cx,
                    cy - half,
                    cx + half,
                    cy,
                    cx,
                    cy + half,
                    cx - half,
                    cy,
                );
                let r_outer = 15;
                let r_inner = 11;
                let pent = (0..5)
                    .map(|i| {
                        let angle = std::f64::consts::FRAC_PI_2
                            + (i as f64) * 2.0 * std::f64::consts::PI / 5.0;
                        let px = (cx as f64) - (r_inner as f64) * angle.cos();
                        let py = (cy as f64) - (r_inner as f64) * angle.sin();
                        format!("{px:.1},{py:.1}")
                    })
                    .collect::<Vec<_>>()
                    .join(" ");
                shapes.push_str(&format!(
                    "<polygon points=\"{points}\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"2\" class=\"{shape_class}\"/>\n\
                     <circle cx=\"{cx}\" cy=\"{cy}\" r=\"{r_outer}\" fill=\"none\" stroke=\"{stroke}\" stroke-width=\"1.5\" class=\"bpmn-ring\"/>\n\
                     <circle cx=\"{cx}\" cy=\"{cy}\" r=\"{r_inner}\" fill=\"none\" stroke=\"{stroke}\" stroke-width=\"1.5\" class=\"bpmn-ring\"/>\n\
                     <polygon points=\"{pent}\" fill=\"none\" stroke=\"{stroke}\" stroke-width=\"1.5\" class=\"bpmn-ring\"/>\n\
                     <text x=\"{cx}\" y=\"{}\" text-anchor=\"middle\" font-size=\"9\" fill=\"#1e293b\" class=\"bpmn-text\">{}</text>\n",
                    cy + 44, escape_xml(label)
                ));
            }
            FlowElement::EscalationIntermediateThrowEvent(e) => {
                let label = e.name.as_deref().unwrap_or(eid);
                shapes.push_str(&format!(
                    "<circle cx=\"{cx}\" cy=\"{cy}\" r=\"25\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"2\" class=\"{shape_class}\"/>\n\
                     <circle cx=\"{cx}\" cy=\"{cy}\" r=\"20\" fill=\"{stroke}\" fill-opacity=\"0.15\" stroke=\"{stroke}\" stroke-width=\"1\" class=\"bpmn-ring\"/>\n\
                     <text x=\"{cx}\" y=\"{}\" text-anchor=\"middle\" font-size=\"14\" fill=\"{stroke}\" class=\"bpmn-ring\">&#x2B06;</text>\n\
                     <text x=\"{cx}\" y=\"{}\" text-anchor=\"middle\" font-size=\"9\" fill=\"#1e293b\" class=\"bpmn-text\">{}</text>\n",
                    cy + 5,
                    cy + 38, escape_xml(label)
                ));
            }
            FlowElement::EscalationEndEvent(e) => {
                let label = e.name.as_deref().unwrap_or("Escalation");
                shapes.push_str(&format!(
                    "<circle cx=\"{cx}\" cy=\"{cy}\" r=\"25\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"4\" class=\"{shape_class}\"/>\n\
                     <text x=\"{cx}\" y=\"{}\" text-anchor=\"middle\" font-size=\"14\" fill=\"{stroke}\" class=\"bpmn-ring\">&#x2B06;</text>\n\
                     <text x=\"{cx}\" y=\"{}\" text-anchor=\"middle\" font-size=\"9\" fill=\"#1e293b\" class=\"bpmn-text\">{}</text>\n",
                    cy + 5,
                    cy + 38, escape_xml(label)
                ));
            }
            FlowElement::EscalationBoundaryEvent(e) => {
                let label = e.name.as_deref().unwrap_or("Escalation");
                let dash = if e.is_interrupting {
                    ""
                } else {
                    " stroke-dasharray=\"3,2\""
                };
                shapes.push_str(&format!(
                    "<circle cx=\"{}\" cy=\"{}\" r=\"14\" fill=\"{fill}\" stroke=\"{stroke}\" \
                     stroke-width=\"2\"{dash} class=\"{shape_class}\"/>\n\
                     <text x=\"{}\" y=\"{}\" text-anchor=\"middle\" font-size=\"10\" fill=\"{stroke}\" class=\"bpmn-ring\">&#x2B06;</text>\n\
                     <text x=\"{}\" y=\"{}\" text-anchor=\"middle\" font-size=\"9\" fill=\"#1e293b\" class=\"bpmn-text\">{}</text>\n",
                    cx + 50, cy + 30,
                    cx + 50, cy + 34,
                    cx + 50, cy + 50, escape_xml(label)
                ));
            }
            FlowElement::LinkIntermediateThrowEvent(e) => {
                let label = e.name.as_deref().unwrap_or(eid);
                shapes.push_str(&format!(
                    "<circle cx=\"{cx}\" cy=\"{cy}\" r=\"25\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"2\" class=\"{shape_class}\"/>\n\
                     <circle cx=\"{cx}\" cy=\"{cy}\" r=\"20\" fill=\"{stroke}\" fill-opacity=\"0.15\" stroke=\"{stroke}\" stroke-width=\"1\" class=\"bpmn-ring\"/>\n\
                     <text x=\"{cx}\" y=\"{}\" text-anchor=\"middle\" font-size=\"14\" fill=\"{stroke}\" class=\"bpmn-ring\">&#x27A1;</text>\n\
                     <text x=\"{cx}\" y=\"{}\" text-anchor=\"middle\" font-size=\"9\" fill=\"#1e293b\" class=\"bpmn-text\">{}</text>\n",
                    cy + 5,
                    cy + 38, escape_xml(label)
                ));
            }
            FlowElement::LinkIntermediateCatchEvent(e) => {
                let label = e.name.as_deref().unwrap_or(eid);
                shapes.push_str(&format!(
                    "<circle cx=\"{cx}\" cy=\"{cy}\" r=\"25\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"2\" class=\"{shape_class}\"/>\n\
                     <circle cx=\"{cx}\" cy=\"{cy}\" r=\"20\" fill=\"none\" stroke=\"{stroke}\" stroke-width=\"1\" class=\"bpmn-ring\"/>\n\
                     <text x=\"{cx}\" y=\"{}\" text-anchor=\"middle\" font-size=\"14\" fill=\"{stroke}\" class=\"bpmn-ring\">&#x27A1;</text>\n\
                     <text x=\"{cx}\" y=\"{}\" text-anchor=\"middle\" font-size=\"9\" fill=\"#1e293b\" class=\"bpmn-text\">{}</text>\n",
                    cy + 5,
                    cy + 38, escape_xml(label)
                ));
            }
            FlowElement::EventSubProcess(e) => {
                let label = e.name.as_deref().unwrap_or("Event Sub-Process");
                shapes.push_str(&format!(
                    "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"4\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"2\" stroke-dasharray=\"4 2\" class=\"{shape_class}\"/>\n\
                     <text x=\"{cx}\" y=\"{}\" text-anchor=\"middle\" font-size=\"11\" fill=\"#1e293b\" class=\"bpmn-text\">{}</text>\n",
                    cx - 70, cy - 40, 140, 80, cy + 5, escape_xml(label)
                ));
            }
            FlowElement::EventSubProcessStartEvent(e) => {
                let label = e.name.as_deref().unwrap_or("");
                let r = 25.0_f32;
                let icon = esp_trigger_icon(&e.trigger, cx as f32, cy as f32, r, stroke);
                shapes.push_str(&format!(
                    "<circle cx=\"{cx}\" cy=\"{cy}\" r=\"25\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"2\" stroke-dasharray=\"2 1\" class=\"{shape_class}\"/>\n\
                     {icon}\n\
                     <text x=\"{cx}\" y=\"{}\" text-anchor=\"middle\" font-size=\"9\" fill=\"#1e293b\" class=\"bpmn-text\">{}</text>\n",
                    cy + 38, escape_xml(label)
                ));
            }
        }
    }

    // Draw sequence flow arrows
    for flow in &definition.sequence_flows {
        let src_idx = id_to_idx.get(flow.source_ref.as_str());
        let tgt_idx = id_to_idx.get(flow.target_ref.as_str());
        if let (Some(&si), Some(&ti)) = (src_idx, tgt_idx) {
            let src_elem = &elements[si];
            let tgt_elem = &elements[ti];

            // right edge of source
            let x1 = match src_elem {
                FlowElement::StartEvent(_)
                | FlowElement::TimerStartEvent(_)
                | FlowElement::EndEvent(_)
                | FlowElement::ErrorEndEvent(_)
                | FlowElement::TimerIntermediateEvent(_)
                | FlowElement::MessageIntermediateCatchEvent(_)
                | FlowElement::SignalIntermediateCatchEvent(_)
                | FlowElement::IntermediateThrowEvent(_)
                | FlowElement::BoundaryEvent(_)
                | FlowElement::MessageStartEvent(_)
                | FlowElement::SignalStartEvent(_)
                | FlowElement::SignalIntermediateThrowEvent(_)
                | FlowElement::SignalEndEvent(_)
                | FlowElement::SignalBoundaryEvent(_)
                | FlowElement::MessageBoundaryEvent(_)
                | FlowElement::TimerBoundaryEvent(_)
                | FlowElement::MessageEndEvent(_)
                | FlowElement::MessageIntermediateThrowEvent(_)
                | FlowElement::TerminateEndEvent(_)
                | FlowElement::EscalationIntermediateThrowEvent(_)
                | FlowElement::EscalationEndEvent(_)
                | FlowElement::EscalationBoundaryEvent(_)
                | FlowElement::LinkIntermediateThrowEvent(_)
                | FlowElement::LinkIntermediateCatchEvent(_)
                | FlowElement::EventSubProcessStartEvent(_) => si * slot_w + slot_w / 2 + 25,
                FlowElement::ServiceTask(_)
                | FlowElement::MultiInstanceTask(_)
                | FlowElement::ReceiveTask(_)
                | FlowElement::ScriptTask(_) => si * slot_w + slot_w / 2 + 60,
                FlowElement::SubProcess(_) | FlowElement::EventSubProcess(_) => {
                    si * slot_w + slot_w / 2 + 70
                }
                FlowElement::ExclusiveGateway(_)
                | FlowElement::ParallelGateway(_)
                | FlowElement::InclusiveGateway(_)
                | FlowElement::EventBasedGateway(_) => si * slot_w + slot_w / 2 + 28,
            };
            // left edge of target
            let x2 = match tgt_elem {
                FlowElement::StartEvent(_)
                | FlowElement::TimerStartEvent(_)
                | FlowElement::EndEvent(_)
                | FlowElement::ErrorEndEvent(_)
                | FlowElement::TimerIntermediateEvent(_)
                | FlowElement::MessageIntermediateCatchEvent(_)
                | FlowElement::SignalIntermediateCatchEvent(_)
                | FlowElement::IntermediateThrowEvent(_)
                | FlowElement::BoundaryEvent(_)
                | FlowElement::MessageStartEvent(_)
                | FlowElement::SignalStartEvent(_)
                | FlowElement::SignalIntermediateThrowEvent(_)
                | FlowElement::SignalEndEvent(_)
                | FlowElement::SignalBoundaryEvent(_)
                | FlowElement::MessageBoundaryEvent(_)
                | FlowElement::TimerBoundaryEvent(_)
                | FlowElement::MessageEndEvent(_)
                | FlowElement::MessageIntermediateThrowEvent(_)
                | FlowElement::TerminateEndEvent(_)
                | FlowElement::EscalationIntermediateThrowEvent(_)
                | FlowElement::EscalationEndEvent(_)
                | FlowElement::EscalationBoundaryEvent(_)
                | FlowElement::LinkIntermediateThrowEvent(_)
                | FlowElement::LinkIntermediateCatchEvent(_)
                | FlowElement::EventSubProcessStartEvent(_) => ti * slot_w + slot_w / 2 - 25,
                FlowElement::ServiceTask(_)
                | FlowElement::MultiInstanceTask(_)
                | FlowElement::ReceiveTask(_)
                | FlowElement::ScriptTask(_) => ti * slot_w + slot_w / 2 - 60,
                FlowElement::SubProcess(_) | FlowElement::EventSubProcess(_) => {
                    ti * slot_w + slot_w / 2 - 70
                }
                FlowElement::ExclusiveGateway(_)
                | FlowElement::ParallelGateway(_)
                | FlowElement::InclusiveGateway(_)
                | FlowElement::EventBasedGateway(_) => ti * slot_w + slot_w / 2 - 28,
            };

            arrows.push_str(&format!(
                "<line x1=\"{x1}\" y1=\"{cy}\" x2=\"{x2}\" y2=\"{cy}\" \
                 stroke=\"#94a3b8\" stroke-width=\"1.5\" marker-end=\"url(#arrow)\" class=\"bpmn-flow\"/>\n"
            ));
        }
    }

    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{total_w}" height="{total_h}" viewBox="0 0 {total_w} {total_h}">
  <defs>
    {BPMN_STYLE}
    <marker id="arrow" markerWidth="8" markerHeight="8" refX="6" refY="3" orient="auto">
      <path d="M0,0 L0,6 L8,3 z" fill="#94a3b8" class="bpmn-marker"/>
    </marker>
  </defs>
  {arrows}{shapes}</svg>"##
    )
}
