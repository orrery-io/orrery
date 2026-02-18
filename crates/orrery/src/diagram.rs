use quick_xml::events::Event;
use quick_xml::reader::Reader;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct Bounds {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Default)]
pub struct DiagramLayout {
    pub shapes: HashMap<String, Bounds>,
    pub edges: HashMap<String, Vec<Point>>,
}

impl DiagramLayout {
    pub fn is_empty(&self) -> bool {
        self.shapes.is_empty()
    }
}

pub fn parse_diagram_layout(xml: &str) -> DiagramLayout {
    let mut layout = DiagramLayout::default();
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut current_shape_id: Option<String> = None;
    let mut current_edge_id: Option<String> = None;
    let mut current_edge_waypoints: Vec<Point> = Vec::new();
    // BPMNLabel children (including their Bounds) must be ignored — they describe
    // label placement, not the shape's own bounding box. Without this guard the
    // label's <Bounds> would overwrite the shape's real bounds.
    let mut inside_label: bool = false;
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let local = local_name_of(e.name());
                match local.as_str() {
                    "BPMNShape" => {
                        current_shape_id = attr_val(e, "bpmnElement");
                        inside_label = false;
                    }
                    "BPMNEdge" => {
                        current_edge_id = attr_val(e, "bpmnElement");
                        current_edge_waypoints = Vec::new();
                    }
                    "BPMNLabel" => {
                        inside_label = true;
                    }
                    _ => {}
                }
            }
            Ok(Event::Empty(ref e)) => {
                let local = local_name_of(e.name());
                match local.as_str() {
                    "Bounds" => {
                        if !inside_label {
                            if let Some(ref id) = current_shape_id {
                                if let (Some(x), Some(y), Some(w), Some(h)) = (
                                    attr_val(e, "x").and_then(|v| v.parse().ok()),
                                    attr_val(e, "y").and_then(|v| v.parse().ok()),
                                    attr_val(e, "width").and_then(|v| v.parse().ok()),
                                    attr_val(e, "height").and_then(|v| v.parse().ok()),
                                ) {
                                    layout.shapes.insert(
                                        id.clone(),
                                        Bounds {
                                            x,
                                            y,
                                            width: w,
                                            height: h,
                                        },
                                    );
                                }
                            }
                        }
                    }
                    "waypoint" => {
                        if current_edge_id.is_some() {
                            if let (Some(x), Some(y)) = (
                                attr_val(e, "x").and_then(|v| v.parse().ok()),
                                attr_val(e, "y").and_then(|v| v.parse().ok()),
                            ) {
                                current_edge_waypoints.push(Point { x, y });
                            }
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::End(ref e)) => {
                let local = local_name_of(e.name());
                match local.as_str() {
                    "BPMNShape" => {
                        current_shape_id = None;
                        inside_label = false;
                    }
                    "BPMNEdge" => {
                        if let Some(id) = current_edge_id.take() {
                            layout
                                .edges
                                .insert(id, std::mem::take(&mut current_edge_waypoints));
                        }
                    }
                    "BPMNLabel" => {
                        inside_label = false;
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }

    layout
}

fn local_name_of(name: quick_xml::name::QName) -> String {
    let raw = std::str::from_utf8(name.as_ref()).unwrap_or("");
    raw.split(':').next_back().unwrap_or(raw).to_string()
}

fn attr_val(e: &quick_xml::events::BytesStart, key: &str) -> Option<String> {
    e.attributes()
        .filter_map(|a| a.ok())
        .find(|a| {
            let k = std::str::from_utf8(a.key.as_ref()).unwrap_or("");
            k == key || k.ends_with(&format!(":{key}"))
        })
        .and_then(|a| a.unescape_value().ok())
        .map(|v| v.into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── parse_diagram_layout ────────────────────────────────────────────────────

    #[test]
    fn parse_diagram_layout_empty_input_is_empty() {
        let layout = parse_diagram_layout("");
        assert!(layout.is_empty());
        assert!(layout.shapes.is_empty());
        assert!(layout.edges.is_empty());
    }

    #[test]
    fn parse_diagram_layout_no_di_section_is_empty() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL" id="simple">
  <process id="proc" isExecutable="true">
    <startEvent id="start"/>
    <endEvent id="end"/>
  </process>
</definitions>"#;
        let layout = parse_diagram_layout(xml);
        assert!(layout.is_empty());
    }

    #[test]
    fn parse_diagram_layout_extracts_shape_bounds() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL"
             xmlns:bpmndi="http://www.omg.org/spec/BPMN/20100524/DI"
             xmlns:dc="http://www.omg.org/spec/DD/20100524/DC">
  <bpmndi:BPMNDiagram>
    <bpmndi:BPMNPlane>
      <bpmndi:BPMNShape bpmnElement="task1">
        <dc:Bounds x="100" y="200" width="120" height="80"/>
      </bpmndi:BPMNShape>
    </bpmndi:BPMNPlane>
  </bpmndi:BPMNDiagram>
</definitions>"#;
        let layout = parse_diagram_layout(xml);
        assert!(!layout.is_empty());
        let b = layout
            .shapes
            .get("task1")
            .expect("task1 shape should be present");
        assert_eq!(b.x, 100.0);
        assert_eq!(b.y, 200.0);
        assert_eq!(b.width, 120.0);
        assert_eq!(b.height, 80.0);
    }

    #[test]
    fn parse_diagram_layout_extracts_multiple_shapes() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL"
             xmlns:bpmndi="http://www.omg.org/spec/BPMN/20100524/DI"
             xmlns:dc="http://www.omg.org/spec/DD/20100524/DC">
  <bpmndi:BPMNDiagram>
    <bpmndi:BPMNPlane>
      <bpmndi:BPMNShape bpmnElement="start">
        <dc:Bounds x="10" y="10" width="36" height="36"/>
      </bpmndi:BPMNShape>
      <bpmndi:BPMNShape bpmnElement="end">
        <dc:Bounds x="200" y="10" width="36" height="36"/>
      </bpmndi:BPMNShape>
    </bpmndi:BPMNPlane>
  </bpmndi:BPMNDiagram>
</definitions>"#;
        let layout = parse_diagram_layout(xml);
        assert_eq!(layout.shapes.len(), 2);
        assert!(layout.shapes.contains_key("start"));
        assert!(layout.shapes.contains_key("end"));
    }

    #[test]
    fn parse_diagram_layout_extracts_edge_waypoints() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL"
             xmlns:bpmndi="http://www.omg.org/spec/BPMN/20100524/DI"
             xmlns:di="http://www.omg.org/spec/DD/20100524/DI">
  <bpmndi:BPMNDiagram>
    <bpmndi:BPMNPlane>
      <bpmndi:BPMNEdge bpmnElement="sf1">
        <di:waypoint x="50" y="100"/>
        <di:waypoint x="150" y="100"/>
        <di:waypoint x="200" y="80"/>
      </bpmndi:BPMNEdge>
    </bpmndi:BPMNPlane>
  </bpmndi:BPMNDiagram>
</definitions>"#;
        let layout = parse_diagram_layout(xml);
        let wps = layout.edges.get("sf1").expect("sf1 edge should be present");
        assert_eq!(wps.len(), 3);
        assert_eq!(wps[0].x, 50.0);
        assert_eq!(wps[0].y, 100.0);
        assert_eq!(wps[1].x, 150.0);
        assert_eq!(wps[2].x, 200.0);
        assert_eq!(wps[2].y, 80.0);
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
}
