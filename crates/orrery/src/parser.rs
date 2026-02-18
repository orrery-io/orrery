use quick_xml::escape::unescape;
use quick_xml::events::Event;
use quick_xml::reader::Reader;
use thiserror::Error;

use crate::model::{
    Association, BoundaryEvent, EndEvent, ErrorEndEvent, EscalationBoundaryEvent,
    EscalationEndEvent, EscalationIntermediateThrowEvent, EventBasedGateway, EventSubProcess,
    EventSubProcessStartEvent, EventSubProcessTrigger, ExclusiveGateway, FlowElement,
    InclusiveGateway, IntermediateThrowEvent, LinkIntermediateCatchEvent,
    LinkIntermediateThrowEvent, MessageBoundaryEvent, MessageEndEvent,
    MessageIntermediateCatchEvent, MessageIntermediateThrowEvent, MessageStartEvent,
    MultiInstanceTask, ParallelGateway, ProcessDefinition, ReceiveTask, ScriptTask, SequenceFlow,
    ServiceTask, SignalBoundaryEvent, SignalEndEvent, SignalIntermediateCatchEvent,
    SignalIntermediateThrowEvent, SignalStartEvent, StartEvent, SubProcess, TerminateEndEvent,
    TextAnnotation, TimerBoundaryEvent, TimerDefinition, TimerIntermediateEvent, TimerKind,
    TimerStartEvent,
};

