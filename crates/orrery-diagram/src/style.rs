/// SVG `<style>` block injected into every diagram's `<defs>`.
/// Light-mode colours are kept as SVG presentation attributes (fallback).
/// Dark-mode overrides use `@media (prefers-color-scheme: dark)` — CSS rules
/// win over presentation attributes per the SVG spec.
pub(crate) const BPMN_STYLE: &str = r#"<style>
    @media (prefers-color-scheme: dark) {
      .bpmn-text  { fill: #cbd5e1 }
      .bpmn-muted { fill: #94a3b8 }
      .bpmn-shape { fill: #1e293b; stroke: #475569 }
      .bpmn-shape.bpmn-active { fill: #451a03; stroke: #f59e0b }
      .bpmn-ring  { stroke: #475569 }
      .bpmn-flow  { stroke: #475569 }
      .bpmn-marker { fill: #475569 }
    }
  </style>"#;

pub(crate) fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
