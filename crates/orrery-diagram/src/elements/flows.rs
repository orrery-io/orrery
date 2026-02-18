use orrery::diagram::Point;
use orrery::model::SequenceFlow;
use std::collections::HashMap;

pub(crate) fn render_flows(flows: &[SequenceFlow], edges: &HashMap<String, Vec<Point>>) -> String {
    let mut arrows = String::new();
    for flow in flows {
        if let Some(waypoints) = edges.get(&flow.id) {
            if waypoints.len() >= 2 {
                let pts: String = waypoints
                    .iter()
                    .map(|p| format!("{},{}", p.x, p.y))
                    .collect::<Vec<_>>()
                    .join(" ");
                arrows.push_str(&format!(
                    "<polyline points=\"{pts}\" fill=\"none\" stroke=\"#94a3b8\" \
                     stroke-width=\"1.5\" marker-end=\"url(#arrow)\" class=\"bpmn-flow\"/>\n"
                ));
            }
        }
    }
    arrows
}