#[derive(Debug, Error)]
pub enum ParseError {
    #[error("XML parse error: {0}")]
    Xml(#[from] quick_xml::Error),
    #[error("Unsupported element type '{0}' — Orrery currently supports: startEvent, endEvent, serviceTask, exclusiveGateway, parallelGateway, inclusiveGateway, eventBasedGateway, subProcess, sequenceFlow")]
    UnsupportedElement(String),
    #[error("Missing required attribute '{attr}' on element '{element}'")]
    MissingAttribute { element: String, attr: String },
    #[error("No process element found in BPMN definitions")]
    NoProcess,
}

/// Context saved when entering a subprocess scope
struct SubProcessCtx {
    id: String,
    name: Option<String>,
    incoming: Vec<String>,
    outgoing: Vec<String>,
    inner_elements: Vec<FlowElement>,
    inner_flows: Vec<SequenceFlow>,
    /// True when this subprocess has triggeredByEvent="true" (event subprocess)
    is_event_subprocess: bool,
}

pub fn parse_bpmn(xml: &str) -> Result<ProcessDefinition, ParseError> {
    let mut reader = Reader::from_str(xml);
    // Do NOT trim_text globally — conditionExpression needs its internal whitespace
    // (spaces around operators like &gt;). Whitespace-only text nodes are skipped
    // below in the Event::Text handler via `text.trim().is_empty()`.
    reader.config_mut().trim_text(false);

    let mut process_id: Option<String> = None;
    let mut process_name: Option<String> = None;
    let mut elements: Vec<FlowElement> = Vec::new();
    let mut sequence_flows: Vec<SequenceFlow> = Vec::new();

    let mut current_element_id: Option<String> = None;
    let mut current_element_name: Option<String> = None;
    let mut current_element_type: Option<String> = None;
    let mut current_outgoing: Vec<String> = Vec::new();
    let mut current_incoming: Vec<String> = Vec::new();
    let mut in_process = false;
    let mut in_outgoing = false;
    let mut in_incoming = false;
    let mut current_tag: Option<String> = None;
    let mut current_seq_flow: Option<SequenceFlow> = None;
    let mut in_timer_def = false;
    let mut current_timer_duration: Option<String> = None;
    let mut current_timer_kind: Option<TimerKind> = None;
    let mut in_multi_instance = false;
    let mut multi_instance_input_ref: Option<String> = None;
    let mut multi_instance_sequential = true;
    let mut subprocess_stack: Vec<SubProcessCtx> = Vec::new();
    // For boundary events
    let mut current_boundary_attached_to: Option<String> = None;
    let mut current_boundary_type = "error".to_string();
    // For message/signal catch events
    let mut current_message_ref: Option<String> = None;
    let mut current_signal_ref: Option<String> = None;
    // For external service tasks (orrery:type="external")
    let mut current_orrery_topic: Option<String> = None;
    // For exclusiveGateway default flow
    let mut current_gateway_default: Option<String> = None;
    // Message map: BPMN message id → (message name, optional correlation key expression)
    // Populated from <message> elements at the definitions level.  These may appear
    // before OR after <process>, so a post-processing step resolves any unresolved refs.
    let mut message_map: std::collections::HashMap<String, (String, Option<String>)> =
        std::collections::HashMap::new();
    // State for parsing <message> elements at definitions level
    let mut in_message_element = false;
    let mut current_msg_id: Option<String> = None;
    let mut current_msg_name: Option<String> = None;
    let mut current_msg_ck: Option<String> = None;
    let mut in_msg_extension = false;
    // Signal map: BPMN signal id → signal name
    // Populated from <signal> elements at the definitions level.
    let mut signal_map: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    // Error definitions: BPMN error id → error code
    // Populated from <error id="..." errorCode="..."/> elements at the definitions level.
    let mut error_definitions: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    // Resolved error code for the current element (set by errorEventDefinition)
    let mut current_error_code: Option<String> = None;
    // Escalation definitions: BPMN escalation id → escalation code
    // Populated from <escalation id="..." escalationCode="..."/> elements at the definitions level.
    let mut escalation_definitions: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    // Resolved escalation code for the current element (set by escalationEventDefinition)
    let mut current_escalation_code: Option<String> = None;
    // Link name for the current link event (set by linkEventDefinition name attribute)
    let mut current_link_name: Option<String> = None;
    // Resolved correlation key for the current BPMN flow element (set by messageEventDefinition)
    let mut current_message_ck: Option<String> = None;
    // Whether the current boundary event is interrupting (from cancelActivity attribute)
    let mut current_boundary_interrupting = true;
    // For scriptTask elements
    let mut current_script_format: Option<String> = None;
    let mut current_script_body: Option<String> = None;
    let mut current_result_variable: Option<String> = None;
    let mut in_script_body = false;
    let mut in_extension_elements = false;
    // For textAnnotation elements
    let mut annotations: Vec<TextAnnotation> = Vec::new();
    let mut associations: Vec<Association> = Vec::new();
    let mut in_text_annotation = false;
    let mut current_annotation_id: Option<String> = None;
    let mut current_annotation_text: Option<String> = None;

    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let local_name = local_name_str(e.name());
                current_tag = Some(local_name.clone());

                match local_name.as_str() {
                    "message" => {
                        in_message_element = true;
                        current_msg_id = attr_value(e, "id");
                        current_msg_name = attr_value(e, "name");
                        current_msg_ck = None;
                    }
                    "signal" => {
                        // <signal id="..." name="..."> at definitions level (with child elements)
                        if let (Some(id), Some(name)) = (attr_value(e, "id"), attr_value(e, "name"))
                        {
                            signal_map.insert(id, name);
                        }
                    }
                    "error" => {
                        // <error id="..." errorCode="..."> at definitions level (with child elements)
                        if let Some(id) = attr_value(e, "id") {
                            if let Some(code) = attr_value(e, "errorCode") {
                                error_definitions.insert(id, code);
                            }
                        }
                    }
                    "escalation" => {
                        // <escalation id="..." escalationCode="..."> at definitions level
                        if let Some(id) = attr_value(e, "id") {
                            if let Some(code) = attr_value(e, "escalationCode") {
                                escalation_definitions.insert(id, code);
                            }
                        }
                    }
                    "extensionElements" if in_message_element => {
                        in_msg_extension = true;
                    }
                    "subscription" if in_msg_extension => {
                        current_msg_ck = attr_value(e, "correlationKey");
                    }
                    "process" => {
                        in_process = true;
                        process_id = attr_value(e, "id");
                        process_name = attr_value(e, "name");
                    }
                    "subProcess" if in_process => {
                        // Flush any pending element before entering subprocess scope
                        {
                            let (ae, af) = active_lists_mut(
                                &mut elements,
                                &mut sequence_flows,
                                &mut subprocess_stack,
                            );
                            flush_element(
                                ae,
                                &mut current_element_type,
                                &mut current_element_id,
                                &mut current_element_name,
                                &mut current_outgoing,
                                &mut current_incoming,
                                &mut current_timer_duration,
                                &mut current_timer_kind,
                                &mut multi_instance_input_ref,
                                multi_instance_sequential,
                                None,
                                "",
                                &mut current_message_ref,
                                &mut current_signal_ref,
                                current_orrery_topic.take(),
                                current_gateway_default.take(),
                                current_message_ck.take(),
                                current_boundary_interrupting,
                                &mut current_script_format,
                                &mut current_script_body,
                                &mut current_result_variable,
                                current_error_code.take(),
                                &mut current_escalation_code,
                                &mut current_link_name,
                            )?;
                            let _ = af; // flows not affected by flush
                        }
                        let id =
                            attr_value(e, "id").ok_or_else(|| ParseError::MissingAttribute {
                                element: "subProcess".to_string(),
                                attr: "id".to_string(),
                            })?;
                        let name = attr_value(e, "name");
                        let is_event_subprocess = attr_value(e, "triggeredByEvent")
                            .map(|v| v == "true")
                            .unwrap_or(false);
                        subprocess_stack.push(SubProcessCtx {
                            id,
                            name,
                            incoming: Vec::new(),
                            outgoing: Vec::new(),
                            inner_elements: Vec::new(),
                            inner_flows: Vec::new(),
                            is_event_subprocess,
                        });
                        current_outgoing = Vec::new();
                        current_incoming = Vec::new();
                    }
                    "startEvent"
                    | "endEvent"
                    | "serviceTask"
                    | "scriptTask"
                    | "exclusiveGateway"
                    | "parallelGateway"
                    | "inclusiveGateway"
                    | "eventBasedGateway"
                    | "intermediateCatchEvent"
                    | "intermediateThrowEvent"
                        if in_process =>
                    {
                        {
                            let (ae, af) = active_lists_mut(
                                &mut elements,
                                &mut sequence_flows,
                                &mut subprocess_stack,
                            );
                            flush_element(
                                ae,
                                &mut current_element_type,
                                &mut current_element_id,
                                &mut current_element_name,
                                &mut current_outgoing,
                                &mut current_incoming,
                                &mut current_timer_duration,
                                &mut current_timer_kind,
                                &mut multi_instance_input_ref,
                                multi_instance_sequential,
                                None,
                                "",
                                &mut current_message_ref,
                                &mut current_signal_ref,
                                current_orrery_topic.take(),
                                current_gateway_default.take(),
                                current_message_ck.take(),
                                current_boundary_interrupting,
                                &mut current_script_format,
                                &mut current_script_body,
                                &mut current_result_variable,
                                current_error_code.take(),
                                &mut current_escalation_code,
                                &mut current_link_name,
                            )?;
                            let _ = af;
                        }
                        let id =
                            attr_value(e, "id").ok_or_else(|| ParseError::MissingAttribute {
                                element: local_name.clone(),
                                attr: "id".to_string(),
                            })?;
                        // If this is a startEvent inside an event subprocess, mark it specially
                        let is_esp_start = local_name == "startEvent"
                            && subprocess_stack
                                .last()
                                .map(|ctx| ctx.is_event_subprocess)
                                .unwrap_or(false);
                        if is_esp_start {
                            current_element_type = Some("espStartEvent".to_string());
                            // isInterrupting defaults to true per BPMN spec
                            current_boundary_interrupting = attr_value(e, "isInterrupting")
                                .map(|v| v != "false")
                                .unwrap_or(true);
                        } else {
                            current_element_type = Some(local_name.clone());
                            current_boundary_interrupting = true;
                        }
                        current_element_id = Some(id);
                        current_element_name = attr_value(e, "name");
                        current_outgoing = Vec::new();
                        current_incoming = Vec::new();
                        current_timer_duration = None;
                        multi_instance_input_ref = None;
                        current_message_ck = None;
                        // Read orrery:type and orrery:topic for external service tasks
                        if local_name == "serviceTask" {
                            if attr_value(e, "type").as_deref() == Some("external") {
                                current_orrery_topic = attr_value(e, "topic");
                            } else {
                                current_orrery_topic = None;
                            }
                        }
                        if local_name == "scriptTask" {
                            current_script_format = attr_value(e, "scriptFormat");
                            current_script_body = None;
                            // camunda:resultVariable is on the scriptTask element itself
                            current_result_variable = attr_value(e, "resultVariable");
                        }
                        if local_name == "exclusiveGateway" || local_name == "inclusiveGateway" {
                            current_gateway_default = attr_value(e, "default");
                        } else if local_name != "serviceTask" {
                            current_gateway_default = None;
                        }
                    }
                    "receiveTask" if in_process => {
                        {
                            let (ae, af) = active_lists_mut(
                                &mut elements,
                                &mut sequence_flows,
                                &mut subprocess_stack,
                            );
                            flush_element(
                                ae,
                                &mut current_element_type,
                                &mut current_element_id,
                                &mut current_element_name,
                                &mut current_outgoing,
                                &mut current_incoming,
                                &mut current_timer_duration,
                                &mut current_timer_kind,
                                &mut multi_instance_input_ref,
                                multi_instance_sequential,
                                None,
                                "",
                                &mut current_message_ref,
                                &mut current_signal_ref,
                                current_orrery_topic.take(),
                                current_gateway_default.take(),
                                current_message_ck.take(),
                                current_boundary_interrupting,
                                &mut current_script_format,
                                &mut current_script_body,
                                &mut current_result_variable,
                                current_error_code.take(),
                                &mut current_escalation_code,
                                &mut current_link_name,
                            )?;
                            let _ = af;
                        }
                        let id = attr_value(e, "id").unwrap_or_default();
                        let name = attr_value(e, "name");
                        current_element_id = Some(id);
                        current_element_name = name;
                        current_element_type = Some("receiveTask".to_string());
                        current_outgoing = Vec::new();
                        current_incoming = Vec::new();
                        current_message_ck = None;
                        // messageRef is an attribute on the element itself — resolve via message_map
                        if let Some(mref) = attr_value(e, "messageRef") {
                            let (resolved_name, resolved_ck) = message_map
                                .get(&mref)
                                .map(|(n, ck)| (n.clone(), ck.clone()))
                                .unwrap_or_else(|| (mref.clone(), None));
                            current_message_ref = Some(resolved_name);
                            current_message_ck = resolved_ck;
                        }
                    }
                    "boundaryEvent" if in_process => {
                        // Flush any pending element
                        {
                            let (ae, af) = active_lists_mut(
                                &mut elements,
                                &mut sequence_flows,
                                &mut subprocess_stack,
                            );
                            flush_element(
                                ae,
                                &mut current_element_type,
                                &mut current_element_id,
                                &mut current_element_name,
                                &mut current_outgoing,
                                &mut current_incoming,
                                &mut current_timer_duration,
                                &mut current_timer_kind,
                                &mut multi_instance_input_ref,
                                multi_instance_sequential,
                                None,
                                "",
                                &mut current_message_ref,
                                &mut current_signal_ref,
                                current_orrery_topic.take(),
                                current_gateway_default.take(),
                                current_message_ck.take(),
                                current_boundary_interrupting,
                                &mut current_script_format,
                                &mut current_script_body,
                                &mut current_result_variable,
                                current_error_code.take(),
                                &mut current_escalation_code,
                                &mut current_link_name,
                            )?;
                            let _ = af;
                        }
                        let id =
                            attr_value(e, "id").ok_or_else(|| ParseError::MissingAttribute {
                                element: "boundaryEvent".to_string(),
                                attr: "id".to_string(),
                            })?;
                        let attached = attr_value(e, "attachedToRef").unwrap_or_default();
                        current_element_type = Some("boundaryEvent".to_string());
                        current_element_id = Some(id);
                        current_element_name = attr_value(e, "name");
                        current_outgoing = Vec::new();
                        current_incoming = Vec::new();
                        current_boundary_attached_to = Some(attached);
                        current_boundary_type = "error".to_string(); // default, overridden by child def
                        current_message_ck = None;
                        current_boundary_interrupting = attr_value(e, "cancelActivity")
                            .map(|v| v != "false")
                            .unwrap_or(true);
                    }
                    "errorEventDefinition" if in_process => {
                        current_boundary_type = "error".to_string();
                        // Resolve errorRef to error code; if not found yet, store raw ref for post-processing
                        current_error_code = attr_value(e, "errorRef")
                            .map(|eref| error_definitions.get(&eref).cloned().unwrap_or(eref));
                        match current_element_type.as_deref() {
                            Some("endEvent") => {
                                current_element_type = Some("errorEndEvent".to_string());
                            }
                            Some("espStartEvent") => {
                                current_element_type = Some("espStartEvent_error".to_string());
                            }
                            _ => {}
                        }
                    }
                    "terminateEventDefinition" if in_process => {
                        if current_element_type.as_deref() == Some("endEvent") {
                            current_element_type = Some("terminateEndEvent".to_string());
                        }
                    }
                    "escalationEventDefinition" if in_process => {
                        current_escalation_code = attr_value(e, "escalationRef")
                            .map(|eref| escalation_definitions.get(&eref).cloned().unwrap_or(eref));
                        match current_element_type.as_deref() {
                            Some("endEvent") => {
                                current_element_type = Some("escalationEndEvent".to_string());
                            }
                            Some("intermediateThrowEvent") => {
                                current_element_type = Some("escalationThrowEvent".to_string());
                            }
                            Some("boundaryEvent") => {
                                current_boundary_type = "escalation".to_string();
                            }
                            Some("espStartEvent") => {
                                current_element_type = Some("espStartEvent_escalation".to_string());
                            }
                            _ => {}
                        }
                    }
                    "linkEventDefinition" if in_process => {
                        current_link_name = attr_value(e, "name");
                        match current_element_type.as_deref() {
                            Some("intermediateThrowEvent") => {
                                current_element_type = Some("linkThrowEvent".to_string());
                            }
                            Some("intermediateCatchEvent") => {
                                current_element_type = Some("linkCatchEvent".to_string());
                            }
                            _ => {}
                        }
                    }
                    "timerEventDefinition" if in_process => {
                        in_timer_def = true;
                        match current_element_type.as_deref() {
                            Some("boundaryEvent") => {
                                current_boundary_type = "timer".to_string();
                            }
                            Some("espStartEvent") => {
                                current_element_type = Some("espStartEvent_timer".to_string());
                            }
                            _ => {}
                        }
                    }
                    "messageEventDefinition" if in_process => {
                        let mref = attr_value(e, "messageRef").unwrap_or_default();
                        let (resolved_name, resolved_ck) = message_map
                            .get(&mref)
                            .map(|(n, ck)| (n.clone(), ck.clone()))
                            .unwrap_or_else(|| (mref.clone(), None));
                        current_message_ref = Some(resolved_name);
                        current_message_ck = resolved_ck;
                        match current_element_type.as_deref() {
                            Some("intermediateCatchEvent") => {
                                current_element_type = Some("messageCatchEvent".to_string());
                            }
                            Some("startEvent") => {
                                current_element_type = Some("messageStartEvent".to_string());
                            }
                            Some("endEvent") => {
                                current_element_type = Some("messageEndEvent".to_string());
                            }
                            Some("intermediateThrowEvent") => {
                                current_element_type = Some("messageThrowEvent".to_string());
                            }
                            Some("boundaryEvent") => {
                                current_boundary_type = "message".to_string();
                            }
                            Some("espStartEvent") => {
                                current_element_type = Some("espStartEvent_message".to_string());
                            }
                            _ => {}
                        }
                    }
                    "signalEventDefinition" if in_process => {
                        let sref = attr_value(e, "signalRef");
                        // Resolve signalRef via signal_map if available
                        current_signal_ref = sref.map(|s| signal_map.get(&s).cloned().unwrap_or(s));
                        match current_element_type.as_deref() {
                            Some("intermediateCatchEvent") => {
                                current_element_type = Some("signalCatchEvent".to_string());
                            }
                            Some("startEvent") => {
                                current_element_type = Some("signalStartEvent".to_string());
                            }
                            Some("endEvent") => {
                                current_element_type = Some("signalEndEvent".to_string());
                            }
                            Some("intermediateThrowEvent") => {
                                current_element_type = Some("signalThrowEvent".to_string());
                            }
                            Some("boundaryEvent") => {
                                current_boundary_type = "signal".to_string();
                            }
                            Some("espStartEvent") => {
                                current_element_type = Some("espStartEvent_signal".to_string());
                            }
                            _ => {}
                        }
                    }
                    "multiInstanceLoopCharacteristics" if in_process => {
                        in_multi_instance = true;
                        let is_seq = attr_value(e, "isSequential")
                            .map(|v| v == "true")
                            .unwrap_or(true);
                        multi_instance_sequential = is_seq;
                        multi_instance_input_ref = None;
                    }
                    "sequenceFlow" if in_process => {
                        let id =
                            attr_value(e, "id").ok_or_else(|| ParseError::MissingAttribute {
                                element: "sequenceFlow".to_string(),
                                attr: "id".to_string(),
                            })?;
                        let source = attr_value(e, "sourceRef").ok_or_else(|| {
                            ParseError::MissingAttribute {
                                element: "sequenceFlow".to_string(),
                                attr: "sourceRef".to_string(),
                            }
                        })?;
                        let target = attr_value(e, "targetRef").ok_or_else(|| {
                            ParseError::MissingAttribute {
                                element: "sequenceFlow".to_string(),
                                attr: "targetRef".to_string(),
                            }
                        })?;
                        current_seq_flow = Some(SequenceFlow {
                            id,
                            source_ref: source,
                            target_ref: target,
                            condition_expression: None,
                        });
                    }
                    "outgoing" if in_process => {
                        in_outgoing = true;
                    }
                    "incoming" if in_process => {
                        in_incoming = true;
                    }
                    "extensionElements" | "documentation" => {
                        if local_name == "extensionElements" {
                            in_extension_elements = true;
                        }
                    }
                    "script"
                        if in_process
                            && current_element_type.as_deref() == Some("scriptTask")
                            && !in_extension_elements =>
                    {
                        in_script_body = true;
                    }
                    "script"
                        if in_extension_elements
                            && current_element_type.as_deref() == Some("scriptTask") =>
                    {
                        // <zeebe:script resultVariable="sum"> as a Start element
                        if let Some(rv) = attr_value(e, "resultVariable") {
                            current_result_variable = Some(rv);
                        }
                    }
                    "textAnnotation" if in_process => {
                        current_annotation_id = attr_value(e, "id");
                        current_annotation_text = None;
                        in_text_annotation = true;
                    }
                    "definitions" => {}
                    "conditionExpression" => {}
                    other
                        if in_process
                            && current_element_type.is_none()
                            && current_seq_flow.is_none()
                            && process_id.is_some()
                            && subprocess_stack.is_empty()
                            && !in_text_annotation
                            && !is_diagram_element(other) =>
                    {
                        return Err(ParseError::UnsupportedElement(other.to_string()));
                    }
                    _ => {}
                }
            }
            Ok(Event::Empty(ref e)) => {
                let local_name = local_name_str(e.name());
                match local_name.as_str() {
                    "startEvent"
                    | "endEvent"
                    | "serviceTask"
                    | "exclusiveGateway"
                    | "parallelGateway"
                    | "intermediateCatchEvent"
                    | "intermediateThrowEvent"
                        if in_process =>
                    {
                        {
                            let (ae, af) = active_lists_mut(
                                &mut elements,
                                &mut sequence_flows,
                                &mut subprocess_stack,
                            );
                            flush_element(
                                ae,
                                &mut current_element_type,
                                &mut current_element_id,
                                &mut current_element_name,
                                &mut current_outgoing,
                                &mut current_incoming,
                                &mut current_timer_duration,
                                &mut current_timer_kind,
                                &mut multi_instance_input_ref,
                                multi_instance_sequential,
                                None,
                                "",
                                &mut current_message_ref,
                                &mut current_signal_ref,
                                current_orrery_topic.take(),
                                current_gateway_default.take(),
                                current_message_ck.take(),
                                current_boundary_interrupting,
                                &mut current_script_format,
                                &mut current_script_body,
                                &mut current_result_variable,
                                current_error_code.take(),
                                &mut current_escalation_code,
                                &mut current_link_name,
                            )?;
                            let _ = af;
                        }
                        let id =
                            attr_value(e, "id").ok_or_else(|| ParseError::MissingAttribute {
                                element: local_name.clone(),
                                attr: "id".to_string(),
                            })?;
                        let name = attr_value(e, "name");
                        let el = match local_name.as_str() {
                            "startEvent" => FlowElement::StartEvent(StartEvent {
                                id,
                                name,
                                outgoing: vec![],
                            }), // Self-closing startEvent never has timer child elements
                            "endEvent" => FlowElement::EndEvent(EndEvent {
                                id,
                                name,
                                outgoing: vec![],
                            }),
                            "serviceTask" => {
                                let topic = if attr_value(e, "type").as_deref() == Some("external")
                                {
                                    attr_value(e, "topic")
                                } else {
                                    None
                                };
                                FlowElement::ServiceTask(ServiceTask {
                                    id,
                                    name,
                                    outgoing: vec![],
                                    topic,
                                })
                            }
                            "exclusiveGateway" => FlowElement::ExclusiveGateway(ExclusiveGateway {
                                id,
                                name,
                                outgoing: vec![],
                                default: attr_value(e, "default"),
                            }),
                            "parallelGateway" => FlowElement::ParallelGateway(ParallelGateway {
                                id,
                                name,
                                incoming: vec![],
                                outgoing: vec![],
                            }),
                            "inclusiveGateway" => FlowElement::InclusiveGateway(InclusiveGateway {
                                id,
                                name,
                                incoming: vec![],
                                outgoing: vec![],
                                default: attr_value(e, "default"),
                            }),
                            "eventBasedGateway" => {
                                FlowElement::EventBasedGateway(EventBasedGateway {
                                    id,
                                    name,
                                    incoming: vec![],
                                    outgoing: vec![],
                                })
                            }
                            "intermediateCatchEvent" => {
                                FlowElement::TimerIntermediateEvent(TimerIntermediateEvent {
                                    id,
                                    name,
                                    outgoing: vec![],
                                    timer: None,
                                })
                            }
                            "intermediateThrowEvent" => {
                                FlowElement::IntermediateThrowEvent(IntermediateThrowEvent {
                                    id,
                                    name,
                                    outgoing: vec![],
                                })
                            }
                            _ => unreachable!(),
                        };
                        if let Some(ctx) = subprocess_stack.last_mut() {
                            ctx.inner_elements.push(el);
                        } else {
                            elements.push(el);
                        }
                    }
                    "sequenceFlow" if in_process => {
                        let id =
                            attr_value(e, "id").ok_or_else(|| ParseError::MissingAttribute {
                                element: "sequenceFlow".to_string(),
                                attr: "id".to_string(),
                            })?;
                        let source = attr_value(e, "sourceRef").ok_or_else(|| {
                            ParseError::MissingAttribute {
                                element: "sequenceFlow".to_string(),
                                attr: "sourceRef".to_string(),
                            }
                        })?;
                        let target = attr_value(e, "targetRef").ok_or_else(|| {
                            ParseError::MissingAttribute {
                                element: "sequenceFlow".to_string(),
                                attr: "targetRef".to_string(),
                            }
                        })?;
                        let flow = SequenceFlow {
                            id,
                            source_ref: source,
                            target_ref: target,
                            condition_expression: None,
                        };
                        if let Some(ctx) = subprocess_stack.last_mut() {
                            ctx.inner_flows.push(flow);
                        } else {
                            sequence_flows.push(flow);
                        }
                    }
                    "message" => {
                        // Self-closing <message id="..." name="..."/> at definitions level
                        if let (Some(id), Some(name)) = (attr_value(e, "id"), attr_value(e, "name"))
                        {
                            message_map.insert(id, (name, None));
                        }
                    }
                    "signal" => {
                        // Self-closing <signal id="..." name="..."/> at definitions level
                        if let (Some(id), Some(name)) = (attr_value(e, "id"), attr_value(e, "name"))
                        {
                            signal_map.insert(id, name);
                        }
                    }
                    "error" => {
                        // Self-closing <error id="..." errorCode="..."/> at definitions level
                        if let Some(id) = attr_value(e, "id") {
                            if let Some(code) = attr_value(e, "errorCode") {
                                error_definitions.insert(id, code);
                            }
                        }
                    }
                    "escalation" => {
                        // Self-closing <escalation id="..." escalationCode="..."/> at definitions level
                        if let Some(id) = attr_value(e, "id") {
                            if let Some(code) = attr_value(e, "escalationCode") {
                                escalation_definitions.insert(id, code);
                            }
                        }
                    }
                    "subscription" if in_msg_extension => {
                        current_msg_ck = attr_value(e, "correlationKey");
                    }
                    "messageEventDefinition" if in_process => {
                        let mref = attr_value(e, "messageRef").unwrap_or_default();
                        let (resolved_name, resolved_ck) = message_map
                            .get(&mref)
                            .map(|(n, ck)| (n.clone(), ck.clone()))
                            .unwrap_or_else(|| (mref.clone(), None));
                        current_message_ref = Some(resolved_name);
                        current_message_ck = resolved_ck;
                        match current_element_type.as_deref() {
                            Some("intermediateCatchEvent") => {
                                current_element_type = Some("messageCatchEvent".to_string());
                            }
                            Some("startEvent") => {
                                current_element_type = Some("messageStartEvent".to_string());
                            }
                            Some("endEvent") => {
                                current_element_type = Some("messageEndEvent".to_string());
                            }
                            Some("intermediateThrowEvent") => {
                                current_element_type = Some("messageThrowEvent".to_string());
                            }
                            Some("boundaryEvent") => {
                                current_boundary_type = "message".to_string();
                            }
                            Some("espStartEvent") => {
                                current_element_type = Some("espStartEvent_message".to_string());
                            }
                            _ => {}
                        }
                    }
                    "signalEventDefinition" if in_process => {
                        let sref = attr_value(e, "signalRef");
                        // Resolve signalRef via signal_map if available
                        current_signal_ref = sref.map(|s| signal_map.get(&s).cloned().unwrap_or(s));
                        match current_element_type.as_deref() {
                            Some("intermediateCatchEvent") => {
                                current_element_type = Some("signalCatchEvent".to_string());
                            }
                            Some("startEvent") => {
                                current_element_type = Some("signalStartEvent".to_string());
                            }
                            Some("endEvent") => {
                                current_element_type = Some("signalEndEvent".to_string());
                            }
                            Some("intermediateThrowEvent") => {
                                current_element_type = Some("signalThrowEvent".to_string());
                            }
                            Some("boundaryEvent") => {
                                current_boundary_type = "signal".to_string();
                            }
                            Some("espStartEvent") => {
                                current_element_type = Some("espStartEvent_signal".to_string());
                            }
                            _ => {}
                        }
                    }
                    "errorEventDefinition" if in_process => {
                        current_boundary_type = "error".to_string();
                        // Resolve errorRef to error code; if not found yet, store raw ref for post-processing
                        current_error_code = attr_value(e, "errorRef")
                            .map(|eref| error_definitions.get(&eref).cloned().unwrap_or(eref));
                        match current_element_type.as_deref() {
                            Some("endEvent") => {
                                current_element_type = Some("errorEndEvent".to_string());
                            }
                            Some("espStartEvent") => {
                                current_element_type = Some("espStartEvent_error".to_string());
                            }
                            _ => {}
                        }
                    }
                    "terminateEventDefinition" if in_process => {
                        if current_element_type.as_deref() == Some("endEvent") {
                            current_element_type = Some("terminateEndEvent".to_string());
                        }
                    }
                    "escalationEventDefinition" if in_process => {
                        current_escalation_code = attr_value(e, "escalationRef")
                            .map(|eref| escalation_definitions.get(&eref).cloned().unwrap_or(eref));
                        match current_element_type.as_deref() {
                            Some("endEvent") => {
                                current_element_type = Some("escalationEndEvent".to_string());
                            }
                            Some("intermediateThrowEvent") => {
                                current_element_type = Some("escalationThrowEvent".to_string());
                            }
                            Some("boundaryEvent") => {
                                current_boundary_type = "escalation".to_string();
                            }
                            Some("espStartEvent") => {
                                current_element_type = Some("espStartEvent_escalation".to_string());
                            }
                            _ => {}
                        }
                    }
                    "linkEventDefinition" if in_process => {
                        current_link_name = attr_value(e, "name");
                        match current_element_type.as_deref() {
                            Some("intermediateThrowEvent") => {
                                current_element_type = Some("linkThrowEvent".to_string());
                            }
                            Some("intermediateCatchEvent") => {
                                current_element_type = Some("linkCatchEvent".to_string());
                            }
                            _ => {}
                        }
                    }
                    "script" if current_element_type.as_deref() == Some("scriptTask") => {
                        // Self-closing <zeebe:script resultVariable="sum" />
                        if let Some(rv) = attr_value(e, "resultVariable") {
                            current_result_variable = Some(rv);
                        }
                    }
                    "association" if in_process => {
                        if let (Some(id), Some(src), Some(tgt)) = (
                            attr_value(e, "id"),
                            attr_value(e, "sourceRef"),
                            attr_value(e, "targetRef"),
                        ) {
                            associations.push(Association {
                                id,
                                source_ref: src,
                                target_ref: tgt,
                            });
                        }
                    }
                    other
                        if in_process
                            && current_element_type.is_none()
                            && current_seq_flow.is_none()
                            && process_id.is_some()
                            && subprocess_stack.is_empty()
                            && !in_text_annotation
                            && !is_diagram_element(other) =>
                    {
                        return Err(ParseError::UnsupportedElement(other.to_string()));
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(e)) => {
                let raw = e.decode().unwrap_or_default().to_string();
                // For conditionExpression and textAnnotation text we preserve internal whitespace.
                // For all other tags we trim to ignore insignificant whitespace.
                let text = if current_tag.as_deref() == Some("conditionExpression")
                    || (in_text_annotation && current_tag.as_deref() == Some("text"))
                {
                    raw
                } else {
                    raw.trim().to_string()
                };
                if text.trim().is_empty() {
                    // skip
                } else if in_text_annotation {
                    match current_annotation_text {
                        Some(ref mut existing) => existing.push_str(&text),
                        None => current_annotation_text = Some(text),
                    }
                } else if in_script_body {
                    match current_script_body {
                        Some(ref mut existing) => existing.push_str(&text),
                        None => current_script_body = Some(text),
                    }
                } else if current_tag.as_deref() == Some("conditionExpression") {
                    if let Some(ref mut flow) = current_seq_flow {
                        let unescaped = unescape(&text).map(|c| c.into_owned()).unwrap_or(text);
                        // Append: quick_xml may emit multiple Text events for content with XML entities
                        match flow.condition_expression {
                            Some(ref mut existing) => existing.push_str(&unescaped),
                            None => flow.condition_expression = Some(unescaped),
                        }
                    }
                } else if current_tag.as_deref() == Some("timeDuration") && in_timer_def {
                    current_timer_kind = Some(TimerKind::Duration);
                    current_timer_duration = Some(text);
                } else if current_tag.as_deref() == Some("timeDate") && in_timer_def {
                    current_timer_kind = Some(TimerKind::Date);
                    current_timer_duration = Some(text);
                } else if current_tag.as_deref() == Some("timeCycle") && in_timer_def {
                    current_timer_kind = Some(TimerKind::Cycle);
                    current_timer_duration = Some(text);
                } else if current_tag.as_deref() == Some("loopDataInputRef") && in_multi_instance {
                    multi_instance_input_ref = Some(text);
                } else if in_outgoing {
                    if current_element_id.is_some() {
                        current_outgoing.push(text);
                    } else if let Some(ctx) = subprocess_stack.last_mut() {
                        ctx.outgoing.push(text);
                    }
                } else if in_incoming {
                    if current_element_id.is_some() {
                        current_incoming.push(text);
                    } else if let Some(ctx) = subprocess_stack.last_mut() {
                        ctx.incoming.push(text);
                    }
                }
            }
            Ok(Event::GeneralRef(ref e)) => {
                // Resolve predefined XML entities within conditionExpression
                if current_tag.as_deref() == Some("conditionExpression") {
                    if let Some(ref mut flow) = current_seq_flow {
                        let name = e.decode().unwrap_or_default();
                        let resolved = match name.as_ref() {
                            "gt" => ">",
                            "lt" => "<",
                            "amp" => "&",
                            "quot" => "\"",
                            "apos" => "'",
                            _ => "",
                        };
                        if !resolved.is_empty() {
                            match flow.condition_expression {
                                Some(ref mut existing) => existing.push_str(resolved),
                                None => flow.condition_expression = Some(resolved.to_string()),
                            }
                        }
                    }
                } else if in_text_annotation && current_tag.as_deref() == Some("text") {
                    // Resolve predefined XML entities within annotation text
                    let name = e.decode().unwrap_or_default();
                    let resolved = match name.as_ref() {
                        "gt" => ">",
                        "lt" => "<",
                        "amp" => "&",
                        "quot" => "\"",
                        "apos" => "'",
                        _ => "",
                    };
                    if !resolved.is_empty() {
                        match current_annotation_text {
                            Some(ref mut existing) => existing.push_str(resolved),
                            None => current_annotation_text = Some(resolved.to_string()),
                        }
                    }
                }
            }
            Ok(Event::End(ref e)) => {
                let local = local_name_str(e.name());
                current_tag = None;
                match local.as_str() {
                    "script" if in_script_body => {
                        in_script_body = false;
                    }
                    "extensionElements" if in_extension_elements && !in_msg_extension => {
                        in_extension_elements = false;
                    }
                    "outgoing" => {
                        in_outgoing = false;
                    }
                    "incoming" => {
                        in_incoming = false;
                    }
                    "timerEventDefinition" => {
                        in_timer_def = false;
                    }
                    "message" => {
                        if let (Some(id), Some(name)) =
                            (current_msg_id.take(), current_msg_name.take())
                        {
                            message_map.insert(id, (name, current_msg_ck.take()));
                        }
                        in_message_element = false;
                        in_msg_extension = false;
                    }
                    "extensionElements" if in_msg_extension => {
                        in_msg_extension = false;
                    }
                    "multiInstanceLoopCharacteristics" => {
                        in_multi_instance = false;
                        if current_element_type.as_deref() == Some("serviceTask")
                            && multi_instance_input_ref.is_some()
                        {
                            current_element_type = Some("multiInstanceTask".to_string());
                        }
                    }
                    "subProcess" => {
                        // Flush any pending inner element first
                        {
                            let (ae, af) = active_lists_mut(
                                &mut elements,
                                &mut sequence_flows,
                                &mut subprocess_stack,
                            );
                            flush_element(
                                ae,
                                &mut current_element_type,
                                &mut current_element_id,
                                &mut current_element_name,
                                &mut current_outgoing,
                                &mut current_incoming,
                                &mut current_timer_duration,
                                &mut current_timer_kind,
                                &mut multi_instance_input_ref,
                                multi_instance_sequential,
                                None,
                                "",
                                &mut current_message_ref,
                                &mut current_signal_ref,
                                current_orrery_topic.take(),
                                current_gateway_default.take(),
                                current_message_ck.take(),
                                current_boundary_interrupting,
                                &mut current_script_format,
                                &mut current_script_body,
                                &mut current_result_variable,
                                current_error_code.take(),
                                &mut current_escalation_code,
                                &mut current_link_name,
                            )?;
                            let _ = af;
                        }
                        // Pop subprocess context and create SubProcess or EventSubProcess element
                        if let Some(ctx) = subprocess_stack.pop() {
                            let sp = if ctx.is_event_subprocess {
                                FlowElement::EventSubProcess(EventSubProcess {
                                    id: ctx.id,
                                    name: ctx.name,
                                    inner_elements: ctx.inner_elements,
                                    inner_flows: ctx.inner_flows,
                                })
                            } else {
                                FlowElement::SubProcess(SubProcess {
                                    id: ctx.id,
                                    name: ctx.name,
                                    incoming: ctx.incoming,
                                    outgoing: ctx.outgoing,
                                    inner_elements: ctx.inner_elements,
                                    inner_flows: ctx.inner_flows,
                                })
                            };
                            if let Some(outer) = subprocess_stack.last_mut() {
                                outer.inner_elements.push(sp);
                            } else {
                                elements.push(sp);
                            }
                        }
                    }
                    "startEvent"
                    | "endEvent"
                    | "serviceTask"
                    | "scriptTask"
                    | "multiInstanceTask"
                    | "exclusiveGateway"
                    | "parallelGateway"
                    | "inclusiveGateway"
                    | "eventBasedGateway"
                    | "intermediateCatchEvent"
                    | "intermediateThrowEvent"
                    | "messageCatchEvent"
                    | "signalCatchEvent"
                    | "boundaryEvent"
                    | "receiveTask" => {
                        let boundary_attached = current_boundary_attached_to.take();
                        let boundary_type_snap = current_boundary_type.clone();
                        let boundary_interrupting_snap = current_boundary_interrupting;
                        let message_ck_snap = current_message_ck.take();
                        let (ae, af) = active_lists_mut(
                            &mut elements,
                            &mut sequence_flows,
                            &mut subprocess_stack,
                        );
                        flush_element(
                            ae,
                            &mut current_element_type,
                            &mut current_element_id,
                            &mut current_element_name,
                            &mut current_outgoing,
                            &mut current_incoming,
                            &mut current_timer_duration,
                            &mut current_timer_kind,
                            &mut multi_instance_input_ref,
                            multi_instance_sequential,
                            boundary_attached,
                            &boundary_type_snap,
                            &mut current_message_ref,
                            &mut current_signal_ref,
                            current_orrery_topic.take(),
                            current_gateway_default.take(),
                            message_ck_snap,
                            boundary_interrupting_snap,
                            &mut current_script_format,
                            &mut current_script_body,
                            &mut current_result_variable,
                            current_error_code.take(),
                            &mut current_escalation_code,
                            &mut current_link_name,
                        )?;
                        let _ = af;
                    }
                    "sequenceFlow" => {
                        if let Some(flow) = current_seq_flow.take() {
                            if let Some(ctx) = subprocess_stack.last_mut() {
                                ctx.inner_flows.push(flow);
                            } else {
                                sequence_flows.push(flow);
                            }
                        }
                    }
                    "textAnnotation" => {
                        if let Some(id) = current_annotation_id.take() {
                            annotations.push(TextAnnotation {
                                id,
                                text: current_annotation_text.take().unwrap_or_default(),
                            });
                        }
                        current_annotation_text = None;
                        in_text_annotation = false;
                    }
                    "process" => {
                        in_process = false;
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(ParseError::Xml(e)),
            _ => {}
        }
        buf.clear();
    }

    let id = process_id.ok_or(ParseError::NoProcess)?;

    // Post-process: resolve any message_name fields that still hold a raw BPMN
    // message ID (e.g. "Message_2doo5s0") because the <message> definition appeared
    // after the <process> block in the XML.  Walk all elements (including those
    // nested inside SubProcesses) and replace with the human-readable name.
    fn resolve_message_names(
        elements: &mut [FlowElement],
        message_map: &std::collections::HashMap<String, (String, Option<String>)>,
    ) {
        for elem in elements.iter_mut() {
            match elem {
                FlowElement::MessageStartEvent(e) => {
                    if let Some((name, _)) = message_map.get(&e.message_name) {
                        e.message_name = name.clone();
                    }
                }
                FlowElement::MessageEndEvent(e) => {
                    if let Some((name, _)) = message_map.get(&e.message_name) {
                        e.message_name = name.clone();
                    }
                }
                FlowElement::MessageIntermediateCatchEvent(e) => {
                    if let Some((name, ck)) = message_map.get(&e.message_name) {
                        e.message_name = name.clone();
                        if e.correlation_key.is_none() {
                            e.correlation_key = ck.clone();
                        }
                    }
                }
                FlowElement::ReceiveTask(e) => {
                    if let Some((name, ck)) = message_map.get(&e.message_name) {
                        e.message_name = name.clone();
                        if e.correlation_key.is_none() {
                            e.correlation_key = ck.clone();
                        }
                    }
                }
                FlowElement::MessageIntermediateThrowEvent(e) => {
                    if let Some((name, _)) = message_map.get(&e.message_name) {
                        e.message_name = name.clone();
                    }
                }
                FlowElement::MessageBoundaryEvent(e) => {
                    if let Some((name, ck)) = message_map.get(&e.message_name) {
                        e.message_name = name.clone();
                        if e.correlation_key.is_none() {
                            e.correlation_key = ck.clone();
                        }
                    }
                }
                FlowElement::SubProcess(sp) => {
                    resolve_message_names(&mut sp.inner_elements, message_map);
                }
                _ => {}
            }
        }
    }
    resolve_message_names(&mut elements, &message_map);

    // Post-process: resolve signal_ref fields that still hold a raw BPMN signal ID
    // because the <signal> definition appeared after the <process> block.
    fn resolve_signal_names(
        elements: &mut [FlowElement],
        signal_map: &std::collections::HashMap<String, String>,
    ) {
        for elem in elements.iter_mut() {
            match elem {
                FlowElement::SignalStartEvent(e) => {
                    if let Some(name) = signal_map.get(&e.signal_ref) {
                        e.signal_ref = name.clone();
                    }
                }
                FlowElement::SignalIntermediateCatchEvent(e) => {
                    if let Some(name) = signal_map.get(&e.signal_ref) {
                        e.signal_ref = name.clone();
                    }
                }
                FlowElement::SignalIntermediateThrowEvent(e) => {
                    if let Some(name) = signal_map.get(&e.signal_ref) {
                        e.signal_ref = name.clone();
                    }
                }
                FlowElement::SignalEndEvent(e) => {
                    if let Some(name) = signal_map.get(&e.signal_ref) {
                        e.signal_ref = name.clone();
                    }
                }
                FlowElement::SignalBoundaryEvent(e) => {
                    if let Some(name) = signal_map.get(&e.signal_ref) {
                        e.signal_ref = name.clone();
                    }
                }
                FlowElement::SubProcess(sp) => {
                    resolve_signal_names(&mut sp.inner_elements, signal_map);
                }
                _ => {}
            }
        }
    }
    resolve_signal_names(&mut elements, &signal_map);

    // Post-process: resolve error references that weren't resolved inline
    // (when <error> definitions appear after <process>)
    fn resolve_error_codes(
        elements: &mut [FlowElement],
        error_defs: &std::collections::HashMap<String, String>,
    ) {
        for elem in elements.iter_mut() {
            match elem {
                FlowElement::ErrorEndEvent(e) => {
                    if let Some(ref code) = e.error_code {
                        if let Some(resolved) = error_defs.get(code) {
                            e.error_code = Some(resolved.clone());
                        }
                    }
                }
                FlowElement::BoundaryEvent(b) if b.event_type == "error" => {
                    if let Some(ref code) = b.error_code {
                        if let Some(resolved) = error_defs.get(code) {
                            b.error_code = Some(resolved.clone());
                        }
                    }
                }
                FlowElement::SubProcess(sp) => {
                    resolve_error_codes(&mut sp.inner_elements, error_defs);
                }
                _ => {}
            }
        }
    }
    resolve_error_codes(&mut elements, &error_definitions);

    fn resolve_escalation_codes(
        elements: &mut [FlowElement],
        esc_defs: &std::collections::HashMap<String, String>,
    ) {
        for elem in elements.iter_mut() {
            match elem {
                FlowElement::EscalationEndEvent(e) => {
                    if let Some(ref code) = e.escalation_code {
                        if let Some(resolved) = esc_defs.get(code) {
                            e.escalation_code = Some(resolved.clone());
                        }
                    }
                }
                FlowElement::EscalationIntermediateThrowEvent(e) => {
                    if let Some(ref code) = e.escalation_code {
                        if let Some(resolved) = esc_defs.get(code) {
                            e.escalation_code = Some(resolved.clone());
                        }
                    }
                }
                FlowElement::EscalationBoundaryEvent(e) => {
                    if let Some(ref code) = e.escalation_code {
                        if let Some(resolved) = esc_defs.get(code) {
                            e.escalation_code = Some(resolved.clone());
                        }
                    }
                }
                FlowElement::SubProcess(sp) => {
                    resolve_escalation_codes(&mut sp.inner_elements, esc_defs);
                }
                _ => {}
            }
        }
    }
    resolve_escalation_codes(&mut elements, &escalation_definitions);

    Ok(ProcessDefinition {
        id,
        name: process_name,
        elements,
        sequence_flows,
        annotations,
        associations,
    })
}

/// Returns (active_elements, active_flows) for the current parse scope.
/// This takes ownership of the refs briefly, so caller must not hold other refs to the same data.
fn active_lists_mut<'a>(
    elements: &'a mut Vec<FlowElement>,
    sequence_flows: &'a mut Vec<SequenceFlow>,
    subprocess_stack: &'a mut [SubProcessCtx],
) -> (&'a mut Vec<FlowElement>, &'a mut Vec<SequenceFlow>) {
    if let Some(ctx) = subprocess_stack.last_mut() {
        (&mut ctx.inner_elements, &mut ctx.inner_flows)
    } else {
        (elements, sequence_flows)
    }
}

fn local_name_str(name: quick_xml::name::QName) -> String {
    let raw = std::str::from_utf8(name.as_ref()).unwrap_or("");
    raw.split(':').next_back().unwrap_or(raw).to_string()
}

fn attr_value(e: &quick_xml::events::BytesStart, key: &str) -> Option<String> {
    e.attributes()
        .filter_map(|a| a.ok())
        .find(|a| {
            let k = std::str::from_utf8(a.key.as_ref()).unwrap_or("");
            k == key || k.ends_with(&format!(":{key}"))
        })
        .and_then(|a| a.unescape_value().ok())
        .map(|v| v.into_owned())
}

#[allow(clippy::too_many_arguments)]
fn flush_element(
    elements: &mut Vec<FlowElement>,
    element_type: &mut Option<String>,
    element_id: &mut Option<String>,
    element_name: &mut Option<String>,
    outgoing: &mut Vec<String>,
    incoming: &mut Vec<String>,
    timer_duration: &mut Option<String>,
    timer_kind: &mut Option<TimerKind>,
    multi_instance_input_ref: &mut Option<String>,
    multi_instance_sequential: bool,
    boundary_attached_to: Option<String>,
    boundary_event_type: &str,
    message_ref: &mut Option<String>,
    signal_ref: &mut Option<String>,
    orrery_topic: Option<String>,
    gateway_default: Option<String>,
    message_correlation_key: Option<String>,
    boundary_interrupting: bool,
    script_format: &mut Option<String>,
    script_body: &mut Option<String>,
    result_variable: &mut Option<String>,
    error_code: Option<String>,
    escalation_code: &mut Option<String>,
    link_name: &mut Option<String>,
) -> Result<(), ParseError> {
    if let (Some(t), Some(id)) = (element_type.take(), element_id.take()) {
        let name = element_name.take();
        let el = match t.as_str() {
            "startEvent" => {
                // If a timerEventDefinition was found inside, create a TimerStartEvent
                let timer = match (timer_duration.take(), timer_kind.take()) {
                    (Some(expr), Some(kind)) => Some(TimerDefinition {
                        kind,
                        expression: expr,
                    }),
                    (Some(expr), None) => Some(TimerDefinition {
                        kind: TimerKind::Duration,
                        expression: expr,
                    }),
                    _ => None,
                };
                if timer.is_some() {
                    FlowElement::TimerStartEvent(TimerStartEvent {
                        id,
                        name,
                        outgoing: std::mem::take(outgoing),
                        timer,
                    })
                } else {
                    FlowElement::StartEvent(StartEvent {
                        id,
                        name,
                        outgoing: std::mem::take(outgoing),
                    })
                }
            }
            "messageStartEvent" => FlowElement::MessageStartEvent(MessageStartEvent {
                id,
                name,
                outgoing: std::mem::take(outgoing),
                message_name: message_ref.take().unwrap_or_default(),
            }),
            "signalStartEvent" => FlowElement::SignalStartEvent(SignalStartEvent {
                id,
                name,
                outgoing: std::mem::take(outgoing),
                signal_ref: signal_ref.take().unwrap_or_default(),
            }),
            "signalEndEvent" => FlowElement::SignalEndEvent(SignalEndEvent {
                id,
                name,
                outgoing: std::mem::take(outgoing),
                signal_ref: signal_ref.take().unwrap_or_default(),
            }),
            "signalThrowEvent" => {
                FlowElement::SignalIntermediateThrowEvent(SignalIntermediateThrowEvent {
                    id,
                    name,
                    outgoing: std::mem::take(outgoing),
                    signal_ref: signal_ref.take().unwrap_or_default(),
                })
            }
            "endEvent" => FlowElement::EndEvent(EndEvent {
                id,
                name,
                outgoing: std::mem::take(outgoing),
            }),
            "errorEndEvent" => FlowElement::ErrorEndEvent(ErrorEndEvent {
                id,
                name,
                outgoing: std::mem::take(outgoing),
                error_code,
            }),
            "terminateEndEvent" => FlowElement::TerminateEndEvent(TerminateEndEvent {
                id,
                name,
                outgoing: std::mem::take(outgoing),
            }),
            "escalationThrowEvent" => {
                FlowElement::EscalationIntermediateThrowEvent(EscalationIntermediateThrowEvent {
                    id,
                    name,
                    outgoing: std::mem::take(outgoing),
                    escalation_code: escalation_code.take(),
                })
            }
            "escalationEndEvent" => FlowElement::EscalationEndEvent(EscalationEndEvent {
                id,
                name,
                outgoing: std::mem::take(outgoing),
                escalation_code: escalation_code.take(),
            }),
            "linkThrowEvent" => {
                FlowElement::LinkIntermediateThrowEvent(LinkIntermediateThrowEvent {
                    id,
                    name,
                    outgoing: std::mem::take(outgoing),
                    link_name: link_name.take().unwrap_or_default(),
                })
            }
            "linkCatchEvent" => {
                FlowElement::LinkIntermediateCatchEvent(LinkIntermediateCatchEvent {
                    id,
                    name,
                    outgoing: std::mem::take(outgoing),
                    link_name: link_name.take().unwrap_or_default(),
                })
            }
            "messageEndEvent" => FlowElement::MessageEndEvent(MessageEndEvent {
                id,
                name,
                outgoing: std::mem::take(outgoing),
                message_name: message_ref.take().unwrap_or_default(),
            }),
            "serviceTask" => FlowElement::ServiceTask(ServiceTask {
                id,
                name,
                outgoing: std::mem::take(outgoing),
                topic: orrery_topic,
            }),
            "multiInstanceTask" => {
                let input_ref = multi_instance_input_ref.take().unwrap_or_default();
                FlowElement::MultiInstanceTask(MultiInstanceTask {
                    id,
                    name,
                    incoming: std::mem::take(incoming),
                    outgoing: std::mem::take(outgoing),
                    loop_data_input_ref: input_ref,
                    is_sequential: multi_instance_sequential,
                })
            }
            "exclusiveGateway" => FlowElement::ExclusiveGateway(ExclusiveGateway {
                id,
                name,
                outgoing: std::mem::take(outgoing),
                default: gateway_default,
            }),
            "parallelGateway" => FlowElement::ParallelGateway(ParallelGateway {
                id,
                name,
                incoming: std::mem::take(incoming),
                outgoing: std::mem::take(outgoing),
            }),
            "inclusiveGateway" => FlowElement::InclusiveGateway(InclusiveGateway {
                id,
                name,
                incoming: std::mem::take(incoming),
                outgoing: std::mem::take(outgoing),
                default: gateway_default,
            }),
            "eventBasedGateway" => FlowElement::EventBasedGateway(EventBasedGateway {
                id,
                name,
                incoming: std::mem::take(incoming),
                outgoing: std::mem::take(outgoing),
            }),
            "intermediateCatchEvent" => {
                let timer = match (timer_duration.take(), timer_kind.take()) {
                    (Some(expr), Some(kind)) => Some(TimerDefinition {
                        kind,
                        expression: expr,
                    }),
                    (Some(expr), None) => Some(TimerDefinition {
                        kind: TimerKind::Duration,
                        expression: expr,
                    }),
                    _ => None,
                };
                FlowElement::TimerIntermediateEvent(TimerIntermediateEvent {
                    id,
                    name,
                    outgoing: std::mem::take(outgoing),
                    timer,
                })
            }
            "messageCatchEvent" => {
                let msg_ref = message_ref.take().unwrap_or_default();
                FlowElement::MessageIntermediateCatchEvent(MessageIntermediateCatchEvent {
                    id,
                    name,
                    message_name: msg_ref,
                    correlation_key: message_correlation_key,
                    outgoing: std::mem::take(outgoing),
                })
            }
            "receiveTask" => {
                let msg_ref = message_ref.take().unwrap_or_default();
                FlowElement::ReceiveTask(ReceiveTask {
                    id,
                    name,
                    message_name: msg_ref,
                    correlation_key: message_correlation_key,
                    outgoing: std::mem::take(outgoing),
                })
            }
            "signalCatchEvent" => {
                FlowElement::SignalIntermediateCatchEvent(SignalIntermediateCatchEvent {
                    id,
                    name,
                    signal_ref: signal_ref.take().unwrap_or_default(),
                    outgoing: std::mem::take(outgoing),
                })
            }
            "intermediateThrowEvent" => {
                FlowElement::IntermediateThrowEvent(IntermediateThrowEvent {
                    id,
                    name,
                    outgoing: std::mem::take(outgoing),
                })
            }
            "scriptTask" => FlowElement::ScriptTask(ScriptTask {
                id,
                name,
                outgoing: std::mem::take(outgoing),
                incoming: std::mem::take(incoming),
                script_format: script_format.take().unwrap_or_else(|| "rhai".to_string()),
                script: script_body.take().unwrap_or_default(),
                result_variable: result_variable.take(),
            }),
            "messageThrowEvent" => {
                FlowElement::MessageIntermediateThrowEvent(MessageIntermediateThrowEvent {
                    id,
                    name,
                    outgoing: std::mem::take(outgoing),
                    message_name: message_ref.take().unwrap_or_default(),
                })
            }
            "boundaryEvent" => {
                if boundary_event_type == "message" {
                    FlowElement::MessageBoundaryEvent(MessageBoundaryEvent {
                        id,
                        name,
                        outgoing: std::mem::take(outgoing),
                        attached_to_ref: boundary_attached_to.unwrap_or_default(),
                        message_name: message_ref.take().unwrap_or_default(),
                        correlation_key: message_correlation_key,
                        is_interrupting: boundary_interrupting,
                    })
                } else if boundary_event_type == "timer" {
                    let timer = match (timer_duration.take(), timer_kind.take()) {
                        (Some(expr), Some(kind)) => TimerDefinition {
                            kind,
                            expression: expr,
                        },
                        (Some(expr), None) => TimerDefinition {
                            kind: TimerKind::Duration,
                            expression: expr,
                        },
                        _ => TimerDefinition::zero_duration(),
                    };
                    FlowElement::TimerBoundaryEvent(TimerBoundaryEvent {
                        id,
                        name,
                        outgoing: std::mem::take(outgoing),
                        attached_to_ref: boundary_attached_to.unwrap_or_default(),
                        timer,
                        is_interrupting: boundary_interrupting,
                    })
                } else if boundary_event_type == "signal" {
                    FlowElement::SignalBoundaryEvent(SignalBoundaryEvent {
                        id,
                        name,
                        outgoing: std::mem::take(outgoing),
                        attached_to_ref: boundary_attached_to.unwrap_or_default(),
                        signal_ref: signal_ref.take().unwrap_or_default(),
                        is_interrupting: boundary_interrupting,
                    })
                } else if boundary_event_type == "escalation" {
                    FlowElement::EscalationBoundaryEvent(EscalationBoundaryEvent {
                        id,
                        name,
                        outgoing: std::mem::take(outgoing),
                        attached_to_ref: boundary_attached_to.unwrap_or_default(),
                        escalation_code: escalation_code.take(),
                        is_interrupting: boundary_interrupting,
                    })
                } else {
                    FlowElement::BoundaryEvent(BoundaryEvent {
                        id,
                        name,
                        outgoing: std::mem::take(outgoing),
                        attached_to_ref: boundary_attached_to.unwrap_or_default(),
                        event_type: boundary_event_type.to_string(),
                        error_code,
                    })
                }
            }
            "espStartEvent" => FlowElement::EventSubProcessStartEvent(EventSubProcessStartEvent {
                id,
                name,
                outgoing: std::mem::take(outgoing),
                trigger: EventSubProcessTrigger::Error { error_code: None },
                is_interrupting: boundary_interrupting,
            }),
            "espStartEvent_error" => {
                FlowElement::EventSubProcessStartEvent(EventSubProcessStartEvent {
                    id,
                    name,
                    outgoing: std::mem::take(outgoing),
                    trigger: EventSubProcessTrigger::Error { error_code },
                    is_interrupting: boundary_interrupting,
                })
            }
            "espStartEvent_escalation" => {
                FlowElement::EventSubProcessStartEvent(EventSubProcessStartEvent {
                    id,
                    name,
                    outgoing: std::mem::take(outgoing),
                    trigger: EventSubProcessTrigger::Escalation {
                        escalation_code: escalation_code.take(),
                    },
                    is_interrupting: boundary_interrupting,
                })
            }
            "espStartEvent_message" => {
                let msg_ref = message_ref.take().unwrap_or_default();
                FlowElement::EventSubProcessStartEvent(EventSubProcessStartEvent {
                    id,
                    name,
                    outgoing: std::mem::take(outgoing),
                    trigger: EventSubProcessTrigger::Message {
                        message_name: msg_ref,
                        correlation_key: message_correlation_key,
                    },
                    is_interrupting: boundary_interrupting,
                })
            }
            "espStartEvent_signal" => {
                FlowElement::EventSubProcessStartEvent(EventSubProcessStartEvent {
                    id,
                    name,
                    outgoing: std::mem::take(outgoing),
                    trigger: EventSubProcessTrigger::Signal {
                        signal_ref: signal_ref.take().unwrap_or_default(),
                    },
                    is_interrupting: boundary_interrupting,
                })
            }
            "espStartEvent_timer" => {
                let timer = match (timer_duration.take(), timer_kind.take()) {
                    (Some(expr), Some(kind)) => TimerDefinition {
                        kind,
                        expression: expr,
                    },
                    (Some(expr), None) => TimerDefinition {
                        kind: TimerKind::Duration,
                        expression: expr,
                    },
                    _ => TimerDefinition::zero_duration(),
                };
                FlowElement::EventSubProcessStartEvent(EventSubProcessStartEvent {
                    id,
                    name,
                    outgoing: std::mem::take(outgoing),
                    trigger: EventSubProcessTrigger::Timer { timer },
                    is_interrupting: boundary_interrupting,
                })
            }
            other => return Err(ParseError::UnsupportedElement(other.to_string())),
        };
        elements.push(el);
    }
    outgoing.clear();
    incoming.clear();
    Ok(())
}

fn is_diagram_element(name: &str) -> bool {
    name.contains("DI")
        || name.contains("Shape")
        || name.contains("Edge")
        || name.contains("Bounds")
        || name.contains("Waypoint")
        || name.contains("diagram")
        || name.contains("Diagram")
        || name.contains("label")
        || name.contains("Label")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_parallel_gateway() {
        let xml = r#"<?xml version="1.0"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <process id="p">
    <startEvent id="s"><outgoing>f0</outgoing></startEvent>
    <sequenceFlow id="f0" sourceRef="s" targetRef="fork"/>
    <parallelGateway id="fork">
      <incoming>f0</incoming>
      <outgoing>fa</outgoing>
      <outgoing>fb</outgoing>
    </parallelGateway>
    <sequenceFlow id="fa" sourceRef="fork" targetRef="join"/>
    <sequenceFlow id="fb" sourceRef="fork" targetRef="join"/>
    <parallelGateway id="join">
      <incoming>fa</incoming>
      <incoming>fb</incoming>
      <outgoing>fe</outgoing>
    </parallelGateway>
    <sequenceFlow id="fe" sourceRef="join" targetRef="e"/>
    <endEvent id="e"/>
  </process>
</definitions>"#;
        let def = parse_bpmn(xml).unwrap();
        let gws: Vec<_> = def
            .elements
            .iter()
            .filter(|e| matches!(e, FlowElement::ParallelGateway(_)))
            .collect();
        assert_eq!(gws.len(), 2);
        if let FlowElement::ParallelGateway(fork) = gws[0] {
            assert_eq!(fork.outgoing.len(), 2);
            assert_eq!(fork.incoming.len(), 1);
        }
        if let FlowElement::ParallelGateway(join) = gws[1] {
            assert_eq!(join.incoming.len(), 2);
            assert_eq!(join.outgoing.len(), 1);
        }
    }

    #[test]
    fn parallel_gateway_has_incoming_and_outgoing() {
        let gw = ParallelGateway {
            id: "fork".to_string(),
            name: None,
            incoming: vec!["f0".to_string()],
            outgoing: vec!["fa".to_string(), "fb".to_string()],
        };
        assert_eq!(gw.incoming.len(), 1);
        assert_eq!(gw.outgoing.len(), 2);
    }

    #[test]
    fn parses_subprocess() {
        let xml = r#"<?xml version="1.0"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <process id="p">
    <startEvent id="s"><outgoing>f1</outgoing></startEvent>
    <sequenceFlow id="f1" sourceRef="s" targetRef="sub1"/>
    <subProcess id="sub1">
      <outgoing>f2</outgoing>
      <startEvent id="sub_s"><outgoing>sf1</outgoing></startEvent>
      <sequenceFlow id="sf1" sourceRef="sub_s" targetRef="inner_task"/>
      <serviceTask id="inner_task"><outgoing>sf2</outgoing></serviceTask>
      <sequenceFlow id="sf2" sourceRef="inner_task" targetRef="sub_e"/>
      <endEvent id="sub_e"/>
    </subProcess>
    <sequenceFlow id="f2" sourceRef="sub1" targetRef="e"/>
    <endEvent id="e"/>
  </process>
</definitions>"#;
        let def = parse_bpmn(xml).unwrap();
        let sp = def
            .elements
            .iter()
            .find(|e| matches!(e, FlowElement::SubProcess(_)))
            .expect("should have subprocess");
        if let FlowElement::SubProcess(sp) = sp {
            assert_eq!(sp.id, "sub1");
            assert_eq!(sp.inner_elements.len(), 3); // sub_s, inner_task, sub_e
            assert_eq!(sp.inner_flows.len(), 2);
        }
    }

    #[test]
    fn parses_message_intermediate_catch_event() {
        let xml = r#"<?xml version="1.0"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <process id="p">
    <startEvent id="s"><outgoing>f1</outgoing></startEvent>
    <sequenceFlow id="f1" sourceRef="s" targetRef="msg1"/>
    <intermediateCatchEvent id="msg1">
      <messageEventDefinition messageRef="OrderApproved"/>
      <outgoing>f2</outgoing>
    </intermediateCatchEvent>
    <sequenceFlow id="f2" sourceRef="msg1" targetRef="e"/>
    <endEvent id="e"/>
  </process>
</definitions>"#;
        let def = parse_bpmn(xml).unwrap();
        let msg = def
            .elements
            .iter()
            .find(|e| e.id() == "msg1")
            .expect("msg1 not found");
        match msg {
            FlowElement::MessageIntermediateCatchEvent(m) => {
                assert_eq!(m.message_name, "OrderApproved");
                assert_eq!(m.outgoing, vec!["f2"]);
            }
            other => panic!("expected MessageIntermediateCatchEvent, got {:?}", other),
        }
    }

    #[test]
    fn parses_signal_intermediate_catch_event() {
        let xml = r#"<?xml version="1.0"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <process id="p">
    <startEvent id="s"><outgoing>f1</outgoing></startEvent>
    <sequenceFlow id="f1" sourceRef="s" targetRef="sig1"/>
    <intermediateCatchEvent id="sig1">
      <signalEventDefinition signalRef="PaymentReceived"/>
      <outgoing>f2</outgoing>
    </intermediateCatchEvent>
    <sequenceFlow id="f2" sourceRef="sig1" targetRef="e"/>
    <endEvent id="e"/>
  </process>
</definitions>"#;
        let def = parse_bpmn(xml).unwrap();
        let sig = def
            .elements
            .iter()
            .find(|e| e.id() == "sig1")
            .expect("sig1 not found");
        match sig {
            FlowElement::SignalIntermediateCatchEvent(s) => {
                assert_eq!(s.signal_ref, "PaymentReceived");
            }
            other => panic!("expected SignalIntermediateCatchEvent, got {:?}", other),
        }
    }

    #[test]
    fn exclusive_gateway_parses_default_flow() {
        let xml = r#"<?xml version="1.0"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <process id="p">
    <startEvent id="s"><outgoing>f0</outgoing></startEvent>
    <sequenceFlow id="f0" sourceRef="s" targetRef="gw"/>
    <exclusiveGateway id="gw" default="f_default">
      <incoming>f0</incoming>
      <outgoing>f_cond</outgoing>
      <outgoing>f_default</outgoing>
    </exclusiveGateway>
    <sequenceFlow id="f_cond" sourceRef="gw" targetRef="e"/>
    <sequenceFlow id="f_default" sourceRef="gw" targetRef="e"/>
    <endEvent id="e"/>
  </process>
</definitions>"#;
        let def = parse_bpmn(xml).unwrap();
        let gw = def.elements.iter().find(|e| e.id() == "gw").expect("gw");
        match gw {
            FlowElement::ExclusiveGateway(g) => assert_eq!(g.default.as_deref(), Some("f_default")),
            other => panic!("expected ExclusiveGateway, got {:?}", other),
        }
    }

    #[test]
    fn empty_service_task_reads_external_topic() {
        let xml = r#"<?xml version="1.0"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <process id="p">
    <startEvent id="s" />
    <serviceTask id="t1" camunda:type="external" camunda:topic="payments"
                 xmlns:camunda="http://camunda.org/schema/1.0/bpmn" />
    <sequenceFlow id="f1" sourceRef="s" targetRef="t1" />
  </process>
</definitions>"#;
        let def = parse_bpmn(xml).unwrap();
        let task = def.elements.iter().find(|e| e.id() == "t1").expect("t1");
        match task {
            FlowElement::ServiceTask(t) => assert_eq!(t.topic.as_deref(), Some("payments")),
            other => panic!("expected ServiceTask, got {:?}", other),
        }
    }

    #[test]
    fn parse_receive_task_basic() {
        let xml = r#"<?xml version="1.0"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL"
             xmlns:zeebe="http://camunda.org/schema/zeebe/1.0">
  <message id="Msg_Order" name="order-received">
    <extensionElements>
      <zeebe:subscription correlationKey="= orderId"/>
    </extensionElements>
  </message>
  <process id="proc1">
    <startEvent id="start1"><outgoing>f1</outgoing></startEvent>
    <sequenceFlow id="f1" sourceRef="start1" targetRef="rt1"/>
    <receiveTask id="rt1" name="Receive Order" messageRef="Msg_Order">
      <outgoing>f2</outgoing>
    </receiveTask>
    <sequenceFlow id="f2" sourceRef="rt1" targetRef="end1"/>
    <endEvent id="end1"/>
  </process>
</definitions>"#;
        let def = parse_bpmn(xml).unwrap();
        let elem = def.elements.iter().find(|e| e.id() == "rt1").unwrap();
        match elem {
            FlowElement::ReceiveTask(rt) => {
                assert_eq!(rt.message_name, "order-received");
                assert_eq!(rt.correlation_key.as_deref(), Some("= orderId"));
                assert_eq!(rt.outgoing, vec!["f2"]);
                assert_eq!(rt.name.as_deref(), Some("Receive Order"));
            }
            other => panic!("Expected ReceiveTask, got {:?}", other),
        }
    }

    #[test]
    fn parse_receive_task_no_correlation_key() {
        let xml = r#"<?xml version="1.0"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <message id="Msg_Notify" name="notify-user"/>
  <process id="proc2">
    <startEvent id="s1"><outgoing>f1</outgoing></startEvent>
    <sequenceFlow id="f1" sourceRef="s1" targetRef="rt1"/>
    <receiveTask id="rt1" messageRef="Msg_Notify">
      <outgoing>f2</outgoing>
    </receiveTask>
    <sequenceFlow id="f2" sourceRef="rt1" targetRef="e1"/>
    <endEvent id="e1"/>
  </process>
</definitions>"#;
        let def = parse_bpmn(xml).unwrap();
        let elem = def.elements.iter().find(|e| e.id() == "rt1").unwrap();
        match elem {
            FlowElement::ReceiveTask(rt) => {
                assert_eq!(rt.message_name, "notify-user");
                assert_eq!(rt.correlation_key, None);
            }
            other => panic!("Expected ReceiveTask, got {:?}", other),
        }
    }

    /// Regression: bpmn.io places <message> elements AFTER </process>.
    /// The parser must still resolve messageRef correctly via post-processing.
    #[test]
    fn parse_receive_task_message_after_process() {
        let xml = r#"<?xml version="1.0"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL"
             xmlns:zeebe="http://camunda.org/schema/zeebe/1.0">
  <process id="proc1">
    <startEvent id="start1"><outgoing>f1</outgoing></startEvent>
    <sequenceFlow id="f1" sourceRef="start1" targetRef="rt1"/>
    <receiveTask id="rt1" name="Wait for Payment" messageRef="Msg_Pay">
      <outgoing>f2</outgoing>
    </receiveTask>
    <sequenceFlow id="f2" sourceRef="rt1" targetRef="end1"/>
    <endEvent id="end1"/>
  </process>
  <message id="Msg_Pay" name="payment-received">
    <extensionElements>
      <zeebe:subscription correlationKey="= orderId"/>
    </extensionElements>
  </message>
</definitions>"#;
        let def = parse_bpmn(xml).unwrap();
        let elem = def.elements.iter().find(|e| e.id() == "rt1").unwrap();
        match elem {
            FlowElement::ReceiveTask(rt) => {
                assert_eq!(rt.message_name, "payment-received");
                assert_eq!(rt.correlation_key.as_deref(), Some("= orderId"));
            }
            other => panic!("Expected ReceiveTask, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_inclusive_gateway() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL" id="ig">
  <process id="p1" isExecutable="true">
    <startEvent id="start"><outgoing>f1</outgoing></startEvent>
    <sequenceFlow id="f1" sourceRef="start" targetRef="igw"/>
    <inclusiveGateway id="igw" default="f3">
      <incoming>f1</incoming>
      <outgoing>f2</outgoing>
      <outgoing>f3</outgoing>
    </inclusiveGateway>
    <sequenceFlow id="f2" sourceRef="igw" targetRef="end">
      <conditionExpression>amount &gt; 100</conditionExpression>
    </sequenceFlow>
    <sequenceFlow id="f3" sourceRef="igw" targetRef="end"/>
    <endEvent id="end"><incoming>f2</incoming></endEvent>
  </process>
</definitions>"#;
        let def = parse_bpmn(xml).unwrap();
        let gw = def
            .elements
            .iter()
            .find_map(|e| {
                if let FlowElement::InclusiveGateway(g) = e {
                    Some(g)
                } else {
                    None
                }
            })
            .expect("InclusiveGateway not found");
        assert_eq!(gw.id, "igw");
        assert_eq!(gw.default, Some("f3".to_string()));
        assert_eq!(gw.outgoing.len(), 2);
        assert_eq!(gw.incoming.len(), 1);
    }

    #[test]
    fn test_parse_event_based_gateway() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL" id="ebg">
  <process id="p1" isExecutable="true">
    <startEvent id="start"><outgoing>f1</outgoing></startEvent>
    <sequenceFlow id="f1" sourceRef="start" targetRef="ebg"/>
    <eventBasedGateway id="ebg">
      <incoming>f1</incoming>
      <outgoing>f2</outgoing>
      <outgoing>f3</outgoing>
    </eventBasedGateway>
    <sequenceFlow id="f2" sourceRef="ebg" targetRef="start"/>
    <sequenceFlow id="f3" sourceRef="ebg" targetRef="start"/>
  </process>
</definitions>"#;
        let def = parse_bpmn(xml).unwrap();
        let gw = def
            .elements
            .iter()
            .find_map(|e| {
                if let FlowElement::EventBasedGateway(g) = e {
                    Some(g)
                } else {
                    None
                }
            })
            .expect("EventBasedGateway not found");
        assert_eq!(gw.id, "ebg");
        assert_eq!(gw.outgoing.len(), 2);
        assert_eq!(gw.incoming.len(), 1);
    }

    #[test]
    fn parse_error_event_subprocess() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <error id="Err1" errorCode="MY_ERROR"/>
  <process id="p1" isExecutable="true">
    <startEvent id="start"><outgoing>sf1</outgoing></startEvent>
    <sequenceFlow id="sf1" sourceRef="start" targetRef="task1"/>
    <serviceTask id="task1"><outgoing>sf2</outgoing></serviceTask>
    <sequenceFlow id="sf2" sourceRef="task1" targetRef="end"/>
    <endEvent id="end"/>
    <subProcess id="esp1" triggeredByEvent="true">
      <startEvent id="esp_start" isInterrupting="true">
        <errorEventDefinition id="eed1" errorRef="Err1"/>
        <outgoing>sf_esp1</outgoing>
      </startEvent>
      <sequenceFlow id="sf_esp1" sourceRef="esp_start" targetRef="esp_task"/>
      <serviceTask id="esp_task"><outgoing>sf_esp2</outgoing></serviceTask>
      <sequenceFlow id="sf_esp2" sourceRef="esp_task" targetRef="esp_end"/>
      <endEvent id="esp_end"/>
    </subProcess>
  </process>
</definitions>"#;

        let def = parse_bpmn(xml).unwrap();
        let esp = def
            .elements
            .iter()
            .find_map(|e| match e {
                FlowElement::EventSubProcess(esp) => Some(esp),
                _ => None,
            })
            .expect("EventSubProcess not found");

        assert_eq!(esp.id, "esp1");

        let start = esp
            .inner_elements
            .iter()
            .find_map(|e| match e {
                FlowElement::EventSubProcessStartEvent(s) => Some(s),
                _ => None,
            })
            .expect("EventSubProcessStartEvent not found");

        assert_eq!(start.id, "esp_start");
        assert!(start.is_interrupting);
        assert!(
            matches!(start.trigger, EventSubProcessTrigger::Error { error_code: Some(ref c) } if c == "MY_ERROR")
        );

        // Inner task must be in inner_elements
        assert!(esp.inner_elements.iter().any(|e| e.id() == "esp_task"));
        // ESP start must NOT be a regular StartEvent
        assert!(!def
            .elements
            .iter()
            .any(|e| matches!(e, FlowElement::StartEvent(s) if s.id == "esp_start")));
    }

    #[test]
    fn parse_non_interrupting_escalation_event_subprocess() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <escalation id="Esc1" escalationCode="ESC_CODE"/>
  <process id="p1" isExecutable="true">
    <startEvent id="start"><outgoing>sf1</outgoing></startEvent>
    <sequenceFlow id="sf1" sourceRef="start" targetRef="end"/>
    <endEvent id="end"/>
    <subProcess id="esp2" triggeredByEvent="true">
      <startEvent id="esp_start2" isInterrupting="false">
        <escalationEventDefinition escalationRef="Esc1"/>
        <outgoing>sf_e1</outgoing>
      </startEvent>
      <sequenceFlow id="sf_e1" sourceRef="esp_start2" targetRef="esp_end2"/>
      <endEvent id="esp_end2"/>
    </subProcess>
  </process>
</definitions>"#;

        let def = parse_bpmn(xml).unwrap();
        let esp = def
            .elements
            .iter()
            .find_map(|e| match e {
                FlowElement::EventSubProcess(e) => Some(e),
                _ => None,
            })
            .expect("EventSubProcess not found");

        let start = esp
            .inner_elements
            .iter()
            .find_map(|e| match e {
                FlowElement::EventSubProcessStartEvent(s) => Some(s),
                _ => None,
            })
            .expect("EventSubProcessStartEvent not found");

        assert!(!start.is_interrupting);
        assert!(
            matches!(start.trigger, EventSubProcessTrigger::Escalation { escalation_code: Some(ref c) } if c == "ESC_CODE")
        );
    }

    #[test]
    fn parse_message_event_subprocess() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <message id="Msg1" name="cancel-order"/>
  <process id="p1" isExecutable="true">
    <startEvent id="start"><outgoing>sf1</outgoing></startEvent>
    <sequenceFlow id="sf1" sourceRef="start" targetRef="end"/>
    <endEvent id="end"/>
    <subProcess id="esp3" triggeredByEvent="true">
      <startEvent id="esp_start3" isInterrupting="true">
        <messageEventDefinition messageRef="Msg1"/>
        <outgoing>sf_m1</outgoing>
      </startEvent>
      <sequenceFlow id="sf_m1" sourceRef="esp_start3" targetRef="esp_end3"/>
      <endEvent id="esp_end3"/>
    </subProcess>
  </process>
</definitions>"#;

        let def = parse_bpmn(xml).unwrap();
        let esp = def
            .elements
            .iter()
            .find_map(|e| match e {
                FlowElement::EventSubProcess(e) => Some(e),
                _ => None,
            })
            .unwrap();
        let start = esp
            .inner_elements
            .iter()
            .find_map(|e| match e {
                FlowElement::EventSubProcessStartEvent(s) => Some(s),
                _ => None,
            })
            .unwrap();
        assert!(
            matches!(start.trigger, EventSubProcessTrigger::Message { ref message_name, .. } if message_name == "cancel-order")
        );
    }

