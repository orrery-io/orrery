#![allow(clippy::too_many_arguments)]

mod elements;
mod icons;
mod render_di;
mod render_linear;
mod style;

use orrery::diagram::DiagramLayout;
use orrery::model::ProcessDefinition;
use std::collections::{HashMap, HashSet};

pub fn render_svg(
    definition: &ProcessDefinition,
    layout: &DiagramLayout,
    active_ids: &[String],
    failed_ids: &[String],
) -> String {
    if !layout.is_empty() {
        return render_di::render_svg_di(definition, layout, active_ids, failed_ids);
    }
    render_linear::render_svg_linear(definition, active_ids, failed_ids)
}

pub fn render_svg_with_counts(
    definition: &ProcessDefinition,
    layout: &DiagramLayout,
    counts: &HashMap<String, usize>,
    active_ids: &[String],
    failed_elements: &HashSet<String>,
) -> String {
    if !layout.is_empty() {
        return render_di::render_svg_di_with_counts(
            definition,
            layout,
            counts,
            active_ids,
            failed_elements,
        );
    }
    render_linear::render_svg_linear(definition, active_ids, &[])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render_di::{render_svg_di, render_svg_di_with_counts};
    use crate::style::escape_xml;
    use orrery::diagram::parse_diagram_layout;
    use orrery::parser::parse_bpmn;

    // ── escape_xml ──────────────────────────────────────────────────────────────

    #[test]
    fn escape_xml_plain_text_is_unchanged() {
        assert_eq!(escape_xml("hello world"), "hello world");
    }

    #[test]
    fn escape_xml_ampersand() {
        assert_eq!(escape_xml("a & b"), "a &amp; b");
    }

    #[test]
    fn escape_xml_less_than() {
        assert_eq!(escape_xml("x < y"), "x &lt; y");
    }

    #[test]
    fn escape_xml_greater_than() {
        assert_eq!(escape_xml("x > y"), "x &gt; y");
    }

    #[test]
    fn escape_xml_double_quote() {
        assert_eq!(escape_xml(r#"say "hi""#), "say &quot;hi&quot;");
    }

    #[test]
    fn escape_xml_all_special_chars_combined() {
        assert_eq!(
            escape_xml(r#"<"a" & 'b'>"#),
            "&lt;&quot;a&quot; &amp; 'b'&gt;"
        );
    }

    #[test]
    fn escape_xml_empty_string() {
        assert_eq!(escape_xml(""), "");
    }

    // ── render_svg (linear fallback, no DI section) ─────────────────────────────

    const SIMPLE_BPMN_NO_DI: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL" id="simple">
  <process id="proc" isExecutable="true">
    <startEvent id="start"><outgoing>sf1</outgoing></startEvent>
    <sequenceFlow id="sf1" sourceRef="start" targetRef="task1"/>
    <serviceTask id="task1" name="My Task"><outgoing>sf2</outgoing></serviceTask>
    <sequenceFlow id="sf2" sourceRef="task1" targetRef="end"/>
    <endEvent id="end"/>
  </process>
</definitions>"#;

    #[test]
    fn render_svg_linear_produces_valid_svg() {
        let definition = parse_bpmn(SIMPLE_BPMN_NO_DI).expect("should parse");
        let layout = parse_diagram_layout(SIMPLE_BPMN_NO_DI);
        assert!(layout.is_empty(), "no DI section → layout should be empty");

        let svg = render_svg(&definition, &layout, &[], &[]);
        assert!(svg.starts_with("<svg"), "output should be an SVG element");
        // The fixture has name="My Task", so the name is used as the label
        assert!(
            svg.contains("My Task"),
            "should render the task element name"
        );
        assert!(svg.contains("Start"), "should render start event label");
        assert!(svg.contains("End"), "should render end event label");
    }

    #[test]
    fn render_svg_idle_has_no_active_or_failed_class() {
        let definition = parse_bpmn(SIMPLE_BPMN_NO_DI).expect("should parse");
        let layout = parse_diagram_layout(SIMPLE_BPMN_NO_DI);
        let svg = render_svg(&definition, &layout, &[], &[]);
        // Check the class *attribute* value (space-separated), not the CSS selector (dot-separated)
        assert!(
            !svg.contains("bpmn-shape bpmn-active"),
            "idle diagram should have no active element classes"
        );
        assert!(
            !svg.contains("bpmn-shape bpmn-failed"),
            "idle diagram should have no failed element classes"
        );
    }

    #[test]
    fn render_svg_marks_active_element() {
        let definition = parse_bpmn(SIMPLE_BPMN_NO_DI).expect("should parse");
        let layout = parse_diagram_layout(SIMPLE_BPMN_NO_DI);
        let svg = render_svg(&definition, &layout, &["task1".to_string()], &[]);
        // The class attribute value uses spaces: "bpmn-shape bpmn-active"
        assert!(
            svg.contains("bpmn-shape bpmn-active"),
            "active element should have bpmn-active class"
        );
        assert!(
            !svg.contains("bpmn-shape bpmn-failed"),
            "active-only diagram should have no failed element classes"
        );
    }

    #[test]
    fn render_svg_marks_failed_element_with_red_color() {
        let definition = parse_bpmn(SIMPLE_BPMN_NO_DI).expect("should parse");
        let layout = parse_diagram_layout(SIMPLE_BPMN_NO_DI);
        let svg = render_svg(&definition, &layout, &[], &["task1".to_string()]);
        assert!(
            svg.contains("bpmn-shape bpmn-failed"),
            "failed element should have bpmn-failed class"
        );
        assert!(
            svg.contains("#ef4444"),
            "failed element should use red color"
        );
        assert!(
            !svg.contains("bpmn-shape bpmn-active"),
            "failed-only diagram should have no active element classes"
        );
    }

    // ── intermediate event centering (Task 1 regression test) ────────────────
    //
    // When DI bounds place a timer/message/signal intermediate event at a known
    // position, the rendered circle must use the bounding-box center (cx, cy),
    // NOT the position computed by event_center_from_flow which shifts by one
    // radius in the flow direction.

    /// BPMN with DI: timer intermediate event at x=270,y=170,w=36,h=36
    /// → bounding-box center is cx=288, cy=188.
    /// A flow waypoint at (288, 188) is placed at the shape center (Camunda Modeler style).
    const TIMER_INTERMEDIATE_DI_BPMN: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL"
             xmlns:bpmndi="http://www.omg.org/spec/BPMN/20100524/DI"
             xmlns:dc="http://www.omg.org/spec/DD/20100524/DC"
             xmlns:di="http://www.omg.org/spec/DD/20100524/DI"
             id="TimerDI">
  <process id="p1" isExecutable="true">
    <startEvent id="start"><outgoing>sf1</outgoing></startEvent>
    <sequenceFlow id="sf1" sourceRef="start" targetRef="timer1"/>
    <intermediateCatchEvent id="timer1">
      <outgoing>sf2</outgoing>
      <timerEventDefinition><timeDuration>PT5M</timeDuration></timerEventDefinition>
    </intermediateCatchEvent>
    <sequenceFlow id="sf2" sourceRef="timer1" targetRef="end"/>
    <endEvent id="end"/>
  </process>
  <bpmndi:BPMNDiagram>
    <bpmndi:BPMNPlane bpmnElement="p1">
      <bpmndi:BPMNShape bpmnElement="start">
        <dc:Bounds x="152" y="170" width="36" height="36"/>
      </bpmndi:BPMNShape>
      <bpmndi:BPMNShape bpmnElement="timer1">
        <dc:Bounds x="270" y="170" width="36" height="36"/>
      </bpmndi:BPMNShape>
      <bpmndi:BPMNShape bpmnElement="end">
        <dc:Bounds x="390" y="170" width="36" height="36"/>
      </bpmndi:BPMNShape>
      <bpmndi:BPMNEdge bpmnElement="sf1">
        <di:waypoint x="188" y="188"/>
        <di:waypoint x="270" y="188"/>
      </bpmndi:BPMNEdge>
      <bpmndi:BPMNEdge bpmnElement="sf2">
        <di:waypoint x="288" y="188"/>
        <di:waypoint x="390" y="188"/>
      </bpmndi:BPMNEdge>
    </bpmndi:BPMNPlane>
  </bpmndi:BPMNDiagram>
</definitions>"#;

    #[test]
    fn timer_intermediate_event_renders_at_bounding_box_center() {
        let definition = parse_bpmn(TIMER_INTERMEDIATE_DI_BPMN).expect("should parse");
        let layout = parse_diagram_layout(TIMER_INTERMEDIATE_DI_BPMN);
        assert!(!layout.is_empty(), "DI section should parse");

        let svg = render_svg_di(&definition, &layout, &[], &[]);
        // timer1 bounding box: x=270, y=170, w=36, h=36 → center (288, 188), r=18.
        // Intermediate events use the exact BPMN DI bounding-box center and radius (no EVENT_R_SCALE,
        // no waypoint snapping). This keeps both the incoming AND outgoing sequence-flow waypoints
        // aligned with the rendered circle boundary simultaneously.
        assert!(
            svg.contains("cx=\"288\"") && svg.contains("cy=\"188\""),
            "timer intermediate event circle must render at bounding-box center (288, 188); svg=\n{svg}"
        );
        // Also verify the clock icon is present (raw SVG lines, not Unicode)
        assert!(
            svg.contains("class=\"bpmn-ring\""),
            "timer intermediate event should contain clock icon SVG elements"
        );
    }

    /// BPMN with a BPMNLabel (containing its own Bounds) nested inside a BPMNShape.
    /// The label Bounds must NOT overwrite the shape's own Bounds — otherwise intermediate
    /// events render at the wrong (label) position with a tiny radius.
    const BPMN_WITH_LABEL_BOUNDS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL"
             xmlns:bpmndi="http://www.omg.org/spec/BPMN/20100524/DI"
             xmlns:dc="http://www.omg.org/spec/DD/20100524/DC"
             xmlns:di="http://www.omg.org/spec/DD/20100524/DI"
             id="LabelTest">
  <process id="p1" isExecutable="true">
    <startEvent id="start"><outgoing>sf1</outgoing></startEvent>
    <sequenceFlow id="sf1" sourceRef="start" targetRef="timer1"/>
    <intermediateCatchEvent id="timer1">
      <outgoing>sf2</outgoing>
      <timerEventDefinition><timeDuration>PT5M</timeDuration></timerEventDefinition>
    </intermediateCatchEvent>
    <sequenceFlow id="sf2" sourceRef="timer1" targetRef="end"/>
    <endEvent id="end"/>
  </process>
  <bpmndi:BPMNDiagram>
    <bpmndi:BPMNPlane bpmnElement="p1">
      <bpmndi:BPMNShape bpmnElement="start">
        <dc:Bounds x="152" y="170" width="36" height="36"/>
        <bpmndi:BPMNLabel>
          <dc:Bounds x="158" y="213" width="24" height="14"/>
        </bpmndi:BPMNLabel>
      </bpmndi:BPMNShape>
      <bpmndi:BPMNShape bpmnElement="timer1">
        <dc:Bounds x="270" y="170" width="36" height="36"/>
        <bpmndi:BPMNLabel>
          <dc:Bounds x="268" y="213" width="40" height="14"/>
        </bpmndi:BPMNLabel>
      </bpmndi:BPMNShape>
      <bpmndi:BPMNShape bpmnElement="end">
        <dc:Bounds x="390" y="170" width="36" height="36"/>
      </bpmndi:BPMNShape>
      <bpmndi:BPMNEdge bpmnElement="sf1">
        <di:waypoint x="188" y="188"/>
        <di:waypoint x="270" y="188"/>
      </bpmndi:BPMNEdge>
      <bpmndi:BPMNEdge bpmnElement="sf2">
        <di:waypoint x="306" y="188"/>
        <di:waypoint x="390" y="188"/>
      </bpmndi:BPMNEdge>
    </bpmndi:BPMNPlane>
  </bpmndi:BPMNDiagram>
</definitions>"#;

    #[test]
    fn bpmnlabel_bounds_do_not_overwrite_shape_bounds() {
        let layout = parse_diagram_layout(BPMN_WITH_LABEL_BOUNDS);
        // timer1 shape bounds: x=270, y=170, w=36, h=36 (NOT the label bounds x=268, y=213, w=40, h=14)
        let b = layout
            .shapes
            .get("timer1")
            .expect("timer1 shape should be present");
        assert_eq!(
            b.x, 270.0,
            "shape x should come from shape Bounds, not label Bounds"
        );
        assert_eq!(
            b.y, 170.0,
            "shape y should come from shape Bounds, not label Bounds"
        );
        assert_eq!(
            b.width, 36.0,
            "shape width should come from shape Bounds, not label Bounds"
        );
        assert_eq!(
            b.height, 36.0,
            "shape height should come from shape Bounds, not label Bounds"
        );
    }

    #[test]
    fn timer_intermediate_event_with_label_renders_at_correct_position() {
        let definition = parse_bpmn(BPMN_WITH_LABEL_BOUNDS).expect("should parse");
        let layout = parse_diagram_layout(BPMN_WITH_LABEL_BOUNDS);
        let svg = render_svg_di(&definition, &layout, &[], &[]);
        // timer1 shape bounds: x=270, y=170, w=36, h=36 → center (288, 188), r=18.
        // The label Bounds at y=213 must NOT shift the rendered circle to y=220 (label center).
        assert!(
            svg.contains("cx=\"288\"") && svg.contains("cy=\"188\""),
            "timer circle must use shape bounds center (288, 188), not label bounds; svg=\n{svg}"
        );
    }

    #[test]
    fn timer_intermediate_event_badge_at_bounding_box_edge() {
        let definition = parse_bpmn(TIMER_INTERMEDIATE_DI_BPMN).expect("should parse");
        let layout = parse_diagram_layout(TIMER_INTERMEDIATE_DI_BPMN);

        // With timer1 active, render_svg_di_with_counts should place the badge at
        // (cx + r, cy - r) where cx=288, cy=188, r=18 (exact BPMN DI radius, no EVENT_R_SCALE).
        // Badge position: (288 + 18, 188 - 18) = (306, 170).
        // We just verify the badge does NOT appear at any position that includes the
        // old overcorrected cx (e.g., cx=270 or cx=252).
        let counts = std::collections::HashMap::from([("timer1".to_string(), 1usize)]);
        let failed = std::collections::HashSet::new();
        let svg = render_svg_di_with_counts(
            &definition,
            &layout,
            &counts,
            &["timer1".to_string()],
            &failed,
        );

        // The badge circle must appear somewhere near cx=288
        assert!(
            svg.contains("#3b82f6"),
            "active timer element should have blue badge"
        );
        // The badge x-coordinate should not be at the shape left edge (270) or further left
        assert!(
            !svg.contains("cx=\"270\"") && !svg.contains("cx=\"252\""),
            "badge must not be placed at the overcorrected position; svg=\n{svg}"
        );
    }

    // ── start/end event BPMN DI center rendering ─────────────────────────────
    //
    // Start and end events must render at the exact BPMN DI bounding-box center
    // and radius — same as intermediate events. No EVENT_R_SCALE inflation and no
    // waypoint-based center shift.
    //
    // Using TIMER_INTERMEDIATE_DI_BPMN:
    //   start: x=152, y=170, w=36, h=36 → center (170, 188), r=18
    //   end:   x=390, y=170, w=36, h=36 → center (408, 188), r=18
    // Flow waypoints deliberately align with the BPMN DI boundaries:
    //   sf1 first: (188, 188) = right boundary of start (170+18=188) ✓
    //   sf2 last:  (390, 188) = left boundary of end ✓

    #[test]
    fn start_event_renders_at_bounding_box_center() {
        let definition = parse_bpmn(TIMER_INTERMEDIATE_DI_BPMN).expect("should parse");
        let layout = parse_diagram_layout(TIMER_INTERMEDIATE_DI_BPMN);
        let svg = render_svg_di(&definition, &layout, &[], &[]);
        // start bounding box: x=152, y=170, w=36, h=36 → center (170, 188), r=18.
        // Must NOT be shifted (e.g. to 164.6) by old event_center_from_flow + EVENT_R_SCALE logic.
        assert!(
            svg.contains("cx=\"170\"") && svg.contains("cy=\"188\""),
            "start event circle must render at bounding-box center (170, 188); svg=\n{svg}"
        );
    }

    #[test]
    fn end_event_renders_at_bounding_box_center() {
        let definition = parse_bpmn(TIMER_INTERMEDIATE_DI_BPMN).expect("should parse");
        let layout = parse_diagram_layout(TIMER_INTERMEDIATE_DI_BPMN);
        let svg = render_svg_di(&definition, &layout, &[], &[]);
        // end bounding box: x=390, y=170, w=36, h=36 → center (408, 188), r=18.
        // Must NOT be shifted (e.g. to 413.4) by old event_center_from_flow + EVENT_R_SCALE logic.
        assert!(
            svg.contains("cx=\"408\"") && svg.contains("cy=\"188\""),
            "end event circle must render at bounding-box center (408, 188); svg=\n{svg}"
        );
    }

    // ── task type icons ──────────────────────────────────────────────────────────

    const EXTERNAL_SERVICE_TASK_BPMN_NO_DI: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL"
             xmlns:camunda="http://camunda.org/schema/1.0/bpmn" id="ext">
  <process id="proc" isExecutable="true">
    <startEvent id="start"><outgoing>sf1</outgoing></startEvent>
    <sequenceFlow id="sf1" sourceRef="start" targetRef="task1"/>
    <serviceTask id="task1" name="Ext Task" camunda:type="external" camunda:topic="my-worker">
      <outgoing>sf2</outgoing>
    </serviceTask>
    <sequenceFlow id="sf2" sourceRef="task1" targetRef="end"/>
    <endEvent id="end"/>
  </process>
</definitions>"#;

    const RECEIVE_TASK_BPMN_NO_DI: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL" id="recv">
  <process id="proc" isExecutable="true">
    <startEvent id="start"><outgoing>sf1</outgoing></startEvent>
    <sequenceFlow id="sf1" sourceRef="start" targetRef="task1"/>
    <receiveTask id="task1" name="Wait Msg">
      <outgoing>sf2</outgoing>
    </receiveTask>
    <sequenceFlow id="sf2" sourceRef="task1" targetRef="end"/>
    <endEvent id="end"/>
  </process>
</definitions>"#;

    /// BPMN with DI: external ServiceTask at x=100,y=50,w=120,h=60.
    /// min(120,60)*0.30 = 18 → expected font-size 18.
    const EXTERNAL_SERVICE_TASK_DI_BPMN: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL"
             xmlns:bpmndi="http://www.omg.org/spec/BPMN/20100524/DI"
             xmlns:dc="http://www.omg.org/spec/DD/20100524/DC"
             xmlns:camunda="http://camunda.org/schema/1.0/bpmn" id="extdi">
  <process id="p1" isExecutable="true">
    <startEvent id="start"><outgoing>sf1</outgoing></startEvent>
    <sequenceFlow id="sf1" sourceRef="start" targetRef="task1"/>
    <serviceTask id="task1" name="Ext Task" camunda:type="external" camunda:topic="my-worker">
      <outgoing>sf2</outgoing>
    </serviceTask>
    <sequenceFlow id="sf2" sourceRef="task1" targetRef="end"/>
    <endEvent id="end"/>
  </process>
  <bpmndi:BPMNDiagram>
    <bpmndi:BPMNPlane bpmnElement="p1">
      <bpmndi:BPMNShape bpmnElement="start">
        <dc:Bounds x="32" y="62" width="36" height="36"/>
      </bpmndi:BPMNShape>
      <bpmndi:BPMNShape bpmnElement="task1">
        <dc:Bounds x="100" y="50" width="120" height="60"/>
      </bpmndi:BPMNShape>
      <bpmndi:BPMNShape bpmnElement="end">
        <dc:Bounds x="282" y="62" width="36" height="36"/>
      </bpmndi:BPMNShape>
    </bpmndi:BPMNPlane>
  </bpmndi:BPMNDiagram>
</definitions>"#;

    #[test]
    fn external_service_task_shows_gear_icon_in_linear_rendering() {
        let definition = parse_bpmn(EXTERNAL_SERVICE_TASK_BPMN_NO_DI).expect("should parse");
        let layout = parse_diagram_layout(EXTERNAL_SERVICE_TASK_BPMN_NO_DI);
        let svg = render_svg(&definition, &layout, &[], &[]);
        assert!(
            svg.contains("class=\"bpmn-muted\""),
            "external service task should have gear icon; svg=\n{svg}"
        );
    }

    #[test]
    fn plain_service_task_has_no_gear_icon() {
        let definition = parse_bpmn(SIMPLE_BPMN_NO_DI).expect("should parse");
        let layout = parse_diagram_layout(SIMPLE_BPMN_NO_DI);
        let svg = render_svg(&definition, &layout, &[], &[]);
        // Plain service tasks (no topic) should have no gear icon SVG elements
        let gear_line_count = svg.matches("class=\"bpmn-muted\"").count();
        assert_eq!(
            gear_line_count, 0,
            "plain service task should not have gear icon; svg=\n{svg}"
        );
    }

    #[test]
    fn receive_task_shows_envelope_icon_in_linear_rendering() {
        let definition = parse_bpmn(RECEIVE_TASK_BPMN_NO_DI).expect("should parse");
        let layout = parse_diagram_layout(RECEIVE_TASK_BPMN_NO_DI);
        let svg = render_svg(&definition, &layout, &[], &[]);
        assert!(
            svg.contains("class=\"bpmn-muted\"") && svg.contains("<polyline"),
            "receive task should have SVG envelope icon; svg=\n{svg}"
        );
    }

    #[test]
    fn external_service_task_icon_size_is_proportional_in_di_rendering() {
        let definition = parse_bpmn(EXTERNAL_SERVICE_TASK_DI_BPMN).expect("should parse");
        let layout = parse_diagram_layout(EXTERNAL_SERVICE_TASK_DI_BPMN);
        assert!(!layout.is_empty(), "DI section should parse");
        let svg = render_svg_di(&definition, &layout, &[], &[]);
        // task1: x=100, y=50, w=120, h=60 → gear uses raw SVG lines
        assert!(
            svg.contains("class=\"bpmn-muted\""),
            "external service task should have gear icon in DI rendering; svg=\n{svg}"
        );
    }

    // ── Annotation rendering ─────────────────────────────────────────────────────

    const ANNOTATION_BPMN: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<bpmn:definitions xmlns:bpmn="http://www.omg.org/spec/BPMN/20100524/MODEL"
    xmlns:bpmndi="http://www.omg.org/spec/BPMN/20100524/DI"
    xmlns:dc="http://www.omg.org/spec/DD/20100524/DC"
    xmlns:di="http://www.omg.org/spec/DD/20100524/DI"
    id="Def1" targetNamespace="test">
  <bpmn:process id="p1" isExecutable="true">
    <bpmn:startEvent id="start"><bpmn:outgoing>f1</bpmn:outgoing></bpmn:startEvent>
    <bpmn:endEvent id="end"><bpmn:incoming>f1</bpmn:incoming></bpmn:endEvent>
    <bpmn:sequenceFlow id="f1" sourceRef="start" targetRef="end"/>
    <bpmn:textAnnotation id="Ann1">
      <bpmn:text>A note</bpmn:text>
    </bpmn:textAnnotation>
    <bpmn:association id="Assoc1" associationDirection="None"
        sourceRef="start" targetRef="Ann1"/>
  </bpmn:process>
  <bpmndi:BPMNDiagram id="Diagram1">
    <bpmndi:BPMNPlane id="Plane1" bpmnElement="p1">
      <bpmndi:BPMNShape id="start_di" bpmnElement="start">
        <dc:Bounds x="100" y="100" width="36" height="36"/>
      </bpmndi:BPMNShape>
      <bpmndi:BPMNShape id="end_di" bpmnElement="end">
        <dc:Bounds x="300" y="100" width="36" height="36"/>
      </bpmndi:BPMNShape>
      <bpmndi:BPMNShape id="Ann1_di" bpmnElement="Ann1">
        <dc:Bounds x="180" y="180" width="100" height="60"/>
      </bpmndi:BPMNShape>
      <bpmndi:BPMNEdge id="f1_di" bpmnElement="f1">
        <di:waypoint x="136" y="118"/>
        <di:waypoint x="300" y="118"/>
      </bpmndi:BPMNEdge>
      <bpmndi:BPMNEdge id="Assoc1_di" bpmnElement="Assoc1">
        <di:waypoint x="118" y="136"/>
        <di:waypoint x="180" y="180"/>
      </bpmndi:BPMNEdge>
    </bpmndi:BPMNPlane>
  </bpmndi:BPMNDiagram>
</bpmn:definitions>"#;

    #[test]
    fn annotation_shape_renders_open_bracket() {
        let def = parse_bpmn(ANNOTATION_BPMN).unwrap();
        let layout = parse_diagram_layout(ANNOTATION_BPMN);
        let svg = render_svg(&def, &layout, &[], &[]);
        // The open bracket path starts at (x+ARM, y) and goes to (x, y)
        // Ann1 bounds: x=180, y=180, w=100, h=60; ARM=10
        // Expect path d contains "M190,180 L180,180 L180,240 L190,240"
        assert!(svg.contains("M190,180"), "bracket path missing: {svg}");
        assert!(
            svg.contains("L180,180"),
            "bracket top-left corner missing: {svg}"
        );
        assert!(svg.contains("L180,240"), "bracket left side missing: {svg}");
        assert!(
            svg.contains("L190,240"),
            "bracket bottom arm missing: {svg}"
        );
    }

    #[test]
    fn annotation_text_rendered_inside_bracket() {
        let def = parse_bpmn(ANNOTATION_BPMN).unwrap();
        let layout = parse_diagram_layout(ANNOTATION_BPMN);
        let svg = render_svg(&def, &layout, &[], &[]);
        assert!(
            svg.contains("A note"),
            "annotation text should appear in SVG: {svg}"
        );
    }

    #[test]
    fn association_renders_dashed_line() {
        let def = parse_bpmn(ANNOTATION_BPMN).unwrap();
        let layout = parse_diagram_layout(ANNOTATION_BPMN);
        let svg = render_svg(&def, &layout, &[], &[]);
        // Association rendered as a dashed polyline
        assert!(
            svg.contains("stroke-dasharray"),
            "association dashed line missing: {svg}"
        );
        // Waypoints: 118,136 → 180,180
        assert!(
            svg.contains("118,136"),
            "association waypoint missing: {svg}"
        );
    }

    #[test]
    fn annotation_missing_from_layout_is_skipped() {
        // Annotation in model but no DI shape — should not panic, not appear in SVG
        let bpmn_no_ann_shape = r#"<?xml version="1.0" encoding="UTF-8"?>
<bpmn:definitions xmlns:bpmn="http://www.omg.org/spec/BPMN/20100524/MODEL"
    xmlns:bpmndi="http://www.omg.org/spec/BPMN/20100524/DI"
    xmlns:dc="http://www.omg.org/spec/DD/20100524/DC"
    xmlns:di="http://www.omg.org/spec/DD/20100524/DI"
    id="Def1" targetNamespace="test">
  <bpmn:process id="p1" isExecutable="true">
    <bpmn:startEvent id="start"><bpmn:outgoing>f1</bpmn:outgoing></bpmn:startEvent>
    <bpmn:endEvent id="end"><bpmn:incoming>f1</bpmn:incoming></bpmn:endEvent>
    <bpmn:sequenceFlow id="f1" sourceRef="start" targetRef="end"/>
    <bpmn:textAnnotation id="AnnNoShape">
      <bpmn:text>Ghost note</bpmn:text>
    </bpmn:textAnnotation>
  </bpmn:process>
  <bpmndi:BPMNDiagram id="Diagram1">
    <bpmndi:BPMNPlane id="Plane1" bpmnElement="p1">
      <bpmndi:BPMNShape id="start_di" bpmnElement="start">
        <dc:Bounds x="100" y="100" width="36" height="36"/>
      </bpmndi:BPMNShape>
      <bpmndi:BPMNShape id="end_di" bpmnElement="end">
        <dc:Bounds x="300" y="100" width="36" height="36"/>
      </bpmndi:BPMNShape>
      <bpmndi:BPMNEdge id="f1_di" bpmnElement="f1">
        <di:waypoint x="136" y="118"/>
        <di:waypoint x="300" y="118"/>
      </bpmndi:BPMNEdge>
    </bpmndi:BPMNPlane>
  </bpmndi:BPMNDiagram>
</bpmn:definitions>"#;
        let def = parse_bpmn(bpmn_no_ann_shape).unwrap();
        let layout = parse_diagram_layout(bpmn_no_ann_shape);
        // Should not panic
        let svg = render_svg(&def, &layout, &[], &[]);
        assert!(
            !svg.contains("Ghost note"),
            "annotation without DI shape must not render: {svg}"
        );
    }

    #[test]
    fn annotation_empty_text_omits_text_element() {
        let bpmn = r#"<?xml version="1.0" encoding="UTF-8"?>
<bpmn:definitions xmlns:bpmn="http://www.omg.org/spec/BPMN/20100524/MODEL"
    xmlns:bpmndi="http://www.omg.org/spec/BPMN/20100524/DI"
    xmlns:dc="http://www.omg.org/spec/DD/20100524/DC"
    id="Def1" targetNamespace="test">
  <bpmn:process id="p1" isExecutable="true">
    <bpmn:startEvent id="start"><bpmn:outgoing>f1</bpmn:outgoing></bpmn:startEvent>
    <bpmn:endEvent id="end"><bpmn:incoming>f1</bpmn:incoming></bpmn:endEvent>
    <bpmn:sequenceFlow id="f1" sourceRef="start" targetRef="end"/>
    <bpmn:textAnnotation id="AnnEmpty">
      <bpmn:text></bpmn:text>
    </bpmn:textAnnotation>
  </bpmn:process>
  <bpmndi:BPMNDiagram id="Diagram1">
    <bpmndi:BPMNPlane id="Plane1" bpmnElement="p1">
      <bpmndi:BPMNShape id="start_di" bpmnElement="start">
        <dc:Bounds x="100" y="100" width="36" height="36"/>
      </bpmndi:BPMNShape>
      <bpmndi:BPMNShape id="end_di" bpmnElement="end">
        <dc:Bounds x="300" y="100" width="36" height="36"/>
      </bpmndi:BPMNShape>
      <bpmndi:BPMNShape id="AnnEmpty_di" bpmnElement="AnnEmpty">
        <dc:Bounds x="180" y="180" width="100" height="60"/>
      </bpmndi:BPMNShape>
      <bpmndi:BPMNEdge id="f1_di" bpmnElement="f1">
        <di:waypoint x="136" y="118"/>
        <di:waypoint x="300" y="118"/>
      </bpmndi:BPMNEdge>
    </bpmndi:BPMNPlane>
  </bpmndi:BPMNDiagram>
</bpmn:definitions>"#;
        let def = parse_bpmn(bpmn).unwrap();
        let layout = parse_diagram_layout(bpmn);
        let svg = render_svg(&def, &layout, &[], &[]);
        // Open bracket path should still render (the box exists)
        assert!(
            svg.contains("M190,180"),
            "bracket should still render for empty annotation: {svg}"
        );
        // But no annotation <text> element should appear — check for the annotation-specific
        // combination of font-size="11" and fill="#475569" used only by annotation text rendering
        assert!(
            !svg.contains(r##"font-size="11" fill="#475569""##),
            "empty annotation must not emit a <text> element: {svg}"
        );
    }

    #[test]
    fn annotation_text_xml_special_chars_escaped() {
        let bpmn = r#"<?xml version="1.0" encoding="UTF-8"?>
<bpmn:definitions xmlns:bpmn="http://www.omg.org/spec/BPMN/20100524/MODEL"
    xmlns:bpmndi="http://www.omg.org/spec/BPMN/20100524/DI"
    xmlns:dc="http://www.omg.org/spec/DD/20100524/DC"
    id="Def1" targetNamespace="test">
  <bpmn:process id="p1" isExecutable="true">
    <bpmn:startEvent id="start"><bpmn:outgoing>f1</bpmn:outgoing></bpmn:startEvent>
    <bpmn:endEvent id="end"><bpmn:incoming>f1</bpmn:incoming></bpmn:endEvent>
    <bpmn:sequenceFlow id="f1" sourceRef="start" targetRef="end"/>
    <bpmn:textAnnotation id="AnnSpecial">
      <bpmn:text>a &amp; b &lt; c</bpmn:text>
    </bpmn:textAnnotation>
  </bpmn:process>
  <bpmndi:BPMNDiagram id="Diagram1">
    <bpmndi:BPMNPlane id="Plane1" bpmnElement="p1">
      <bpmndi:BPMNShape id="start_di" bpmnElement="start">
        <dc:Bounds x="100" y="100" width="36" height="36"/>
      </bpmndi:BPMNShape>
      <bpmndi:BPMNShape id="end_di" bpmnElement="end">
        <dc:Bounds x="300" y="100" width="36" height="36"/>
      </bpmndi:BPMNShape>
      <bpmndi:BPMNShape id="AnnSpecial_di" bpmnElement="AnnSpecial">
        <dc:Bounds x="180" y="180" width="100" height="60"/>
      </bpmndi:BPMNShape>
      <bpmndi:BPMNEdge id="f1_di" bpmnElement="f1">
        <di:waypoint x="136" y="118"/>
        <di:waypoint x="300" y="118"/>
      </bpmndi:BPMNEdge>
    </bpmndi:BPMNPlane>
  </bpmndi:BPMNDiagram>
</bpmn:definitions>"#;
        // Note: quick_xml decodes &amp; to & and &lt; to < during parsing,
        // so the annotation text will be "a & b < c" — escape_xml must re-escape for SVG output.
        let def = parse_bpmn(bpmn).unwrap();
        let layout = parse_diagram_layout(bpmn);
        let svg = render_svg(&def, &layout, &[], &[]);
        assert!(
            svg.contains("&amp;"),
            "& must be escaped in SVG output: {svg}"
        );
        assert!(
            svg.contains("&lt;"),
            "< must be escaped in SVG output: {svg}"
        );
    }
}
