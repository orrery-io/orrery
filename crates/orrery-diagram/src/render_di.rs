use crate::elements::{events, flows, gateways, subprocesses, tasks};
use crate::render_linear::render_svg_linear;
use crate::style::BPMN_STYLE;
use orrery::diagram::DiagramLayout;
use orrery::model::{FlowElement, ProcessDefinition};
use std::collections::{HashMap, HashSet};

/// Compute the SVG viewport `(vx, vy, vw, vh)` from the layout bounding box.
/// Returns `None` if the layout is empty (no shapes or waypoints).
fn compute_viewport(layout: &DiagramLayout) -> Option<(f32, f32, f32, f32)> {
    let pad = 28.0_f32;
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);

    for b in layout.shapes.values() {
        min_x = min_x.min(b.x);
        min_y = min_y.min(b.y);
        max_x = max_x.max(b.x + b.width);
        max_y = max_y.max(b.y + b.height);
    }
    // Include edge waypoints so arcs that extend beyond shape bounds are not clipped
    for waypoints in layout.edges.values() {
        for p in waypoints {
            min_x = min_x.min(p.x);
            min_y = min_y.min(p.y);
            max_x = max_x.max(p.x);
            max_y = max_y.max(p.y);
        }
    }

    if min_x == f32::MAX {
        return None;
    }

    let vx = min_x - pad;
    let vy = min_y - pad;
    let vw = (max_x - min_x) + pad * 2.0;
    let vh = (max_y - min_y) + pad * 2.0 + 20.0; // extra for labels below events
    Some((vx, vy, vw, vh))
}

/// Render all text annotations and association lines for the DI path.
/// Called before the main element loop so annotations appear behind process shapes.
fn render_annotations(definition: &ProcessDefinition, layout: &DiagramLayout) -> String {
    use crate::style::escape_xml;
    const ARM: f32 = 10.0;

    let mut out = String::new();

    // Association lines — dashed polylines, no arrowhead
    for assoc in &definition.associations {
        let Some(waypoints) = layout.edges.get(&assoc.id) else {
            continue;
        };
        if waypoints.is_empty() {
            continue;
        }
        let pts = waypoints
            .iter()
            .map(|p| format!("{},{}", p.x, p.y))
            .collect::<Vec<_>>()
            .join(" ");
        out.push_str(&format!(
            "<polyline points=\"{pts}\" fill=\"none\" stroke=\"#94a3b8\" stroke-width=\"1.5\" stroke-dasharray=\"4,3\"/>\n"
        ));
    }

    // Annotation shapes — open bracket + text
    for ann in &definition.annotations {
        let Some(b) = layout.shapes.get(&ann.id) else {
            continue;
        };
        let x = b.x;
        let y = b.y;
        let h = b.height;

        // Open bracket path: vertical bar on left, short arms extending right
        out.push_str(&format!(
            "<path d=\"M{},{} L{},{} L{},{} L{},{}\" fill=\"none\" stroke=\"#94a3b8\" stroke-width=\"1.5\"/>\n",
            x + ARM, y,   // top arm start
            x, y,          // top-left corner
            x, y + h,      // bottom-left corner
            x + ARM, y + h // bottom arm end
        ));

        // Text content — skip entirely if empty
        if ann.text.is_empty() {
            continue;
        }

        // Greedy word wrap: approximate char width = 6.5px at font-size 11
        let max_chars = ((b.width - 12.0) / 6.5).max(1.0) as usize;
        let lines = wrap_words(&ann.text, max_chars);
        let text_x = b.x + 12.0; // ARM + 2px padding
        let text_y = b.y + 14.0; // first line baseline

        out.push_str(&format!(
            "<text x=\"{text_x}\" y=\"{text_y}\" font-size=\"11\" fill=\"#475569\" class=\"bpmn-text\">\n"
        ));
        for (i, line) in lines.iter().enumerate() {
            let dy = if i == 0 {
                "0".to_string()
            } else {
                "14".to_string()
            };
            out.push_str(&format!(
                "  <tspan x=\"{text_x}\" dy=\"{dy}\">{}</tspan>\n",
                escape_xml(line)
            ));
        }
        out.push_str("</text>\n");
    }

    out
}