    #[test]
    fn parse_bpmn_text_annotation_and_association() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<bpmn:definitions xmlns:bpmn="http://www.omg.org/spec/BPMN/20100524/MODEL"
    id="Def1" targetNamespace="test">
  <bpmn:process id="p1" isExecutable="true">
    <bpmn:startEvent id="start"><bpmn:outgoing>f1</bpmn:outgoing></bpmn:startEvent>
    <bpmn:endEvent id="end"><bpmn:incoming>f1</bpmn:incoming></bpmn:endEvent>
    <bpmn:sequenceFlow id="f1" sourceRef="start" targetRef="end"/>
    <bpmn:textAnnotation id="Ann1">
      <bpmn:text>This is a note</bpmn:text>
    </bpmn:textAnnotation>
    <bpmn:association id="Assoc1" associationDirection="None"
        sourceRef="start" targetRef="Ann1"/>
  </bpmn:process>
</bpmn:definitions>"#;
        let def = parse_bpmn(xml).expect("parse should succeed");
        assert_eq!(def.annotations.len(), 1);
        assert_eq!(def.annotations[0].id, "Ann1");
        assert_eq!(def.annotations[0].text, "This is a note");
        assert_eq!(def.associations.len(), 1);
        assert_eq!(def.associations[0].id, "Assoc1");
        assert_eq!(def.associations[0].source_ref, "start");
        assert_eq!(def.associations[0].target_ref, "Ann1");
    }

    #[test]
    fn parse_bpmn_annotation_not_in_flow_elements() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<bpmn:definitions xmlns:bpmn="http://www.omg.org/spec/BPMN/20100524/MODEL"
    id="Def1" targetNamespace="test">
  <bpmn:process id="p1" isExecutable="true">
    <bpmn:startEvent id="start"><bpmn:outgoing>f1</bpmn:outgoing></bpmn:startEvent>
    <bpmn:endEvent id="end"><bpmn:incoming>f1</bpmn:incoming></bpmn:endEvent>
    <bpmn:sequenceFlow id="f1" sourceRef="start" targetRef="end"/>
    <bpmn:textAnnotation id="Ann1">
      <bpmn:text>Note</bpmn:text>
    </bpmn:textAnnotation>
  </bpmn:process>
</bpmn:definitions>"#;
        let def = parse_bpmn(xml).expect("parse should succeed");
        assert!(
            !def.elements.iter().any(|e| e.id() == "Ann1"),
            "annotation id must not appear in flow elements"
        );
    }

    #[test]
    fn parse_bpmn_annotation_without_association_does_not_panic() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<bpmn:definitions xmlns:bpmn="http://www.omg.org/spec/BPMN/20100524/MODEL"
    id="Def1" targetNamespace="test">
  <bpmn:process id="p1" isExecutable="true">
    <bpmn:startEvent id="start"><bpmn:outgoing>f1</bpmn:outgoing></bpmn:startEvent>
    <bpmn:endEvent id="end"><bpmn:incoming>f1</bpmn:incoming></bpmn:endEvent>
    <bpmn:sequenceFlow id="f1" sourceRef="start" targetRef="end"/>
    <bpmn:textAnnotation id="Ann1">
      <bpmn:text>Standalone note</bpmn:text>
    </bpmn:textAnnotation>
  </bpmn:process>
</bpmn:definitions>"#;
        let def = parse_bpmn(xml).expect("parse should succeed without panicking");
        assert_eq!(def.annotations.len(), 1);
        assert_eq!(def.associations.len(), 0);
    }
}