/// Greedy word-wrap: splits `text` on `\n` first (explicit line breaks), then on
/// whitespace within each line. Each line accumulates words until the next word
/// would exceed `max_chars`. A single word longer than `max_chars` occupies its
/// own line unbroken.
fn wrap_words(text: &str, max_chars: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();

    for segment in text.split('\n') {
        // Each \n starts a new line
        if !current.trim().is_empty() {
            lines.push(current.trim().to_string());
            current = String::new();
        } else if !lines.is_empty() {
            // Empty segment from consecutive \n — emit a blank line
            lines.push(String::new());
        }
        for word in segment.split_whitespace() {
            if current.is_empty() {
                current.push_str(word);
            } else if current.len() + 1 + word.len() <= max_chars {
                current.push(' ');
                current.push_str(word);
            } else {
                lines.push(current.trim().to_string());
                current = word.to_string();
            }
        }
    }
    if !current.trim().is_empty() {
        lines.push(current.trim().to_string());
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

pub(crate) fn render_svg_di(
    definition: &ProcessDefinition,
    layout: &DiagramLayout,
    active_ids: &[String],
    failed_ids: &[String],
) -> String {
    let Some((vx, vy, vw, vh)) = compute_viewport(layout) else {
        return render_svg_linear(definition, active_ids, failed_ids);
    };

    let mut shapes = String::new();
    let mut badges = String::new();

    let arrows = flows::render_flows(&definition.sequence_flows, &layout.edges);
    let annotation_layer = render_annotations(definition, layout);

    for element in &definition.elements {
        let eid = element.id();
        let Some(b) = layout.shapes.get(eid) else {
            continue;
        };
        let is_active = active_ids.contains(&eid.to_string());
        let is_failed = failed_ids.contains(&eid.to_string());
        let fill = if is_failed {
            "#fee2e2"
        } else if is_active {
            "#fef3c7"
        } else {
            "#f8fafc"
        };
        let stroke = if is_failed {
            "#ef4444"
        } else if is_active {
            "#d97706"
        } else {
            "#94a3b8"
        };
        let sw = if is_failed || is_active { "2.5" } else { "1.5" };
        let shape_class = if is_failed {
            "bpmn-shape bpmn-failed"
        } else if is_active {
            "bpmn-shape bpmn-active"
        } else {
            "bpmn-shape"
        };

        let cx = b.x + b.width / 2.0;
        let cy = b.y + b.height / 2.0;

        let shape_svg = match element {
            FlowElement::StartEvent(e) => events::render_start_event(
                e.name.as_deref(),
                eid,
                b,
                cx,
                cy,
                fill,
                stroke,
                sw,
                shape_class,
            ),
            FlowElement::TimerStartEvent(e) => events::render_timer_start_event(
                e.name.as_deref(),
                eid,
                b,
                cx,
                cy,
                fill,
                stroke,
                sw,
                shape_class,
            ),
            FlowElement::MessageStartEvent(e) => events::render_message_start_event(
                e.name.as_deref(),
                eid,
                b,
                cx,
                cy,
                fill,
                stroke,
                sw,
                shape_class,
            ),
            FlowElement::SignalStartEvent(e) => events::render_signal_start_event(
                e.name.as_deref(),
                eid,
                b,
                cx,
                cy,
                fill,
                stroke,
                sw,
                shape_class,
            ),
            FlowElement::EndEvent(e) => events::render_end_event(
                e.name.as_deref(),
                eid,
                b,
                cx,
                cy,
                fill,
                stroke,
                sw,
                shape_class,
            ),
            FlowElement::ErrorEndEvent(e) => events::render_error_end_event(
                e.name.as_deref(),
                eid,
                b,
                cx,
                cy,
                fill,
                stroke,
                sw,
                shape_class,
            ),
            FlowElement::TerminateEndEvent(e) => events::render_terminate_end_event(
                e.name.as_deref(),
                b,
                cx,
                cy,
                fill,
                stroke,
                sw,
                shape_class,
            ),
            FlowElement::MessageEndEvent(e) => events::render_message_end_event(
                e.name.as_deref(),
                eid,
                b,
                cx,
                cy,
                fill,
                stroke,
                sw,
                shape_class,
            ),
            FlowElement::SignalEndEvent(e) => events::render_signal_end_event(
                e.name.as_deref(),
                eid,
                b,
                cx,
                cy,
                fill,
                stroke,
                sw,
                shape_class,
            ),
            FlowElement::EscalationEndEvent(e) => events::render_escalation_end_event(
                e.name.as_deref(),
                eid,
                b,
                cx,
                cy,
                fill,
                stroke,
                sw,
                shape_class,
            ),
            FlowElement::TimerIntermediateEvent(e) => events::render_timer_intermediate_event(
                e.name.as_deref(),
                eid,
                b,
                cx,
                cy,
                fill,
                stroke,
                sw,
                shape_class,
            ),
            FlowElement::MessageIntermediateCatchEvent(e) => {
                events::render_message_intermediate_catch_event(
                    e.name.as_deref(),
                    eid,
                    b,
                    cx,
                    cy,
                    fill,
                    stroke,
                    sw,
                    shape_class,
                )
            }
            FlowElement::SignalIntermediateCatchEvent(e) => {
                events::render_signal_intermediate_catch_event(
                    e.name.as_deref(),
                    eid,
                    b,
                    cx,
                    cy,
                    fill,
                    stroke,
                    sw,
                    shape_class,
                )
            }
            FlowElement::IntermediateThrowEvent(e) => events::render_intermediate_throw_event(
                e.name.as_deref(),
                eid,
                b,
                cx,
                cy,
                fill,
                stroke,
                sw,
                shape_class,
            ),
            FlowElement::MessageIntermediateThrowEvent(e) => {
                events::render_message_intermediate_throw_event(
                    e.name.as_deref(),
                    eid,
                    b,
                    cx,
                    cy,
                    fill,
                    stroke,
                    sw,
                    shape_class,
                )
            }
            FlowElement::SignalIntermediateThrowEvent(e) => {
                events::render_signal_intermediate_throw_event(
                    e.name.as_deref(),
                    eid,
                    b,
                    cx,
                    cy,
                    fill,
                    stroke,
                    sw,
                    shape_class,
                )
            }
            FlowElement::EscalationIntermediateThrowEvent(e) => {
                events::render_escalation_intermediate_throw_event(
                    e.name.as_deref(),
                    eid,
                    b,
                    cx,
                    cy,
                    fill,
                    stroke,
                    sw,
                    shape_class,
                )
            }
            FlowElement::LinkIntermediateThrowEvent(e) => {
                events::render_link_intermediate_throw_event(
                    e.name.as_deref(),
                    eid,
                    b,
                    cx,
                    cy,
                    fill,
                    stroke,
                    sw,
                    shape_class,
                )
            }
            FlowElement::LinkIntermediateCatchEvent(e) => {
                events::render_link_intermediate_catch_event(
                    e.name.as_deref(),
                    eid,
                    b,
                    cx,
                    cy,
                    fill,
                    stroke,
                    sw,
                    shape_class,
                )
            }
            FlowElement::BoundaryEvent(e) => events::render_boundary_event(
                e.name.as_deref(),
                b,
                cx,
                cy,
                fill,
                stroke,
                sw,
                shape_class,
            ),
            FlowElement::MessageBoundaryEvent(e) => events::render_message_boundary_event(
                e.name.as_deref(),
                eid,
                b,
                cx,
                cy,
                fill,
                stroke,
                sw,
                shape_class,
            ),
            FlowElement::TimerBoundaryEvent(e) => events::render_timer_boundary_event(
                e.name.as_deref(),
                eid,
                b,
                cx,
                cy,
                fill,
                stroke,
                sw,
                shape_class,
                e.is_interrupting,
            ),
            FlowElement::SignalBoundaryEvent(e) => events::render_signal_boundary_event(
                e.name.as_deref(),
                eid,
                b,
                cx,
                cy,
                fill,
                stroke,
                sw,
                shape_class,
                e.is_interrupting,
            ),
            FlowElement::EscalationBoundaryEvent(e) => events::render_escalation_boundary_event(
                e.name.as_deref(),
                eid,
                b,
                cx,
                cy,
                fill,
                stroke,
                sw,
                shape_class,
                e.is_interrupting,
            ),
            FlowElement::EventSubProcessStartEvent(e) => events::render_esp_start_event(
                e.name.as_deref(),
                &e.trigger,
                b,
                cx,
                cy,
                fill,
                stroke,
                sw,
                shape_class,
            ),
            FlowElement::ServiceTask(e) => tasks::render_service_task(
                e.name.as_deref(),
                eid,
                b,
                cx,
                cy,
                fill,
                stroke,
                sw,
                shape_class,
                e.topic.is_some(),
            ),
            FlowElement::MultiInstanceTask(e) => tasks::render_multi_instance_task(
                e.name.as_deref(),
                eid,
                b,
                cx,
                cy,
                fill,
                stroke,
                sw,
                shape_class,
            ),
            FlowElement::ScriptTask(e) => tasks::render_script_task(
                e.name.as_deref(),
                eid,
                b,
                cx,
                cy,
                fill,
                stroke,
                sw,
                shape_class,
            ),
            FlowElement::ReceiveTask(e) => tasks::render_receive_task(
                e.name.as_deref(),
                eid,
                b,
                cx,
                cy,
                fill,
                stroke,
                sw,
                shape_class,
            ),
            FlowElement::ExclusiveGateway(e) => gateways::render_exclusive_gateway(
                e.name.as_deref(),
                eid,
                b,
                cx,
                cy,
                fill,
                stroke,
                sw,
                shape_class,
            ),
            FlowElement::ParallelGateway(e) => gateways::render_parallel_gateway(
                e.name.as_deref(),
                eid,
                b,
                cx,
                cy,
                fill,
                stroke,
                sw,
                shape_class,
            ),
            FlowElement::InclusiveGateway(e) => gateways::render_inclusive_gateway(
                e.name.as_deref(),
                eid,
                b,
                cx,
                cy,
                fill,
                stroke,
                sw,
                shape_class,
            ),
            FlowElement::EventBasedGateway(e) => gateways::render_event_based_gateway(
                e.name.as_deref(),
                eid,
                b,
                cx,
                cy,
                fill,
                stroke,
                sw,
                shape_class,
            ),
            FlowElement::SubProcess(e) => subprocesses::render_subprocess(
                e.name.as_deref(),
                eid,
                b,
                cx,
                cy,
                fill,
                stroke,
                sw,
                shape_class,
            ),
            FlowElement::EventSubProcess(e) => subprocesses::render_event_subprocess(
                e.name.as_deref(),
                eid,
                b,
                cx,
                cy,
                fill,
                stroke,
                sw,
                shape_class,
            ),
        };
        shapes.push_str(&shape_svg);

        // Badge position: diamond gateways use the top-right edge midpoint;
        // all other elements use the bounding-box top-right corner.
        let is_gateway = matches!(
            element,
            FlowElement::ExclusiveGateway(_)
                | FlowElement::ParallelGateway(_)
                | FlowElement::InclusiveGateway(_)
                | FlowElement::EventBasedGateway(_)
        );
        let (badge_cx, badge_cy) = if is_gateway {
            let half = b.width.min(b.height) / 2.0;
            (cx + half * 0.5, cy - half * 0.5)
        } else {
            (b.x + b.width, b.y)
        };

        if is_active {
            badges.push_str(&format!(
                "<circle cx=\"{badge_cx}\" cy=\"{badge_cy}\" r=\"8\" fill=\"#3b82f6\" stroke=\"white\" stroke-width=\"2\"/>\n",
            ));
        } else if is_failed {
            badges.push_str(&format!(
                "<circle cx=\"{badge_cx}\" cy=\"{badge_cy}\" r=\"8\" fill=\"#ef4444\" stroke=\"white\" stroke-width=\"2\"/>\n\
                 <text x=\"{badge_cx}\" y=\"{}\" text-anchor=\"middle\" font-size=\"10\" font-weight=\"bold\" fill=\"white\">!</text>\n",
                badge_cy + 3.5,
            ));
        }
    }

    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="100%" height="100%" viewBox="{vx} {vy} {vw} {vh}">
  <defs>
    {BPMN_STYLE}
    <marker id="arrow" markerWidth="8" markerHeight="8" refX="6" refY="3" orient="auto">
      <path d="M0,0 L0,6 L8,3 z" fill="#94a3b8" class="bpmn-marker"/>
    </marker>
  </defs>
  {arrows}{annotation_layer}{shapes}{badges}</svg>"##
    )
}

pub(crate) fn render_svg_di_with_counts(
    definition: &ProcessDefinition,
    layout: &DiagramLayout,
    counts: &HashMap<String, usize>,
    active_ids: &[String],
    failed_elements: &HashSet<String>,
) -> String {
    let Some((vx, vy, vw, vh)) = compute_viewport(layout) else {
        return render_svg_linear(definition, active_ids, &[]);
    };

    let mut shapes = String::new();
    let mut badges = String::new();

    let arrows = flows::render_flows(&definition.sequence_flows, &layout.edges);
    let annotation_layer = render_annotations(definition, layout);

    for element in &definition.elements {
        let eid = element.id();
        let Some(b) = layout.shapes.get(eid) else {
            continue;
        };
        let is_active = active_ids.contains(&eid.to_string());
        let has_failure = failed_elements.contains(eid);
        let fill = if has_failure {
            "#fee2e2"
        } else if is_active {
            "#fef3c7"
        } else {
            "#f8fafc"
        };
        let stroke = if has_failure {
            "#ef4444"
        } else if is_active {
            "#d97706"
        } else {
            "#94a3b8"
        };
        let sw = if has_failure || is_active {
            "2.5"
        } else {
            "1.5"
        };
        let shape_class = if has_failure {
            "bpmn-shape bpmn-failed"
        } else if is_active {
            "bpmn-shape bpmn-active"
        } else {
            "bpmn-shape"
        };

        let cx = b.x + b.width / 2.0;
        let cy = b.y + b.height / 2.0;

        let shape_svg = match element {
            FlowElement::StartEvent(e) => events::render_start_event(
                e.name.as_deref(),
                eid,
                b,
                cx,
                cy,
                fill,
                stroke,
                sw,
                shape_class,
            ),
            FlowElement::TimerStartEvent(e) => events::render_timer_start_event(
                e.name.as_deref(),
                eid,
                b,
                cx,
                cy,
                fill,
                stroke,
                sw,
                shape_class,
            ),
            FlowElement::MessageStartEvent(e) => events::render_message_start_event(
                e.name.as_deref(),
                eid,
                b,
                cx,
                cy,
                fill,
                stroke,
                sw,
                shape_class,
            ),
            FlowElement::SignalStartEvent(e) => events::render_signal_start_event(
                e.name.as_deref(),
                eid,
                b,
                cx,
                cy,
                fill,
                stroke,
                sw,
                shape_class,
            ),
            FlowElement::EndEvent(e) => events::render_end_event(
                e.name.as_deref(),
                eid,
                b,
                cx,
                cy,
                fill,
                stroke,
                sw,
                shape_class,
            ),
            FlowElement::ErrorEndEvent(e) => events::render_error_end_event(
                e.name.as_deref(),
                eid,
                b,
                cx,
                cy,
                fill,
                stroke,
                sw,
                shape_class,
            ),
            FlowElement::TerminateEndEvent(e) => events::render_terminate_end_event(
                e.name.as_deref(),
                b,
                cx,
                cy,
                fill,
                stroke,
                sw,
                shape_class,
            ),
            FlowElement::MessageEndEvent(e) => events::render_message_end_event(
                e.name.as_deref(),
                eid,
                b,
                cx,
                cy,
                fill,
                stroke,
                sw,
                shape_class,
            ),
            FlowElement::SignalEndEvent(e) => events::render_signal_end_event(
                e.name.as_deref(),
                eid,
                b,
                cx,
                cy,
                fill,
                stroke,
                sw,
                shape_class,
            ),
            FlowElement::EscalationEndEvent(e) => events::render_escalation_end_event(
                e.name.as_deref(),
                eid,
                b,
                cx,
                cy,
                fill,
                stroke,
                sw,
                shape_class,
            ),
            FlowElement::TimerIntermediateEvent(e) => events::render_timer_intermediate_event(
                e.name.as_deref(),
                eid,
                b,
                cx,
                cy,
                fill,
                stroke,
                sw,
                shape_class,
            ),
            FlowElement::MessageIntermediateCatchEvent(e) => {
                events::render_message_intermediate_catch_event(
                    e.name.as_deref(),
                    eid,
                    b,
                    cx,
                    cy,
                    fill,
                    stroke,
                    sw,
                    shape_class,
                )
            }
            FlowElement::SignalIntermediateCatchEvent(e) => {
                events::render_signal_intermediate_catch_event(
                    e.name.as_deref(),
                    eid,
                    b,
                    cx,
                    cy,
                    fill,
                    stroke,
                    sw,
                    shape_class,
                )
            }
            FlowElement::IntermediateThrowEvent(e) => events::render_intermediate_throw_event(
                e.name.as_deref(),
                eid,
                b,
                cx,
                cy,
                fill,
                stroke,
                sw,
                shape_class,
            ),
            FlowElement::MessageIntermediateThrowEvent(e) => {
                events::render_message_intermediate_throw_event(
                    e.name.as_deref(),
                    eid,
                    b,
                    cx,
                    cy,
                    fill,
                    stroke,
                    sw,
                    shape_class,
                )
            }
            FlowElement::SignalIntermediateThrowEvent(e) => {
                events::render_signal_intermediate_throw_event(
                    e.name.as_deref(),
                    eid,
                    b,
                    cx,
                    cy,
                    fill,
                    stroke,
                    sw,
                    shape_class,
                )
            }
            FlowElement::EscalationIntermediateThrowEvent(e) => {
                events::render_escalation_intermediate_throw_event(
                    e.name.as_deref(),
                    eid,
                    b,
                    cx,
                    cy,
                    fill,
                    stroke,
                    sw,
                    shape_class,
                )
            }
            FlowElement::LinkIntermediateThrowEvent(e) => {
                events::render_link_intermediate_throw_event(
                    e.name.as_deref(),
                    eid,
                    b,
                    cx,
                    cy,
                    fill,
                    stroke,
                    sw,
                    shape_class,
                )
            }
            FlowElement::LinkIntermediateCatchEvent(e) => {
                events::render_link_intermediate_catch_event(
                    e.name.as_deref(),
                    eid,
                    b,
                    cx,
                    cy,
                    fill,
                    stroke,
                    sw,
                    shape_class,
                )
            }
            FlowElement::BoundaryEvent(e) => events::render_boundary_event(
                e.name.as_deref(),
                b,
                cx,
                cy,
                fill,
                stroke,
                sw,
                shape_class,
            ),
            FlowElement::MessageBoundaryEvent(e) => events::render_message_boundary_event(
                e.name.as_deref(),
                eid,
                b,
                cx,
                cy,
                fill,
                stroke,
                sw,
                shape_class,
            ),
            FlowElement::TimerBoundaryEvent(e) => events::render_timer_boundary_event(
                e.name.as_deref(),
                eid,
                b,
                cx,
                cy,
                fill,
                stroke,
                sw,
                shape_class,
                e.is_interrupting,
            ),
            FlowElement::SignalBoundaryEvent(e) => events::render_signal_boundary_event(
                e.name.as_deref(),
                eid,
                b,
                cx,
                cy,
                fill,
                stroke,
                sw,
                shape_class,
                e.is_interrupting,
            ),
            FlowElement::EscalationBoundaryEvent(e) => events::render_escalation_boundary_event(
                e.name.as_deref(),
                eid,
                b,
                cx,
                cy,
                fill,
                stroke,
                sw,
                shape_class,
                e.is_interrupting,
            ),
            FlowElement::EventSubProcessStartEvent(e) => events::render_esp_start_event(
                e.name.as_deref(),
                &e.trigger,
                b,
                cx,
                cy,
                fill,
                stroke,
                sw,
                shape_class,
            ),
            FlowElement::ServiceTask(e) => tasks::render_service_task(
                e.name.as_deref(),
                eid,
                b,
                cx,
                cy,
                fill,
                stroke,
                sw,
                shape_class,
                e.topic.is_some(),
            ),
            FlowElement::MultiInstanceTask(e) => tasks::render_multi_instance_task(
                e.name.as_deref(),
                eid,
                b,
                cx,
                cy,
                fill,
                stroke,
                sw,
                shape_class,
            ),
            FlowElement::ScriptTask(e) => tasks::render_script_task(
                e.name.as_deref(),
                eid,
                b,
                cx,
                cy,
                fill,
                stroke,
                sw,
                shape_class,
            ),
            FlowElement::ReceiveTask(e) => tasks::render_receive_task(
                e.name.as_deref(),
                eid,
                b,
                cx,
                cy,
                fill,
                stroke,
                sw,
                shape_class,
            ),
            FlowElement::ExclusiveGateway(e) => gateways::render_exclusive_gateway(
                e.name.as_deref(),
                eid,
                b,
                cx,
                cy,
                fill,
                stroke,
                sw,
                shape_class,
            ),
            FlowElement::ParallelGateway(e) => gateways::render_parallel_gateway(
                e.name.as_deref(),
                eid,
                b,
                cx,
                cy,
                fill,
                stroke,
                sw,
                shape_class,
            ),
            FlowElement::InclusiveGateway(e) => gateways::render_inclusive_gateway(
                e.name.as_deref(),
                eid,
                b,
                cx,
                cy,
                fill,
                stroke,
                sw,
                shape_class,
            ),
            FlowElement::EventBasedGateway(e) => gateways::render_event_based_gateway(
                e.name.as_deref(),
                eid,
                b,
                cx,
                cy,
                fill,
                stroke,
                sw,
                shape_class,
            ),
            FlowElement::SubProcess(e) => subprocesses::render_subprocess(
                e.name.as_deref(),
                eid,
                b,
                cx,
                cy,
                fill,
                stroke,
                sw,
                shape_class,
            ),
            FlowElement::EventSubProcess(e) => subprocesses::render_event_subprocess(
                e.name.as_deref(),
                eid,
                b,
                cx,
                cy,
                fill,
                stroke,
                sw,
                shape_class,
            ),
        };
        shapes.push_str(&shape_svg);

        if is_active {
            let count = counts.get(eid).copied().unwrap_or(1);
            // Events: badge at circle edge (cx + r, cy - r).
            // Gateways: badge at diamond top-right edge midpoint.
            // Rectangles: badge at bounding-box top-right corner.
            let r_ev = b.width.min(b.height) / 2.0;
            let (badge_cx, badge_cy) = match element {
                FlowElement::StartEvent(_)
                | FlowElement::TimerStartEvent(_)
                | FlowElement::EndEvent(_)
                | FlowElement::TimerIntermediateEvent(_)
                | FlowElement::MessageIntermediateCatchEvent(_)
                | FlowElement::SignalIntermediateCatchEvent(_)
                | FlowElement::IntermediateThrowEvent(_)
                | FlowElement::BoundaryEvent(_)
                | FlowElement::MessageStartEvent(_)
                | FlowElement::MessageBoundaryEvent(_)
                | FlowElement::TimerBoundaryEvent(_)
                | FlowElement::SignalStartEvent(_)
                | FlowElement::SignalIntermediateThrowEvent(_)
                | FlowElement::SignalEndEvent(_)
                | FlowElement::SignalBoundaryEvent(_)
                | FlowElement::MessageEndEvent(_)
                | FlowElement::MessageIntermediateThrowEvent(_)
                | FlowElement::ErrorEndEvent(_)
                | FlowElement::TerminateEndEvent(_)
                | FlowElement::EscalationIntermediateThrowEvent(_)
                | FlowElement::EscalationEndEvent(_)
                | FlowElement::EscalationBoundaryEvent(_)
                | FlowElement::LinkIntermediateThrowEvent(_)
                | FlowElement::LinkIntermediateCatchEvent(_) => (cx + r_ev, cy - r_ev),
                FlowElement::ExclusiveGateway(_)
                | FlowElement::ParallelGateway(_)
                | FlowElement::InclusiveGateway(_)
                | FlowElement::EventBasedGateway(_) => {
                    let half = b.width.min(b.height) / 2.0;
                    (cx + half * 0.5, cy - half * 0.5)
                }
                _ => (b.x + b.width, b.y),
            };
            let badge_fill = if has_failure { "#ef4444" } else { "#3b82f6" };
            badges.push_str(&format!(
                "<g data-element-id=\"{eid}\" style=\"cursor:pointer\">\
                   <circle cx=\"{badge_cx}\" cy=\"{badge_cy}\" r=\"9\" fill=\"{badge_fill}\" stroke=\"white\" stroke-width=\"2\"/>\
                   <text x=\"{badge_cx}\" y=\"{y}\" text-anchor=\"middle\" font-size=\"9\" font-weight=\"bold\" fill=\"white\">{count}</text>\
                 </g>\n",
                y = badge_cy + 3.5
            ));
        }
    }

    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="100%" height="100%" viewBox="{vx} {vy} {vw} {vh}">
  <defs>
    {BPMN_STYLE}
    <marker id="arrow" markerWidth="8" markerHeight="8" refX="6" refY="3" orient="auto">
      <path d="M0,0 L0,6 L8,3 z" fill="#94a3b8" class="bpmn-marker"/>
    </marker>
  </defs>
  {arrows}{annotation_layer}{shapes}{badges}</svg>"##
    )
}
